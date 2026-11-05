//! `mypaper-plugins`: the plugin runtime (docs/plugins.md). One process, one Wasmtime engine,
//! every WebAssembly component plugin. The shell starts it lazily, speaks binary frames on its
//! stdio (`mypaper_plugin_api::wire`), and stops it when no plugin is active.
//!
//! `mypaper-plugins` serves the shell; `--compile <root>...` AOT-compiles every WASM plugin under
//! the roots into the cache (install time); `--bench <plugin dir>...` prints the budget table.
//!
//! Capabilities are `caps.rs` (HTTP, secrets, storage, launch, toasts, system info), `io.rs`
//! (websockets, pipes, HTTP over pipes, each connection on its own threads), `images.rs`
//! (`ui.put-image` shared memory) and `native.rs` (the plugin's own modules in
//! `mypaper-native-host`). System events arrive from the shell as `HostMsg::System`.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod bench;
#[cfg(test)]
#[path = "../../../plugins/tailscale/src/runtime_tests.rs"]
mod tailscale_component_tests;
mod caps;
mod host;
mod images;
mod io;
mod native;
mod runtime;
mod settings_permissions;
#[cfg(test)]
#[path = "../../../plugins/obs/src/runtime_tests.rs"]
mod obs_component_tests;
#[cfg(test)]
mod caps_tests;
#[cfg(test)]
#[path = "../../../plugins/grok/src/runtime_tests.rs"]
mod grok_runtime_tests;
#[cfg(test)]
#[path = "../../../plugins/discord/src/runtime_tests.rs"]
mod discord_runtime_tests;
#[cfg(test)]
mod tests;

use std::io::{BufWriter, Read};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use anyhow::Result;
use mypaper_plugin_api::wire::{self, HostMsg};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cache, data) = runtime::default_dirs();
    match args.first().map(String::as_str) {
        Some("--compile") => host::compile_all(&runtime::Runtime::new(cache, data)?, &args[1..].iter().map(PathBuf::from).collect::<Vec<_>>()),
        Some("--bench") => bench::run(&args[1..].iter().map(PathBuf::from).collect::<Vec<_>>()),
        _ => serve(runtime::Runtime::new(cache, data)?),
    }
}

/// Reads frames from stdin on a thread and runs the loop on this one. Blocks while idle.
fn serve(rt: runtime::Runtime) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let reader = tx.clone();
    std::thread::Builder::new().name("stdin".into()).stack_size(128 << 10).spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            match wire::read::<HostMsg>(&mut input) {
                Ok(Some(message)) => {
                    if reader.send(host::Input::Host(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::warn!("bad frame from the shell: {e}");
                    break;
                }
            }
        }
        let _ = reader.send(host::Input::Eof);
        drain(&mut input);
    })?;
    let out = BufWriter::new(std::io::stdout().lock());
    let mut host = host::Host::new(rt, out, tx);
    loop {
        let now = Instant::now();
        let input = match host::wait(host.deadline(), now) {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(input) => Some(input),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(input) => Some(input),
                Err(_) => break,
            },
        };
        let now = Instant::now();
        if let Some(input) = input
            && !host.on_input(input, now)
        {
            host.on_host(HostMsg::Shutdown, now);
            break;
        }
        host.tick(Instant::now());
    }
    Ok(())
}

fn drain(input: &mut impl Read) {
    let _ = std::io::copy(input, &mut std::io::sink());
}
