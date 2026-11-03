//! mypaper RSS plugin: JSON-RPC 2.0 over stdio (docs/widgets.md, "Plugins").
//!
//! One method, `feeds({ urls, limit? })`: loads up to 12 RSS 2.0 / RSS 1.0 / Atom feeds (in
//! parallel), merges their entries newest-first and returns
//! `{ items: [{ title, link, source, published?, summary? }], errors: [{ url, message }] }`.
//! A feed that fails lands in `errors` and doesn't fail the call. Each feed is cached for
//! 15 minutes; if a refresh fails, the stale copy is served. There is no background work, so
//! `suspend` and `resume` need no handling.

mod client;
mod date;
mod feed;
mod rpc;
mod service;
mod text;

use std::io::{self, BufRead};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use client::Http;
use rpc::{Incoming, Out, error_response, parse_line, response};
use service::{Service, SystemClock};

/// The protocol version we speak (the `initialize` handshake).
const PROTOCOL: u64 = 1;
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

type Stdout = Out<io::Stdout>;

struct Plugin {
    service: Service<Http, SystemClock>,
    out: Stdout,
}

fn main() {
    let plugin = Arc::new(Plugin {
        service: Service::new(Http::new(), SystemClock, CACHE_TTL),
        out: Out::new(io::stdout()),
    });

    // Until the host closes our stdin.
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if !handle_line(&plugin, &line) {
            break;
        }
    }
}

/// Returns false after `shutdown`.
fn handle_line(plugin: &Arc<Plugin>, line: &str) -> bool {
    match parse_line(line) {
        Err((id, err)) => send(plugin, &error_response(&id, &err)),
        Ok(Incoming::Notification { method }) => match method.as_str() {
            // Nothing runs in the background, so there is nothing to stop or restart.
            "suspend" | "resume" => {}
            other => eprintln!("rss: ignoring notification \"{other}\""),
        },
        Ok(Incoming::Request { id, method, params }) => match method.as_str() {
            "initialize" => send(plugin, &response(&id, json!({ "protocol": PROTOCOL }))),
            "shutdown" => {
                send(plugin, &response(&id, Value::Null));
                return false;
            }
            // Calls make network requests, so they must not block the loop (shutdown still has
            // to be heard).
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
        eprintln!("rss: cannot write to the host: {e}");
        std::process::exit(1);
    }
}
