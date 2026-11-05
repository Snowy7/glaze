//! T3 Code as an SDK plugin (docs/plugins.md): the reference port. The provider (`t3*.rs`,
//! `transport.rs`) is the same code the shell ran in-process; this file supplies the few
//! crate-root items it used, routes its credentials through the host's secrets broker, and
//! turns each snapshot into declarative faces (`faces.rs`).

mod faces;
#[cfg(test)]
mod golden;
mod t3;
mod t3_auth;
mod t3_client;
mod transport;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mypaper_plugin_sdk::{Ctx, Event, MsgSender, Plugin, SizeClass, run};
use serde_json::Value;

pub use t3::Snapshot as T3Snapshot;

/// How live the data is (the shell's provider vocabulary, kept for the ported code).
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    T3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub availability: Availability,
    pub data: Option<Data>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            availability: Availability::Connecting,
            data: None,
        }
    }
}

#[derive(Default)]
pub struct Counters;

/// Where the provider publishes: straight into the plugin's loop.
pub struct Publish {
    pub provider: Provider,
    pub snapshot: Arc<Mutex<Snapshot>>,
    pub wake: Arc<dyn Fn(Provider) + Send + Sync>,
    pub counters: Arc<Counters>,
}

impl Publish {
    pub fn delivered(&self) {}

    pub fn set(&self, availability: Availability, data: Option<Data>) {
        let mut current = self.snapshot.lock().unwrap();
        if current.availability == availability && current.data == data {
            return;
        }
        current.availability = availability;
        current.data = data;
        current.revision += 1;
        drop(current);
        (self.wake)(self.provider);
    }
}

pub mod config {
    use std::collections::BTreeMap;

    pub struct T3 {
        pub enabled: bool,
        pub endpoint: String,
        pub protocol_version: u32,
        pub live_verified: bool,
        pub thread_links: BTreeMap<String, String>,
    }

    impl Default for T3 {
        fn default() -> Self {
            Self {
                enabled: true,
                endpoint: crate::t3_client::DEFAULT_ENDPOINT.into(),
                protocol_version: 2,
                live_verified: true,
                thread_links: BTreeMap::new(),
            }
        }
    }
}

pub mod launcher {
    pub fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
}

/// The ported code's Credential Manager calls, served from what the host's secrets broker gave
/// the loop at start; writes go back to the loop, which stores them through the broker.
pub mod credentials {
    use anyhow::{Result, anyhow};
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};

    pub struct Secret(String);
    impl Secret {
        pub fn text(&self) -> Result<&str> {
            Ok(&self.0)
        }
    }

    type Store = Mutex<(BTreeMap<String, String>, Option<Box<dyn Fn(String, String) + Send>>)>;
    fn store() -> &'static Store {
        static STORE: OnceLock<Store> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new((BTreeMap::new(), None)))
    }

    /// Seeds the cache from the broker and sets where writes go.
    pub fn init(known: BTreeMap<String, String>, write: Box<dyn Fn(String, String) + Send>) {
        *store().lock().unwrap() = (known, Some(write));
    }

    pub fn read(key: &str) -> Result<Secret> {
        let store = store().lock().unwrap();
        store.0.get(key).cloned().map(Secret).ok_or_else(|| anyhow!("no {key} credential"))
    }

    pub fn write(key: &str, value: &str) -> Result<()> {
        let mut store = store().lock().unwrap();
        store.0.insert(key.into(), value.into());
        if let Some(write) = &store.1 {
            write(key.into(), value.into());
        }
        Ok(())
    }
}

/// Secrets the ported code reads: the bearer and its session record.
const SECRETS: [&str; 2] = ["t3", "t3-session"];

pub enum Msg {
    /// The provider published a new snapshot.
    Changed,
    /// The provider stored a secret; persist it through the broker.
    Secret(String, String),
    /// The provider's run ended (error text), to retry later.
    Ended(String),
}

