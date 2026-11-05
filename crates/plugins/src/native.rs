//! The `native` capability: calls into a plugin's own native modules, which run in mypaper's
//! separate native host process (`crates/native-host`, docs/plugins.md › "Native modules").
//!
//! Stubbed transport: the calls will go through `mypaper_native_host::Supervisor::call` on a
//! blocking worker lane, with replies delivered as `event::native`. Until that crate's API
//! settles, every call is refused as unsupported. The permission check (`native-code`) already
//! happens in `runtime.rs`.

/// Starts a call and returns its request id.
pub fn call(plugin: &str, module: &str, method: &str, bytes: &[u8]) -> Result<u64, String> {
    log::info!("plugin {plugin}: native {module}.{method} ({} bytes) refused: the native host isn't linked yet", bytes.len());
    Err("native modules aren't linked into this host yet".into())
}
