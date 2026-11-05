//! Docker Engine over the host's permission-gated HTTP named-pipe transport.
//! Only visible faces refresh their container list. Metrics are bounded, explicit samples.
mod docker;
mod faces;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
mod component;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Availability {
    #[default]
    Connecting,
    Live,
    Stale(String),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data { Docker(docker::Snapshot) }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub availability: Availability,
    pub data: Option<Data>,
    pub action_error: Option<String>,
}

/// Encode Engine-provided ids as path segments.
fn encode(value: &str) -> String {
    value.bytes().map(|b| {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() }
        else { format!("%{b:02X}") }
    }).collect()
}

/// Only ids in the current live snapshot can be acted on, once at a time.
fn action_path(snapshot: &Snapshot, busy: bool, action: &str) -> Option<String> {
    if busy || snapshot.availability != Availability::Live { return None; }
    let (verb @ ("start" | "stop"), id) = action.split_once(':')? else { return None; };
    let Some(Data::Docker(state)) = &snapshot.data else { return None; };
    let container = state.containers.iter().find(|c| c.id == id)?;
    if (verb == "stop") != (container.state == "running") { return None; }
    Some(format!("/containers/{}/{verb}", encode(id)))
}