#[derive(Default)]
struct T3Plugin {
    /// Placed faces: instance → (face, size).
    faces: BTreeMap<String, (String, SizeClass)>,
    snapshot: Arc<Mutex<Snapshot>>,
    running: bool,
}

impl T3Plugin {
    fn start(&mut self, ctx: &mut Ctx<Msg>) {
        if self.running {
            return;
        }
        self.running = true;
        let known = SECRETS
            .iter()
            .filter_map(|key| Some((key.to_string(), ctx.secret(key).ok()??)))
            .collect();
        let sender: MsgSender<Msg> = ctx.sender();
        let writes = sender.clone();
        credentials::init(known, Box::new(move |k, v| { writes.send(Msg::Secret(k, v)); }));
        let snapshot = self.snapshot.clone();
        std::thread::spawn(move || {
            let woke = sender.clone();
            let publish = Publish {
                provider: Provider::T3,
                snapshot,
                wake: Arc::new(move |_| { woke.send(Msg::Changed); }),
                counters: Arc::new(Counters),
            };
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            let ended = match runtime {
                Ok(runtime) => runtime.block_on(t3::run(&config::T3::default(), &publish)),
                Err(e) => Err(e.into()),
            };
            let why = ended.err().map_or_else(|| "T3 disconnected".into(), |e| format!("{e:#}"));
            sender.send(Msg::Ended(why));
        });
    }

    fn draw(&self, ctx: &mut Ctx<Msg>, instance: &str) {
        let Some((face, size)) = self.faces.get(instance) else { return };
        let snapshot = self.snapshot.lock().unwrap().clone();
        let now_ms = chrono::Utc::now().timestamp_millis();
        ctx.view(instance, faces::face(face, *size, &snapshot, now_ms));
    }

    fn draw_all(&self, ctx: &mut Ctx<Msg>) {
        for instance in self.faces.keys() {
            self.draw(ctx, instance);
        }
    }
}

impl Plugin for T3Plugin {
    type Msg = Msg;

    fn on_event(&mut self, ctx: &mut Ctx<Msg>, event: Event<Msg>) {
        match event {
            Event::Open { instance, face, size, .. } => {
                self.faces.insert(instance.clone(), (face, size));
                self.start(ctx);
                self.draw(ctx, &instance);
                // Countdowns in Limits move without provider events: redraw each minute.
                ctx.every("minute", std::time::Duration::from_secs(60));
            }
            Event::Resize { instance, size } => {
                if let Some(slot) = self.faces.get_mut(&instance) {
                    slot.1 = size;
                }
                self.draw(ctx, &instance);
            }
            Event::Close { instance } => {
                self.faces.remove(&instance);
                if self.faces.is_empty() {
                    ctx.cancel("minute");
                }
            }
            Event::Action { instance, action, .. } => {
                if action == "retry" && !self.running {
                    self.start(ctx);
                }
                self.draw(ctx, &instance);
            }
            Event::Timer("minute") | Event::Msg(Msg::Changed) => self.draw_all(ctx),
            Event::Timer("retry") => self.start(ctx),
            Event::Msg(Msg::Secret(key, value)) => {
                if let Err(e) = ctx.set_secret(&key, Some(&value)) {
                    eprintln!("storing {key}: {e}");
                }
            }
            Event::Msg(Msg::Ended(why)) => {
                self.running = false;
                let mut snapshot = self.snapshot.lock().unwrap();
                snapshot.availability = match snapshot.data {
                    Some(_) => Availability::Stale(why),
                    None => Availability::Unavailable(why),
                };
                drop(snapshot);
                self.draw_all(ctx);
                // Event-driven retry: one timer, not a poll of the UI.
                ctx.after("retry", std::time::Duration::from_secs(30));
            }
            _ => {}
        }
    }

    fn on_call(&mut self, _ctx: &mut Ctx<Msg>, method: &str, _params: Value) -> Result<Value, String> {
        Err(format!("unknown method \"{method}\""))
    }
}

fn main() {
    run(T3Plugin::default());
}
