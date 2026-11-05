//! OBS Studio as a WASI component. Only the host owns sockets, timers and credentials.
pub mod faces;
pub mod obs;
#[cfg(target_arch = "wasm32")]
mod component;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability { Connecting, Live, Stale(String), Unavailable(String) }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data { Obs(obs::Snapshot) }
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub data: Option<Data>,
    pub observed_at_ms: i64,
}
impl Default for Snapshot {
    fn default() -> Self { Self { availability: Availability::Connecting, data: None, observed_at_ms: 0 } }
}
