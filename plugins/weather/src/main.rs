//! Glaze weather plugin: JSON-RPC 2.0 over stdio (docs/widgets.md, "Plugins").
//!
//! Methods `current` and `forecast` default to automatic location; `locate` reports the device's
//! location and `search` disambiguates towns. Weather is cached for 10 minutes; stale entries are
//! refreshed in the background and announced with an `updated` event.

use glaze_plugin_weather::{client, location, rpc, service};

use std::io::{self, BufRead};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use client::OpenMeteo;
use rpc::{Incoming, Out, RpcError, error_response, event, parse_line, response};
use service::Service;

/// The protocol version we speak (the `initialize` handshake).
const PROTOCOL: u64 = 1;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// How often the idle loop checks for stale entries.
const REFRESH_CHECK: Duration = Duration::from_secs(30);

type Stdout = Out<io::Stdout>;

struct Plugin {
    service: Service<OpenMeteo>,
    out: Stdout,
    suspended: AtomicBool,
}

fn main() {
    // Keep the MTA alive so worker completion never tears down the process's WinRT runtime
    // between RPC calls that reuse cached activation factories.
    #[cfg(windows)]
    let _runtime = location::native::Apartment::new().ok();
    let plugin = Arc::new(Plugin {
        service: Service::new(OpenMeteo::new(), CACHE_TTL),
        out: Out::new(io::stdout()),
        suspended: AtomicBool::new(false),
    });

    // Reading stdin blocks, so it gets its own thread; the main loop also needs to wake up for
    // the background refresh.
    let (lines, inbox) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        match inbox.recv_timeout(REFRESH_CHECK) {
            Ok(line) if line.trim().is_empty() => {}
            Ok(line) => {
                if !handle_line(&plugin, &line) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => refresh_stale(&plugin),
            // The host closed our stdin: it's gone.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Returns false after `shutdown`.
fn handle_line(plugin: &Arc<Plugin>, line: &str) -> bool {
    match parse_line(line) {
        Err((id, err)) => send(plugin, &error_response(&id, &err)),
        Ok(Incoming::Notification { method }) => match method.as_str() {
            "suspend" => plugin.suspended.store(true, Ordering::Relaxed),
            "resume" => {
                plugin.suspended.store(false, Ordering::Relaxed);
                refresh_stale(plugin);
            }
            other => eprintln!("weather: ignoring notification \"{other}\""),
        },
        Ok(Incoming::Request { id, method, params }) => match method.as_str() {
            "initialize" => match plugin.service.initialize(&params) {
                Ok(()) => send(plugin, &response(&id, json!({ "protocol": PROTOCOL }))),
                Err(error) => send(plugin, &error_response(&id, &error)),
            },
            "shutdown" => {
                send(plugin, &response(&id, Value::Null));
                return false;
            }
            // Calls make network requests, so they must not block the loop (suspend and
            // shutdown still have to be heard).
            _ => {
                let plugin = Arc::clone(plugin);
                thread::spawn(move || {
                    let reply = match plugin.service.call(&method, &params) {
                        Ok(result) => response(&id, result),
                        Err(err) => error_response(&id, &err),
                    };
                    send(&plugin, &reply);
                });
            }
        },
    }
    true
}

fn send(plugin: &Plugin, message: &Value) {
    if let Err(e) = plugin.out.send(message) {
        // stdout is gone: the host died. Nothing left to do.
        eprintln!("weather: cannot write to the host: {e}");
        std::process::exit(1);
    }
}

/// Re-fetches entries past the cache time and tells widgets when something changed.
fn refresh_stale(plugin: &Arc<Plugin>) {
    if plugin.suspended.load(Ordering::Relaxed) {
        return;
    }
    for (place, units) in plugin.service.stale() {
        let plugin = Arc::clone(plugin);
        thread::spawn(move || match plugin.service.refresh(&place, units) {
            Ok(true) => send(
                &plugin,
                &event(
                    "updated",
                    json!({ "location": place, "units": units.as_str() }),
                ),
            ),
            Ok(false) => {}
            Err(RpcError { message, .. }) => eprintln!("weather: refresh failed: {message}"),
        });
    }
}
