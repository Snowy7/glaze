//! Pure LocalAPI status decoding and masked preference patches. No OS access in the guest.
use crate::Action;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
