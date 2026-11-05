//! The helper's event loop state (docs/plugins.md, "Lifecycle"): loads plugins on demand,
//! delivers events, applies their effects (diffed views, styles, coalesced timers, async
//! fetches), restarts a trapping plugin from its saved state and stops it after three strikes,
//! and saves and unloads plugins with nothing visible.
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_plugin_api::wire::{self, HelperMsg, HostMsg, Status};
use mypaper_plugin_api::{SizeClass, diff};
use mypaper_plugin_sdk::view::Face;

use crate::runtime::exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen};
use crate::runtime::mypaper::plugin::http::{Reply, Response};
use crate::runtime::mypaper::plugin::types::{Error as WitError, SizeClass as WitSize};
use crate::runtime::{Effect, Instance, Manifest, Runtime};

/// Nothing visible this long → saved and unloaded.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(30);
/// Timers are rounded up to this grid so one wake serves every timer due together.
pub const TIMER_GRID: Duration = Duration::from_millis(250);
const STRIKES: usize = 3;
const STRIKE_WINDOW: Duration = Duration::from_secs(300);

/// What reaches the loop: shell frames and I/O completions from worker threads.
pub enum Input {
    Host(HostMsg),
    Http { plugin: String, id: u64, result: Result<crate::caps::Fetched, String> },
    Eof,
}

struct Placed {
    face: String,
    size: SizeClass,
    settings: String,
}

struct Loaded {
    manifest: Manifest,
    dir: PathBuf,
    instance: Option<Instance>,
    faces: BTreeMap<String, Placed>,
    last: HashMap<String, Face>,
    strikes: Vec<Instant>,
    /// Restart after a trap no sooner than this (the throttle).
    retry_at: Option<Instant>,
    stopped: Option<String>,
    idle_since: Option<Instant>,
}

pub struct Host<W: Write> {
    pub rt: Runtime,
    plugins: HashMap<String, Loaded>,
    /// instance → plugin
    owners: HashMap<String, String>,
    /// `(due, seq)` → `(plugin, timer id, period)`
    timers: BTreeMap<(Instant, u64), (String, String, Option<Duration>)>,
    seq: u64,
    epoch: Instant,
    held: bool,
    out: W,
    tx: Sender<Input>,
}

fn size(size: SizeClass) -> WitSize {
    match size {
        SizeClass::Small => WitSize::Small,
        SizeClass::Medium => WitSize::Medium,
        SizeClass::Large => WitSize::Large,
    }
}

impl<W: Write> Host<W> {
    pub fn new(rt: Runtime, out: W, tx: Sender<Input>) -> Self {
        Self { rt, plugins: HashMap::new(), owners: HashMap::new(), timers: BTreeMap::new(), seq: 0, epoch: Instant::now(), held: false, out, tx }
    }

    pub fn out(&mut self) -> &mut W {
        &mut self.out
    }

    fn send(&mut self, message: HelperMsg) {
        if let Err(e) = wire::write(&mut self.out, &message) {
            log::warn!("writing to the shell: {e}");
        }
    }

    fn status(&mut self, plugin: &str, status: Status) {
        self.send(HelperMsg::Status { plugin: plugin.into(), status });
    }

