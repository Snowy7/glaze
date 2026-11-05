//! The connection's truth comes from BackendState, independently of LocalAPI availability.
use crate::{State, tailscale::Peer};
use mypaper_plugin_sdk::{Face, Role, SizeClass, Tone, View};

fn stack(children: impl IntoIterator<Item = View>, gap: f32) -> View {
    View::Stack { children: children.into_iter().collect(), gap: Some(gap) }
}
fn caption(text: impl Into<String>) -> View { View::tinted(text, Role::Caption, Tone::Secondary) }

pub fn bytes(value: Option<u64>) -> String {
    let Some(value) = value else { return "Unavailable".into() };
    match value {
        n if n >= 1 << 30 => format!("{:.1} GiB", n as f64 / (1_u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MiB", n as f64 / (1_u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KiB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

fn seen(peer: &Peer, now: i64) -> String {
    if peer.online { return "Online".into(); }
    let time = peer.last_seen.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
    let Some(time) = time else { return "Last seen unknown".into() };
    let mins = now.saturating_sub(time.timestamp_millis()).max(0) / 60_000;
    match mins { 0 => "Seen just now".into(), 1..60 => format!("Seen {mins}m ago"), 60..1440 => format!("Seen {}h ago", mins / 60), _ => format!("Seen {}d ago", mins / 1440) }
}

fn peer_row(peer: &Peer, exit: Option<&str>, now: i64, traffic: bool) -> View {
    let detail = if traffic {
        format!("↓ {} · ↑ {}", bytes(peer.rx_bytes), bytes(peer.tx_bytes))
    } else { format!("{} · {}", peer.os.as_deref().unwrap_or("OS unknown"), seen(peer, now)) };
    let selected = exit == Some(peer.id.as_str());
    View::ListRow {
        title: peer.name.split('.').next().unwrap_or(&peer.name).into(),
        detail: Some(detail), icon: Some(if traffic { "activity" } else { "globe" }.into()),
        trailing: Some(if selected { "Exit node" } else if peer.exit_option && peer.online { "Use as exit" } else if peer.online { "Online" } else { "Offline" }.into()),
        action: (peer.exit_option && peer.online && !peer.id.is_empty()).then(|| if selected { "exit:none".into() } else { format!("exit:{}", peer.id) }),
    }
}

pub fn face(id: &str, size: SizeClass, state: &State, now: i64) -> Face {
    if id == "status" {
        let connected = state.error.is_none() && state.data.as_ref().is_some_and(|data| data.state == "Running");
        return View::tinted(if connected { "Tailscale · Connected" } else { "Tailscale · Disconnected" }, Role::Caption, if connected { Tone::Positive } else { Tone::Secondary }).face();
    }
    let mut face = match &state.data {
        None => View::empty("globe", if state.error.is_some() { "Tailscale unavailable" } else { "Connecting…" }, state.error.as_deref().unwrap_or("Reading the existing Tailscale service")).face(),
        Some(data) => {
            let connected = data.state == "Running";
            let status = if connected { "Connected" } else { match data.state.as_str() { "Stopped" => "Disconnected", "NeedsLogin" => "Sign in required", "NeedsMachineAuth" => "Approval required", "Starting" => "Connecting…", "" => "Unknown", other => other } };
            // Retained data after a pipe failure must never claim a healthy connection.
            let healthy = connected && state.error.is_none();
            let status = if connected && state.error.is_some() { "Status unavailable" } else { status };
            let headline = View::tinted(status, Role::Title2, if healthy { Tone::Positive } else { Tone::Secondary });
            let count = if data.peers_known { format!("{} of {} peers online", data.peers.iter().filter(|p| p.online).count(), data.peers.len()) } else { "Peers unavailable".into() };
            let control = View::button(if connected { "Disconnect" } else { "Connect" }, if connected { "disconnect" } else { "connect" });
            let exit = format!("Exit node · {}", data.exit_node.as_deref().unwrap_or("None"));
            let mut options = vec![mypaper_plugin_sdk::view::SelectOption { value: String::new(), label: "No exit node".into() }];
            for peer in data.peers.iter().filter(|p| p.exit_option && p.online && !p.id.is_empty()) {
                options.push(mypaper_plugin_sdk::view::SelectOption { value: peer.id.clone(), label: peer.name.split('.').next().unwrap_or(&peer.name).into() });
            }
            let choose_exit = View::Select { label: exit, action: "exit".into(), options, selected: Some(data.exit_node_id.clone().unwrap_or_default()) };
            let address = data.addresses.first().map_or("No address", String::as_str);
            let traffic = format!("↓ {} · ↑ {}", bytes(data.rx_bytes), bytes(data.tx_bytes));
            let peers = |limit, traffic| View::List { children: data.peers.iter().take(limit).map(|p| peer_row(p, data.exit_node_id.as_deref(), now, traffic)).collect(), gap: Some(0.0) };
            let body = match (id, size) {
                (_, SizeClass::Small) => stack([headline, caption(if id == "traffic" { traffic } else { count }), control], 2.0),
                ("traffic", _) => {
                    let stats = View::row([
                        View::Stat { label: "Received".into(), value: bytes(data.rx_bytes), detail: None },
                        View::Stat { label: "Sent".into(), value: bytes(data.tx_bytes), detail: None },
                    ]);
                    stack([stats, caption("Peer totals · snapshot on refresh"), peers(if size == SizeClass::Large { 3 } else { 1 }, true)], 5.0)
                }
                ("peers", _) => {
                    let mut nodes = vec![caption(count), choose_exit];
                    if data.peers.is_empty() { nodes.push(caption(if data.peers_known { "No peers in this tailnet" } else { "Peer status unavailable" })); }
                    else { nodes.push(peers(if size == SizeClass::Large { 4 } else { 2 }, false)); }
                    stack(nodes, 3.0)
                }
                (_, SizeClass::Medium) => stack([
                    View::row([headline, control]),
                    caption(format!("{address} · {count}")),
                    choose_exit,
                    peers(1, false),
                ], 3.0),
                _ => {
                    let mut nodes = vec![View::row([headline, control]), caption(format!("{address} · {count}")), choose_exit, caption(traffic)];
                    nodes.push(peers(3, false));
                    stack(nodes, 3.0)
                }
            };
            body.face()
        }
    };
    face.refresh = true;
    face.footer = Some(state.action_error.as_ref().or(state.error.as_ref()).cloned().unwrap_or_else(|| if state.live { "Live connection · peers and traffic on refresh".into() } else { "Snapshot · refresh for peers and traffic".into() }));
    face
}
