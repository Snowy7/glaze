//! OBS's websocket provider in its own process; credentials are supplied by the host broker.
mod faces;
#[cfg(test)]
mod golden;
mod obs;
mod transport;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, sync::{Arc, Mutex}, time::Duration};
use mypaper_plugin_sdk::{Ctx, Event, Plugin, SizeClass, run};
use tokio::sync::{mpsc, oneshot};

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
    fn default() -> Self {
        Self { availability: Availability::Connecting, data: None, observed_at_ms: 0 }
    }
}
pub enum Action { Obs(obs::Action) }
pub enum Msg { Changed, Ended(u64, String), Notice(String) }

pub struct Publish {
    pub snapshot: Arc<Mutex<Snapshot>>,
    pub sender: mypaper_plugin_sdk::MsgSender<Msg>,
}
impl Publish {
    pub fn delivered(&self) {}
    pub fn set(&self, availability: Availability, data: Option<Data>) {
        let mut current = self.snapshot.lock().unwrap();
        if current.availability == availability && current.data == data { return; }
        current.availability = availability;
        current.data = data;
        current.observed_at_ms = epoch_ms();
        drop(current);
        self.sender.send(Msg::Changed);
    }
    pub fn action_result(&self, result: anyhow::Result<()>) {
        if let Err(error) = result { self.sender.send(Msg::Notice(format!("{error:#}"))); }
    }
}
pub mod config {
    pub struct Obs { pub endpoint: String }
    impl Default for Obs {
        fn default() -> Self { Self { endpoint: "ws://127.0.0.1:4455".into() } }
    }
}
pub mod credentials {
    use std::sync::{Mutex, OnceLock};
    pub struct Secret(String);
    impl Secret { pub fn text(&self) -> anyhow::Result<&str> { Ok(&self.0) } }
    fn store() -> &'static Mutex<Option<String>> {
        static PASSWORD: OnceLock<Mutex<Option<String>>> = OnceLock::new();
        PASSWORD.get_or_init(|| Mutex::new(None))
    }
    pub fn init(value: Option<String>) { *store().lock().unwrap() = value; }
    pub fn read(_key: &str) -> anyhow::Result<Secret> {
        store().lock().unwrap().clone().map(Secret).ok_or_else(|| anyhow::anyhow!("Save the OBS password in plugin settings"))
    }
}
fn epoch_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

#[derive(Default)]
struct ObsPlugin {
    faces: BTreeMap<String, SizeClass>,
    snapshot: Arc<Mutex<Snapshot>>,
    commands: Option<mpsc::Sender<Action>>,
    stop: Option<oneshot::Sender<()>>,
    generation: u64,
    suspended: bool,
    notice: Option<String>,
}
impl ObsPlugin {
    fn start(&mut self, ctx: &mut Ctx<Msg>) {
        if self.stop.is_some() || self.suspended || self.faces.is_empty() { return; }
        credentials::init(ctx.secret("obs").ok().flatten());
        let config = config::Obs {
            endpoint: ctx.settings()["endpoint"].as_str().map(str::to_owned).unwrap_or_else(|| config::Obs::default().endpoint),
        };
        let (tx, mut rx) = mpsc::channel(32);
        let (stop, stopped) = oneshot::channel();
        self.commands = Some(tx);
        self.stop = Some(stop);
        self.generation += 1;
        let generation = self.generation;
        let publish = Publish { snapshot: self.snapshot.clone(), sender: ctx.sender() };
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            let result = match runtime {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = obs::run(&config, &publish, &mut rx) => result,
                        _ = stopped => return,
                    }.unwrap_or_else(|e| { publish.sender.send(Msg::Ended(generation, format!("{e:#}"))); });
                }),
                Err(error) => { publish.sender.send(Msg::Ended(generation, error.to_string())); },
            };
            result
        });
    }
    fn stop(&mut self, ctx: &mut Ctx<Msg>) {
        self.generation += 1;
        self.commands = None;
        self.stop.take();
        ctx.cancel("clock");
        ctx.cancel("retry");
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.observed_at_ms = epoch_ms();
        snapshot.availability = Availability::Stale("Suspended".into());
    }
    fn draw_all(&self, ctx: &mut Ctx<Msg>) {
        let snapshot = self.snapshot.lock().unwrap().clone();
        for (instance, size) in &self.faces {
            let mut face = faces::face(*size, &snapshot, epoch_ms());
            if let Some(note) = &self.notice { face.footer = Some(note.clone()); }
            ctx.view(instance, face);
        }
    }
    fn clocks(&self, ctx: &mut Ctx<Msg>) {
        let snapshot = self.snapshot.lock().unwrap();
        let advancing = snapshot.availability == Availability::Live && matches!(&snapshot.data,
            Some(Data::Obs(state)) if state.record_clock.advancing || state.stream_clock.advancing);
        if advancing && !self.suspended && !self.faces.is_empty() { ctx.every("clock", Duration::from_secs(1)); }
        else { ctx.cancel("clock"); }
    }
    fn action(&mut self, ctx: &mut Ctx<Msg>, action: &str) {
        if matches!(action, "refresh" | "retry") {
            self.stop(ctx);
            self.start(ctx);
            return;
        }
        let snapshot = self.snapshot.lock().unwrap();
        if snapshot.availability != Availability::Live { return; }
        let command = faces::action(action, snapshot.data.as_ref());
        drop(snapshot);
        if let Some(command) = command && let Some(tx) = &self.commands && tx.try_send(Action::Obs(command)).is_err() {
            self.notice = Some("OBS is busy; try again".into());
        }
    }
}
impl Plugin for ObsPlugin {
    type Msg = Msg;
    fn on_event(&mut self, ctx: &mut Ctx<Msg>, event: Event<Msg>) {
        match event {
            Event::Open { instance, size, .. } => { self.faces.insert(instance, size); self.start(ctx); self.clocks(ctx); }
            Event::Resize { instance, size } => { if let Some(slot) = self.faces.get_mut(&instance) { *slot = size; } }
            Event::Close { instance } => { self.faces.remove(&instance); if self.faces.is_empty() { self.stop(ctx); } }
            Event::Suspend => { self.suspended = true; self.stop(ctx); }
            Event::Resume => { self.suspended = false; self.start(ctx); }
            Event::Action { instance, action, .. } if self.faces.contains_key(&instance) => self.action(ctx, &action),
            Event::Msg(Msg::Changed) => { self.notice = None; self.clocks(ctx); }
            Event::Msg(Msg::Notice(note)) => self.notice = Some(note),
            Event::Msg(Msg::Ended(generation, why)) if generation == self.generation => {
                self.commands = None;
                self.stop = None;
                let mut snapshot = self.snapshot.lock().unwrap();
                snapshot.observed_at_ms = epoch_ms();
                snapshot.availability = if snapshot.data.is_some() { Availability::Stale(why) } else { Availability::Unavailable(why) };
                drop(snapshot);
                self.clocks(ctx);
                ctx.after("retry", Duration::from_secs(30));
            }
            Event::Timer("retry") => self.start(ctx),
            Event::Timer("clock") => {},
            _ => return,
        }
        self.draw_all(ctx);
    }
}
fn main() { run(ObsPlugin::default()); }