    /// Handles one shell message; `false` means shut down.
    pub fn on_host(&mut self, message: HostMsg, now: Instant) -> bool {
        match message {
            HostMsg::Open { plugin, dir, instance, face, size, settings } => {
                if !self.plugins.contains_key(&plugin) {
                    let dir = PathBuf::from(dir);
                    match Manifest::load(&dir) {
                        Ok(manifest) if manifest.id == plugin => {
                            let loaded = Loaded { manifest, dir, instance: None, faces: BTreeMap::new(), last: HashMap::new(), strikes: Vec::new(), retry_at: None, stopped: None, idle_since: None };
                            self.plugins.insert(plugin.clone(), loaded);
                        }
                        Ok(_) => return true,
                        Err(e) => {
                            log::warn!("plugin {plugin} refused: {e:#}");
                            self.status(&plugin, Status::Stopped(format!("{e:#}")));
                            return true;
                        }
                    }
                }
                let entry = self.plugins.get_mut(&plugin).expect("inserted above");
                if !entry.manifest.contributes(&face) {
                    log::warn!("plugin {plugin} has no face {face:?}");
                    return true;
                }
                entry.idle_since = None;
                let placed = Placed { face: face.clone(), size, settings: settings.clone() };
                let reopened = entry.faces.insert(instance.clone(), placed).is_some();
                self.owners.insert(instance.clone(), plugin.clone());
                if self.ensure_loaded(&plugin, now) == Some(false) {
                    let open = FaceOpen { instance, face, size: self::size(size), settings };
                    self.deliver(&plugin, if reopened { Event::Resize(open) } else { Event::Open(open) }, now);
                }
            }
            HostMsg::Close { instance } => {
                if let Some(plugin) = self.owners.remove(&instance) {
                    if let Some(entry) = self.plugins.get_mut(&plugin) {
                        entry.faces.remove(&instance);
                        entry.last.remove(&instance);
                    }
                    if self.plugins.get(&plugin).is_some_and(|p| p.instance.is_some()) {
                        self.deliver(&plugin, Event::Close(instance), now);
                    }
                }
            }
            HostMsg::Action { instance, action, value } => {
                if let Some(plugin) = self.owners.get(&instance).cloned()
                    && self.ensure_loaded(&plugin, now).is_some()
                {
                    self.deliver(&plugin, Event::Action(Action { instance, action, value }), now);
                }
            }
            HostMsg::Command { plugin, id } => {
                if self.plugins.contains_key(&plugin) && self.ensure_loaded(&plugin, now).is_some() {
                    self.deliver(&plugin, Event::Command(id), now);
                }
            }
            HostMsg::Held(held) => {
                self.held = held;
                let ids: Vec<String> = self.plugins.keys().cloned().collect();
                for id in ids {
                    let loaded = self.plugins[&id].instance.is_some();
                    if held && loaded {
                        self.deliver(&id, Event::Suspend, now);
                    } else if !held && loaded {
                        self.deliver(&id, Event::Resume, now);
                    } else if !held && !self.plugins[&id].faces.is_empty() {
                        self.ensure_loaded(&id, now);
                    }
                }
            }
            HostMsg::Stats => {
                let stats: Vec<HelperMsg> = self
                    .plugins
                    .iter()
                    .filter_map(|(id, p)| p.instance.as_ref().map(|i| HelperMsg::Stats { plugin: id.clone(), memory: i.memory() as u64, cpu_us: i.cpu.as_micros() as u64, events: i.events }))
                    .collect();
                for message in stats {
                    self.send(message);
                }
            }
            HostMsg::Shutdown => {
                let ids: Vec<String> = self.plugins.keys().cloned().collect();
                for id in ids {
                    self.unload(&id);
                }
                return false;
            }
        }
        true
    }

    /// A worker's I/O completion.
    pub fn on_input(&mut self, input: Input, now: Instant) -> bool {
        match input {
            Input::Host(message) => self.on_host(message, now),
            Input::Http { plugin, id, result } => {
                if self.plugins.get(&plugin).is_some_and(|p| p.instance.is_some()) {
                    let outcome = result.map(|f| Response { status: f.status, headers: Vec::new(), body: f.body }).map_err(WitError::Failed);
                    self.deliver(&plugin, Event::Http(Reply { id, outcome }), now);
                }
                true
            }
            Input::Eof => false,
        }
    }

    /// Loads `plugin` if it isn't: `Some(true)` freshly loaded (its faces were re-opened),
    /// `Some(false)` already running, `None` it can't run now (stopped or throttled).
    pub fn ensure_loaded(&mut self, plugin: &str, now: Instant) -> Option<bool> {
        let entry = self.plugins.get_mut(plugin)?;
        if entry.instance.is_some() {
            return Some(false);
        }
        if entry.stopped.is_some() || entry.retry_at.is_some_and(|at| now < at) {
            return None;
        }
        let state_path = self.rt.data_dir(plugin).join("state.bin");
        let state = std::fs::read(&state_path).ok();
        let loaded = self.rt.component(&entry.manifest, &entry.dir).and_then(|(component, _)| self.rt.instantiate(&entry.manifest, &component, state));
        let mut instance = match loaded {
            Ok(instance) => instance,
            Err(e) => {
                log::warn!("plugin {plugin} failed to load: {e:#}");
                entry.stopped = Some(format!("{e:#}"));
                self.status(plugin, Status::Stopped(format!("{e:#}")));
                return None;
            }
        };
        log::info!("plugin {plugin} loaded");
        let effects = instance.take_effects();
        entry.instance = Some(instance);
        entry.retry_at = None;
        let opens: Vec<Event> = entry
            .faces
            .iter()
            .map(|(instance, p)| Event::Open(FaceOpen { instance: instance.clone(), face: p.face.clone(), size: size(p.size), settings: p.settings.clone() }))
            .collect();
        self.status(plugin, Status::Live);
        self.apply(plugin, effects, now);
        for open in opens {
            self.deliver(plugin, open, now);
        }
        Some(true)
    }

