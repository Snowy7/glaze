//! Tailscale component. LocalAPI I/O runs in the shared host through its declared pipe.
mod faces;
mod tailscale;
#[cfg(target_arch = "wasm32")]
mod component;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    data: Option<tailscale::Snapshot>,
    error: Option<String>,
    action_error: Option<String>,
    live: bool,
}

#[derive(Debug)]
pub enum Action {
    Refresh,
    Connected(bool),
    ExitNode(String),
}

/// Reject forged/stale exit-node selections before sending a preferences patch.
fn allowed_exit(state: &State, id: &str) -> bool {
    id.is_empty() || state.data.as_ref().is_some_and(|s|
        s.peers.iter().any(|p| p.id == id && p.exit_option && p.online)
    )
}
