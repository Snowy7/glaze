//! T3 Code as a WebAssembly component (docs/plugins.md): running threads with their subagents,
//! approvals, limits, 30-day usage and a new-thread composer, from the local T3 Code server.
//! `t3.rs`/`t3_client.rs` are the model the built-in integration used (pure decoding);
//! `component.rs` drives the socket, `auth.rs` T3's background key, and `faces.rs` draws.

// The faces and model are driven only by the wasm component; the host build runs their tests.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod faces;
#[cfg(test)]
mod golden;
mod t3;
mod t3_client;

#[cfg(target_arch = "wasm32")]
mod auth;
#[cfg(target_arch = "wasm32")]
mod component;

/// How live the data is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Connecting,
    Live,
    Cached,
    Stale(String),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    T3(t3::Snapshot),
}

/// What the faces draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub availability: Availability,
    pub data: Option<Data>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self { availability: Availability::Connecting, data: None }
    }
}

pub mod launcher {
    /// Percent-encodes a deep-link path segment.
    pub fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    }
}