    /// Delivers an event to a loaded plugin and applies what it asked for; a trap is a strike.
    pub fn deliver(&mut self, plugin: &str, event: Event, now: Instant) {
        let Some(instance) = self.plugins.get_mut(plugin).and_then(|p| p.instance.as_mut()) else { return };
        match instance.event(&event) {
            Ok(effects) => self.apply(plugin, effects, now),
            Err(trap) => self.strike(plugin, &format!("{trap:#}"), now),
        }
    }

    fn strike(&mut self, plugin: &str, why: &str, now: Instant) {
        let entry = self.plugins.get_mut(plugin).expect("delivering to it");
        entry.instance = None; // a trapped instance can't be re-entered
        entry.strikes.retain(|t| now.duration_since(*t) < STRIKE_WINDOW);
        entry.strikes.push(now);
        let reason = if why.contains("interrupt") { "used too much CPU".to_owned() } else { why.lines().next().unwrap_or(why).to_owned() };
        self.timers.retain(|_, (p, ..)| p != plugin);
        if entry.strikes.len() >= STRIKES {
            log::warn!("plugin {plugin} stopped after {STRIKES} traps: {why}");
            entry.stopped = Some(reason.clone());
            self.status(plugin, Status::Stopped(reason));
        } else {
            let delay = Duration::from_secs(1 << (entry.strikes.len() - 1));
            log::warn!("plugin {plugin} trapped ({why}); restarting in {delay:?}");
            entry.retry_at = Some(now + delay);
            self.status(plugin, Status::Restarting(reason));
        }
    }

    fn apply(&mut self, plugin: &str, effects: Vec<Effect>, now: Instant) {
        for effect in effects {
            match effect {
                Effect::View(instance, face) => {
                    let Some(entry) = self.plugins.get_mut(plugin) else { return };
                    if !entry.faces.contains_key(&instance) {
                        continue; // not one of its placed faces
                    }
                    let changed = match entry.last.get(&instance) {
                        Some(old) if old.footer == face.footer && old.refresh == face.refresh => diff(&old.body, &face.body).len(),
                        _ => 1,
                    };
                    if changed == 0 {
                        continue;
                    }
                    let bytes = serde_json::to_vec(&face).expect("views serialize");
                    entry.last.insert(instance.clone(), face);
                    self.send(HelperMsg::View { instance, face: bytes, changed: changed as u32 });
                }
                Effect::Clear(instance) => {
                    if let Some(entry) = self.plugins.get_mut(plugin) {
                        entry.last.remove(&instance);
                    }
                }
                Effect::Style(surface, patch) => self.send(HelperMsg::Style { plugin: plugin.into(), surface, patch }),
                Effect::After(id, ms) => self.schedule(plugin, id, Duration::from_millis(ms.into()), None, now),
                Effect::Every(id, ms) => {
                    let period = Duration::from_millis(ms.into());
                    self.schedule(plugin, id, period, Some(period), now);
                }
                Effect::Cancel(id) => self.timers.retain(|_, (p, t, _)| !(p == plugin && *t == id)),
                Effect::Fetch { id, method, url, headers, body, timeout_ms } => {
                    let (tx, plugin) = (self.tx.clone(), plugin.to_owned());
                    let spawned = std::thread::Builder::new().name("fetch".into()).stack_size(256 << 10).spawn(move || {
                        let result = crate::caps::fetch(method, &url, &headers, body.as_deref(), timeout_ms);
                        let _ = tx.send(Input::Http { plugin, id, result });
                    });
                    if let Err(e) = spawned {
                        log::warn!("no fetch thread: {e}");
                    }
                }
            }
        }
    }

