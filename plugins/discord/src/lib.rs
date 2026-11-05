//! Discord is a WASI component. Only the guest module imports host capabilities; the model,
//! framing and faces also run natively in focused tests and the shell's golden renderer.
pub mod faces;
#[cfg(target_arch = "wasm32")]
mod guest;
pub mod model;
