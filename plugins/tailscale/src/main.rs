//! Demand-owned Tailscale SDK plugin. Reads the existing service; only explicit actions edit prefs.
mod faces;
mod tailscale;
mod transport;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, sync::{Arc, Mutex}, time::Duration};
use mypaper_plugin_sdk::{Ctx, Event, Face, MsgSender, Plugin, SizeClass, run};
use tokio::sync::{mpsc, oneshot};

const DEFAULT_PIPE: &str = r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    data: Option<tailscale::Snapshot>,
    error: Option<String>,
    action_error: Option<String>,
    live: bool,
}

pub struct Publish {
    state: Arc<Mutex<State>>,
    sender: MsgSender<Msg>,
}
impl Publish {
    fn update(&self, change: impl FnOnce(&mut State)) {
        let mut current = self.state.lock().unwrap();
        let old = current.clone();
        change(&mut current);
        let changed = *current != old;
        drop(current);
        if changed { self.sender.send(Msg::Changed); }
    }
    fn set(&self, data: tailscale::Snapshot, live: bool) {
        self.update(|s| { s.data = Some(data); s.error = None; s.live = live; });
    }
    fn action_result(&self, result: anyhow::Result<()>) {
        self.update(|s| s.action_error = result.err().map(|e| format!("Action failed · {e:#}")));
    }
}

#[derive(Debug)]
pub enum Action { Refresh, Connected(bool), ExitNode(String) }
pub enum Msg { Changed, Ended(String) }

struct Worker { commands: mpsc::Sender<Action>, stop: oneshot::Sender<()>, thread: std::thread::JoinHandle<()> }
impl Worker {
    fn stop(self) { let _ = self.stop.send(()); let _ = self.thread.join(); }
}

#[derive(Default)]
struct TailscalePlugin {
    faces: BTreeMap<String, (String, SizeClass)>,
    state: Arc<Mutex<State>>,
    worker: Option<Worker>,
    suspended: bool,
    pipe: String,
    live: bool,
    last_views: BTreeMap<String, Face>,
}
impl TailscalePlugin {
    fn start(&mut self, ctx: &mut Ctx<Msg>) {
        if self.worker.is_some() || self.suspended || self.faces.is_empty() { return; }
        self.pipe = ctx.settings()["pipe"].as_str().unwrap_or(DEFAULT_PIPE).into();
        self.live = ctx.settings()["liveVerified"].as_bool().unwrap_or(false);
        let (commands, mut receiver) = mpsc::channel(16);
        let (stop, stopped) = oneshot::channel();
        let sender = ctx.sender();
        let publish = Publish { state: self.state.clone(), sender: sender.clone() };
        let pipe = if self.pipe.is_empty() { DEFAULT_PIPE.into() } else { self.pipe.clone() };
        let live = self.live;
        let thread = std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(anyhow::Error::from).and_then(|rt| rt.block_on(async {
                tokio::select! {
                    _ = stopped => Ok(()),
                    result = tailscale::run(&pipe, live, &publish, &mut receiver) => result,
                }
            }));
            if let Err(e) = result { sender.send(Msg::Ended(format!("{e:#}"))); }
        });
        self.worker = Some(Worker { commands, stop, thread });
    }
    fn stop(&mut self) { if let Some(worker) = self.worker.take() { worker.stop(); } }
    fn draw(&mut self, ctx: &mut Ctx<Msg>) {
        let state = self.state.lock().unwrap().clone();
        for (instance, (face, size)) in &self.faces {
            let view = faces::face(face, *size, &state, chrono::Utc::now().timestamp_millis());
            if self.last_views.get(instance) != Some(&view) {
                ctx.view(instance, view.clone());
                self.last_views.insert(instance.clone(), view);
            }
        }
    }
    fn command(&mut self, ctx: &mut Ctx<Msg>, action: Action) {
        self.start(ctx);
        if let Some(worker) = &self.worker && worker.commands.try_send(action).is_err() {
            self.state.lock().unwrap().action_error = Some("Tailscale is busy; refresh and try again".into());
        }
    }
}
impl Drop for TailscalePlugin { fn drop(&mut self) { self.stop(); } }
impl Plugin for TailscalePlugin {
    type Msg = Msg;
    fn on_event(&mut self, ctx: &mut Ctx<Msg>, event: Event<Msg>) {
        match event {
            Event::Open { instance, face, size, .. } => {
                self.last_views.remove(&instance);
                self.faces.insert(instance, (face, size));
                self.start(ctx);
                ctx.every("last-seen", Duration::from_secs(60));
            }
            Event::Resize { instance, size } => { if let Some(f) = self.faces.get_mut(&instance) { f.1 = size; } }
            Event::Close { instance } => {
                self.faces.remove(&instance); self.last_views.remove(&instance);
                if self.faces.is_empty() { self.stop(); ctx.cancel("last-seen"); }
            }
            Event::Suspend => { self.suspended = true; self.stop(); ctx.cancel("last-seen"); }
            Event::Resume => { self.suspended = false; self.start(ctx); if !self.faces.is_empty() { ctx.every("last-seen", Duration::from_secs(60)); } }
            Event::Action { instance, action, value } if self.faces.contains_key(&instance) => {
                let state = self.state.lock().unwrap().clone();
                match action.as_str() {
                    "refresh" => { self.state.lock().unwrap().action_error = None; self.command(ctx, Action::Refresh); }
                    "connect" => self.command(ctx, Action::Connected(true)),
                    "disconnect" => self.command(ctx, Action::Connected(false)),
                    "exit:none" => self.command(ctx, Action::ExitNode(String::new())),
                    "exit" => if let Some(id) = value && (id.is_empty() || state.data.as_ref().is_some_and(|s| s.peers.iter().any(|p| p.id == id && p.exit_option && p.online))) {
                        self.command(ctx, Action::ExitNode(id));
                    },
                    _ => if let Some(id) = action.strip_prefix("exit:") && state.data.as_ref().is_some_and(|s| s.peers.iter().any(|p| p.id == id && p.exit_option && p.online)) {
                        self.command(ctx, Action::ExitNode(id.into()));
                    },
                }
            }
            Event::Msg(Msg::Ended(why)) => { self.stop(); self.state.lock().unwrap().error = Some(why); }
            Event::Msg(Msg::Changed) | Event::Timer(_) => {}
            _ => return,
        }
        self.draw(ctx);
    }
}
fn main() { run(TailscalePlugin::default()); }
