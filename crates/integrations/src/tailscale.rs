//! Replaceable unstable LocalAPI adapter. Never watches engine/peer traffic or starts a VPN node.
use crate::{Availability, Data, Publish, transport};
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
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub online: bool,
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
        state.exit_node = status["ExitNodeStatus"]["ID"].as_str().map(str::to_owned);
        if let Some(peers) = status["Peer"].as_object() {
            state.peers_known = true;
            state.peers = peers
                .values()
                .take(128)
                .map(|peer| Peer {
                    name: peer["DNSName"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| peer["HostName"].as_str())
                        .unwrap_or("Device")
                        .trim_end_matches('.')
                        .to_owned(),
                    online: peer["Online"].as_bool().unwrap_or(false),
                })
                .collect();
            state
                .peers
                .sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.cmp(&b.name)));
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
        state.exit_node = prefs["ExitNodeID"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
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
pub(crate) async fn run(
    config: &crate::config::Tailscale,
    publish: &Publish,
    commands: &mut mpsc::Receiver<crate::Action>,
) -> Result<()> {
    if !config.live_verified {
        let status = transport::pipe_json(&config.pipe, "GET", "/localapi/v0/status", None).await?;
        let mut state = Snapshot::default();
        apply(&mut state, &json!({"InitialStatus":status}));
        ensure!(
            !state.state.is_empty(),
            "Tailscale returned an incompatible status snapshot"
        );
        publish.set(Availability::Cached, Some(Data::Tailscale(state)));
        while let Some(action) = commands.recv().await {
            if let crate::Action::Tailscale { connected } = action {
                let result = transport::pipe_json(
                    &config.pipe,
                    "PATCH",
                    "/localapi/v0/prefs",
                    Some(json!({"WantRunning":connected,"WantRunningSet":true})),
                )
                .await;
                let succeeded = result.is_ok();
                publish.action_result(result.map(|_| ()));
                if succeeded {
                    let status =
                        transport::pipe_json(&config.pipe, "GET", "/localapi/v0/status", None)
                            .await?;
                    let mut state = Snapshot::default();
                    apply(&mut state, &json!({"InitialStatus":status}));
                    publish.set(Availability::Cached, Some(Data::Tailscale(state)));
                }
            }
        }
        return Ok(());
    }
    let mut stream = transport::pipe_request(
        &config.pipe,
        "GET",
        &format!("/localapi/v0/watch-ipn-bus?mask={WATCH_MASK}"),
        None,
    )
    .await?;
    let mut state = Snapshot::default();
    let mut initial = true;
    loop {
        tokio::select! {
            Some(action) = commands.recv() => if let crate::Action::Tailscale { connected } = action {
                let result = transport::pipe_json(&config.pipe, "PATCH", "/localapi/v0/prefs", Some(json!({"WantRunning":connected,"WantRunningSet":true}))).await.map(|_| ());
                publish.action_result(result);
            },
            event = stream.next() => {
                let event = event?.ok_or_else(|| anyhow::anyhow!("Tailscale IPN bus closed"))?;
                publish.delivered();
                ensure!(event.get("ErrMessage").and_then(Value::as_str).is_none_or(str::is_empty), "Tailscale reported a watch error");
                apply(&mut state, &event);
                if event.get("SelfChange").is_some() {
                    let status=transport::pipe_json(&config.pipe,"GET","/localapi/v0/status?peers=false",None).await?;
                    apply(&mut state,&json!({"InitialStatus":status}));
                }
                if !state.state.is_empty() {
                    publish.set(if config.live_verified { Availability::Live } else { Availability::Cached }, Some(Data::Tailscale(state.clone())));
                    if initial && !config.live_verified { return Ok(()); }
                    initial = false;
                }
            }
        }
    }
}
