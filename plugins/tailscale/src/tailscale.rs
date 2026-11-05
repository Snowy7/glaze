//! Replaceable unstable LocalAPI adapter. Never watches engine/peer traffic or starts a VPN node.
use crate::{Action, Publish, transport};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// InitialState | InitialPrefs | NoNetMap | InitialStatus. Old incompatible versions fail visibly.
pub const WATCH_MASK: u32 = 2 | 4 | 8192 | 16384;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub state: String,
    pub addresses: Vec<String>,
    pub exit_node: Option<String>,
    pub peers: Vec<Peer>,
    pub peers_known: bool,
    pub exit_node_id: Option<String>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}

/// A masked preferences patch changes only the fields named by an explicit user action.
pub fn prefs(action: &Action) -> Option<Value> {
    match action {
        Action::Connected(connected) => Some(json!({"WantRunning":connected,"WantRunningSet":true})),
        Action::ExitNode(id) => Some(json!({"ExitNodeID":id,"ExitNodeIDSet":true,"ExitNodeIP":"","ExitNodeIPSet":true,"AutoExitNode":"","AutoExitNodeSet":true})),
        Action::Refresh => None,
    }
}

async fn refresh(pipe: &str, state: &mut Snapshot, publish: &Publish, live: bool) -> Result<()> {
    let status = transport::pipe_json(pipe, "GET", "/localapi/v0/status", None).await?;
    // A full status replaces vanished peers and clears an exit node that was switched off.
    *state = Snapshot::default();
    apply(state, &json!({"InitialStatus":status}));
    ensure!(!state.state.is_empty(), "Tailscale returned an incompatible status snapshot");
    publish.set(state.clone(), live);
    Ok(())
}

async fn action(pipe: &str, command: Action, state: &mut Snapshot, publish: &Publish, live: bool) -> Result<()> {
    if let Some(patch) = prefs(&command) {
        let result = transport::pipe_json(pipe, "PATCH", "/localapi/v0/prefs", Some(patch)).await;
        let ok = result.is_ok();
        publish.action_result(result.map(|_| ()));
        if !ok { return Ok(()); }
    }
    refresh(pipe, state, publish, live).await
}

/// Snapshot mode stays asleep until refresh/action. Opt-in live mode watches only the IPN bus.
pub(crate) async fn run(pipe: &str, live: bool, publish: &Publish, commands: &mut mpsc::Receiver<Action>) -> Result<()> {
    let mut state = Snapshot::default();
    refresh(pipe, &mut state, publish, live).await?;
    if !live {
        while let Some(command) = commands.recv().await { action(pipe, command, &mut state, publish, false).await?; }
        return Ok(());
    }
    let mut stream = transport::pipe_request(pipe, "GET", &format!("/localapi/v0/watch-ipn-bus?mask={WATCH_MASK}"), None).await?;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(command) => action(pipe, command, &mut state, publish, true).await?,
                None => return Ok(()),
            },
            event = stream.next() => {
                let event = event?.ok_or_else(|| anyhow::anyhow!("Tailscale IPN bus closed"))?;
                ensure!(event.get("ErrMessage").and_then(Value::as_str).is_none_or(str::is_empty), "Tailscale reported a watch error");
                apply(&mut state, &event);
                if event.get("SelfChange").is_some() { refresh(pipe, &mut state, publish, true).await?; }
                else { publish.set(state.clone(), true); }
            }
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub os: Option<String>,
    pub last_seen: Option<String>,
    pub exit_option: bool,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}
pub fn apply(state: &mut Snapshot, event: &Value) {
    if let Some(index) = event["State"].as_u64() {
        state.state = [
            "NoState",
            "InUseOtherUser",
            "NeedsLogin",
            "NeedsMachineAuth",
            "Stopped",
            "Starting",
            "Running",
        ]
        .get(index as usize)
        .unwrap_or(&"Unknown")
        .to_string();
    }
    if let Some(s) = event["State"].as_str() {
        state.state = s.into();
    }
    if let Some(status) = event.get("InitialStatus") {
        if let Some(s) = status["BackendState"].as_str() {
            state.state = s.into();
        }
        if let Some(addresses) = status["TailscaleIPs"].as_array() {
            state.addresses = addresses
                .iter()
                .filter_map(Value::as_str)
                .take(8)
                .map(str::to_owned)
                .collect();
        }
        state.exit_node_id = status["ExitNodeStatus"]["ID"].as_str().map(str::to_owned);
        state.exit_node = state.exit_node_id.clone();
        if let Some(peers) = status["Peer"].as_object() {
            state.peers_known = true;
            state.peers = peers
                .values()
                .take(128)
                .map(|peer| Peer {
                    id: peer["ID"].as_str().unwrap_or_default().into(),
                    name: peer["DNSName"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| peer["HostName"].as_str())
                        .unwrap_or("Device")
                        .trim_end_matches('.')
                        .to_owned(),
                    online: peer["Online"].as_bool().unwrap_or(false),
                    os: peer["OS"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
                    last_seen: peer["LastSeen"].as_str().filter(|s| !s.starts_with("0001-")).map(str::to_owned),
                    exit_option: peer["ExitNodeOption"].as_bool().unwrap_or(false),
                    rx_bytes: peer["RxBytes"].as_u64(),
                    tx_bytes: peer["TxBytes"].as_u64(),
                })
                .collect();
            state
                .peers
                .sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.cmp(&b.name)));
            state.rx_bytes = state.peers.iter().filter_map(|p| p.rx_bytes).reduce(u64::saturating_add);
            state.tx_bytes = state.peers.iter().filter_map(|p| p.tx_bytes).reduce(u64::saturating_add);
        }
        if let Some(exit) = status["Peer"].as_object().and_then(|peers| {
            peers
                .values()
                .find(|peer| peer["ExitNode"].as_bool() == Some(true))
        }) {
            state.exit_node = exit["HostName"]
                .as_str()
                .map(str::to_owned)
                .or(state.exit_node.take());
        }
    }
    if let Some(prefs) = event.get("Prefs") {
        state.exit_node_id = prefs["ExitNodeID"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        state.exit_node = state.exit_node_id.as_ref().map(|id| state.peers.iter().find(|p| &p.id == id).map_or_else(|| id.clone(), |p| p.name.clone()));
    }
    if let Some(change) = event.get("SelfChange")
        && let Some(addresses) = change["TailscaleIPs"].as_array()
    {
        state.addresses = addresses
            .iter()
            .filter_map(Value::as_str)
            .take(8)
            .map(str::to_owned)
            .collect();
    }
}