    /// Replaces the plugin's timer `id`; the deadline is rounded up to the shared grid.
    fn schedule(&mut self, plugin: &str, id: String, after: Duration, period: Option<Duration>, now: Instant) {
        self.timers.retain(|_, (p, t, _)| !(p == plugin && *t == id));
        let grid = TIMER_GRID.as_nanos();
        let since = (now + after).duration_since(self.epoch).as_nanos();
        let due = self.epoch + Duration::from_nanos((since.div_ceil(grid) * grid) as u64);
        self.seq += 1;
        self.timers.insert((due, self.seq), (plugin.into(), id, period));
    }

    /// Saves and drops a plugin's instance (its faces stay placed).
    pub fn unload(&mut self, plugin: &str) {
        let Some(entry) = self.plugins.get_mut(plugin) else { return };
        let Some(mut instance) = entry.instance.take() else { return };
        entry.idle_since = None;
        match instance.save() {
            Ok(state) => {
                let dir = self.rt.data_dir(plugin);
                if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("state.bin"), state)) {
                    log::warn!("plugin {plugin}: saving state: {e}");
                }
            }
            Err(e) => log::warn!("plugin {plugin}: save trapped ({e:#}); its last saved state stays"),
        }
        self.timers.retain(|_, (p, ..)| p != plugin);
        log::info!("plugin {plugin} unloaded");
        self.status(plugin, Status::Unloaded);
    }

    /// Fires due timers, retries throttled plugins and unloads idle ones.
    pub fn tick(&mut self, now: Instant) {
        while let Some((&key, _)) = self.timers.first_key_value().filter(|((due, _), _)| *due <= now) {
            let (plugin, id, period) = self.timers.remove(&key).expect("first key");
            if let Some(period) = period {
                self.schedule(&plugin, id.clone(), period, Some(period), now);
            }
            self.deliver(&plugin, Event::Timer(id), now);
        }
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        for id in ids {
            let entry = &self.plugins[&id];
            if entry.instance.is_none() && entry.retry_at.is_some_and(|at| now >= at) && !entry.faces.is_empty() && !self.held {
                self.ensure_loaded(&id, now);
                continue;
            }
            let visible = !entry.faces.is_empty() && !self.held;
            let entry = self.plugins.get_mut(&id).expect("listed");
            if entry.instance.is_none() || visible {
                entry.idle_since = None;
                continue;
            }
            if now >= *entry.idle_since.get_or_insert(now) + IDLE_UNLOAD {
                self.unload(&id);
            }
        }
    }

    /// When the loop must wake next (`None`: only for input).
    pub fn deadline(&self) -> Option<Instant> {
        let timer = self.timers.keys().next().map(|(due, _)| *due);
        let plugins = self.plugins.values().flat_map(|p| [p.retry_at.filter(|_| p.instance.is_none()), p.idle_since.filter(|_| p.instance.is_some()).map(|t| t + IDLE_UNLOAD)]);
        timer.into_iter().chain(plugins.flatten()).min()
    }

    pub fn loaded(&self) -> usize {
        self.plugins.values().filter(|p| p.instance.is_some()).count()
    }

    pub fn memory(&self, plugin: &str) -> Option<usize> {
        self.plugins.get(plugin)?.instance.as_ref().map(Instance::memory)
    }
}

/// The loop's next wait, given the host's deadline.
pub fn wait(deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    deadline.map(|d| d.saturating_duration_since(now))
}

/// Opens every installed WASM plugin's folders (bundled, then user) for `--compile`.
pub fn plugin_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = roots.iter().filter_map(|r| std::fs::read_dir(r).ok()).flat_map(|d| d.flatten().map(|e| e.path())).filter(|p| p.join("plugin.json").is_file()).collect();
    dirs.sort();
    dirs
}

pub fn compile_all(rt: &Runtime, roots: &[PathBuf]) -> Result<()> {
    for dir in plugin_dirs(roots) {
        let text = std::fs::read_to_string(dir.join("plugin.json")).unwrap_or_default();
        if !text.contains("\"component\"") {
            continue;
        }
        match Manifest::load(&dir).and_then(|m| rt.component(&m, &dir).map(|(_, warm)| (m, warm))) {
            Ok((m, warm)) => println!("{}: {}", m.id, if warm { "cached" } else { "compiled" }),
            Err(e) => println!("{}: refused: {e:#}", dir.display()),
        }
    }
    Ok(())
}
