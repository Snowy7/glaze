//! Grok Bot's sandboxed component. Webhooks and drafts run only on explicit actions.
pub mod config;
pub mod discovery;
pub mod faces;
pub mod routines;
#[cfg(target_arch = "wasm32")]
mod component;
