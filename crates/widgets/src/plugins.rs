//! The plugin manager (docs/widgets.md, "Plugins"): starts a plugin on first
//! use inside a kill-on-close Job object, speaks JSON-RPC over its stdio
//! (framing in `rpc.rs`), routes replies and events, suspends plugins while
//! the host is held, and stops them after 30 s without users.
//!
//! Each plugin gets three threads: stdout lines go to the main loop as
//! [`Msg::Plugin`], stderr lines go to the log, and a writer drains a channel
//! into stdin so the main thread never blocks on a full pipe.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::PCWSTR;

use crate::bridge::{ErrorCode, Out, Target, WidgetError};
use crate::manifest::PluginManifest;
use crate::rpc::{self, Incoming, Pending, RpcError};
use crate::wake::{Msg, Waker};
use mypaper_plugin_api::wire;
use mypaper_plugin_sdk::protocol::{self as sdk, FromPlugin, HostMessage, HostRequest};
use mypaper_plugin_sdk::{Face, SizeClass};

const INIT_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_STOP: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// A plugin that dies while widgets listen to it is restarted after this,
/// doubling per crash up to a minute.
const RESTART_DELAY: Duration = Duration::from_secs(2);

fn unavailable(message: impl Into<String>) -> WidgetError {
    WidgetError::new(ErrorCode::PluginUnavailable, message)
}

/// How a plugin's JSON-RPC error reaches the widget: the name in
/// `error.data.code` if it's one of ours, else by the JSON-RPC code.
fn widget_error(error: RpcError) -> WidgetError {
    let named = error.name.as_deref().and_then(ErrorCode::from_name);
    let code = named.unwrap_or(match error.code {
        -32601 => ErrorCode::NotFound,
        -32602 => ErrorCode::InvalidParams,
        _ => ErrorCode::Internal,
    });
    WidgetError::new(code, error.message)
}

/// What to run for a plugin's `command`: the file in its folder (`.exe`
/// optional), else the bare name for a `PATH` lookup.
fn program(dir: &Path, command: &str) -> std::path::PathBuf {
    let local = dir.join(command);
    let exe = dir.join(format!("{command}.exe"));
    if local.is_file() {
        local
    } else if exe.is_file() {
        exe
    } else {
        command.into()
    }
}

enum State {
    Starting { deadline: Instant },
    Ready,
    Stopping { deadline: Instant },
}

struct Process {
    manifest: PluginManifest,
    generation: u64,
    child: Child,
    /// Lines for stdin; dropping it closes stdin.
    stdin: Option<Sender<String>>,
    state: State,
    pending: Pending<Target>,
    /// Calls made before the handshake finished.
    queued: Vec<(Target, String, Option<Value>)>,
    idle_since: Option<Instant>,
}

impl Process {
    fn write(&self, line: String) {
        if let Some(stdin) = &self.stdin {
            let _ = stdin.send(line);
        }
    }

    fn send(&mut self, target: Target, method: &str, params: Option<&Value>, now: Instant) {
        let id = self.pending.insert(target, now + CALL_TIMEOUT);
        self.write(rpc::request(id, method, params));
    }

    /// Asks the plugin to exit; it's killed if it hasn't after the grace period.
    fn stop(&mut self, now: Instant) {
        let id = self.pending.reserve();
        self.write(rpc::request(id, "shutdown", None));
        self.stdin = None;
        self.state = State::Stopping {
            deadline: now + SHUTDOWN_GRACE,
        };
    }

    /// Fails every waiting call.
    fn fail_all(&mut self, why: &str) -> Vec<Out> {
        let queued = self.queued.drain(..).map(|(target, ..)| target);
        queued
            .chain(self.pending.drain())
            .map(|target| Out::Reply(target, Err(unavailable(why))))
            .collect()
    }
}

/// Kills every plugin when the host goes away, however it goes away.
struct Job(HANDLE);

impl Job {
    fn new() -> Result<Self> {
        // SAFETY: creates an unnamed job we own; `info` outlives the call.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(Self(job))
        }
    }

    fn assign(&self, child: &Child) -> Result<()> {
        // SAFETY: both handles are valid; the child's stays open while `child` lives.
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }?;
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle; closing it kills what's still inside.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The plugin's own section of `integrations.json` (`{}` when there is none), sent in `initialize`.
fn plugin_settings(id: &str) -> Value {
    let config = crate::files::user("integrations.json")
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    match config.as_ref().and_then(|config| config.get(id)) {
        Some(section @ Value::Object(_)) => section.clone(),
        _ => json!({}),
    }
}

/// A placed face of an SDK plugin (docs/plugins.md): who draws it and what it last drew.
struct Slot {
    plugin: String,
    face: String,
    size: SizeClass,
    settings: Value,
    view: Option<Face>,
    /// A WebAssembly component's face (runs in the shared helper): its plugin folder.
    component: Option<std::path::PathBuf>,
}

/// The `Msg::Plugin` id of the shared WebAssembly runtime (`mypaper-plugins`); never a plugin id.
pub const HELPER: &str = "\0mypaper-plugins";

/// The one `mypaper-plugins` process that runs every component plugin (docs/plugins.md, "Shell ↔
/// helper protocol"). Frames are decoded on its reader thread into `inbox`; the main loop is
/// woken through `Msg::Plugin { id: HELPER, line: Some("") }` and drains it.
struct Helper {
    child: Child,
    generation: u64,
    frames: Sender<wire::HostMsg>,
    inbox: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<wire::HelperMsg>>>,
    idle_since: Option<Instant>,
    notification_plugins: Vec<String>,
    /// System topics a visible plugin subscribes to ([`wire::HelperMsg::Demand`]).
    demand: Vec<String>,
}

/// The helper frame that opens (or re-sizes) a component face.
fn component_open(instance: &str, slot: &Slot) -> wire::HostMsg {
    let dir = slot.component.as_deref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    wire::HostMsg::Open { plugin: slot.plugin.clone(), dir, instance: instance.into(), face: slot.face.clone(), size: slot.size, settings: slot.settings.to_string() }
}

/// `mypaper-plugins.exe` beside the running exe.
fn helper_exe() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.with_file_name("mypaper-plugins.exe");
    exe.is_file().then_some(exe)
}

/// How a placed face is doing, for the chrome's footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceStatus {
    Starting,
    Live,
    /// It crashed; the last view stays up until it's back.
    Restarting,
    Failed(String),
}

/// Whether `permissions` let a plugin make `request` (docs/plugins.md, "Permissions").
pub fn authorize(permissions: &[String], request: &HostRequest) -> Result<(), String> {
    let wanted = request.permission();
    if mypaper_plugin_sdk::allows(permissions, &wanted) {
        Ok(())
    } else {
        Err(format!("the plugin didn't declare {wanted:?}"))
    }
}

/// Answers a plugin's host request after checking its manifest: secrets live in Credential
/// Manager under `mypaper/integrations/plugin/<id>/<key>`.
fn serve(manifest: &PluginManifest, id: &Value, request: HostRequest) -> String {
    use mypaper_integrations::credentials;
    if let Err(why) = authorize(&manifest.permissions, &request) {
        log::warn!("plugin {}: refused {request:?}: {why}", manifest.id);
        return sdk::error(id, sdk::PERMISSION_DENIED, &why, "PermissionDenied");
    }
    let key = |key: &str| format!("plugin/{}/{key}", manifest.id);
    let outcome = match request {
        HostRequest::GetSecret { key: name } => match credentials::read(&key(&name)) {
            Ok(secret) => Ok(json!({ "value": secret.text().ok() })),
            Err(_) => Ok(json!({ "value": null })),
        },
        HostRequest::SetSecret { key: name, value: Some(value) } => {
            credentials::write(&key(&name), &value).map(|()| Value::Null)
        }
        HostRequest::SetSecret { key: name, value: None } => {
            let _ = credentials::remove(&key(&name));
            Ok(Value::Null)
        }
        HostRequest::Launch { target } => {
            let item = mypaper_integrations::launcher::Recent { title: String::new(), target, arguments: String::new() };
            mypaper_integrations::launcher::open(&item).map(|()| Value::Null)
        }
        HostRequest::Notify { .. } => Err(anyhow::anyhow!("notifications aren't wired in this host yet")),
    };
    match outcome {
        Ok(value) => sdk::result(id, value),
        Err(e) => sdk::error(id, -32603, &format!("{e:#}"), "Internal"),
    }
}

pub struct Plugins {
    studio: crate::plugin_manager::Snapshot,
    preferences: crate::plugin_manager::Preferences,
    /// Legacy integration settings are inherited once, before schema defaults or saved settings.
    legacy_settings: HashMap<String, Value>,
    uninstalling: std::collections::BTreeSet<String>,
    procs: HashMap<String, Process>,
    /// Placed faces of SDK plugins, by instance.
    faces: BTreeMap<String, Slot>,
    /// Instances whose view or status changed since [`Plugins::take_changed`].
    changed: Vec<String>,
    /// Where plugins are looked up; `None` is the installed and bundled folders.
    roots: Option<Vec<std::path::PathBuf>>,
    /// `(instance, sub)` → `(plugin, event)`.
    subs: BTreeMap<(String, String), (String, String)>,
    /// Plugins that died with listeners: when to try again, and the next delay.
    restarts: HashMap<String, (Instant, Duration)>,
    /// Old processes still shutting down while a new one took their id.
    dying: Vec<(Child, Instant)>,
    job: Option<Job>,
    waker: Waker,
    held: bool,
    game_active: bool,
    next_generation: u64,
    helper: Option<Helper>,
    /// When to start the helper again after it died with faces placed, and the next delay.
    helper_retry: Option<(Instant, Duration)>,
    /// Component plugins' statuses, from the helper.
    component_status: HashMap<String, wire::Status>,
    /// Style patches from component plugins, by `(plugin, surface)` (docs/plugins.md, "Styling").
    styles: BTreeMap<(String, String), wire::StylePatch>,
    repeats: RepeatGuard,
}

/// Drops an action identical to the last one (same instance, action and value) that comes
/// within [`RepeatGuard::WINDOW`] of it: a safety net under the shell's press/release pairing,
/// so one click can't approve or send twice whatever path repeats it. Each drop restarts the
/// window, so a chain of repeats goes as one. No view kind repeats an action on purpose yet; one
/// that does (a stepper) should mark its action repeatable and skip this.
#[derive(Default)]
struct RepeatGuard(Option<(String, String, Option<String>, Instant)>);

impl RepeatGuard {
    const WINDOW: Duration = Duration::from_millis(300);

    /// Whether the action goes out; remembers it either way.
    fn allow(&mut self, instance: &str, action: &str, value: Option<&str>, now: Instant) -> bool {
        let repeat = self.0.as_ref().is_some_and(|(i, a, v, at)| {
            i == instance && a == action && v.as_deref() == value && now.saturating_duration_since(*at) < Self::WINDOW
        });
        self.0 = Some((instance.into(), action.into(), value.map(Into::into), now));
        !repeat
    }
}

impl Plugins {
    fn publish_studio(&self) {
        if self.roots.is_none() { self.studio.publish(); }
    }
    fn studio_request(&mut self, line: &str, now: Instant) {
        use crate::plugin_manager::Request;
        let result = (|| -> anyhow::Result<()> {
            let request: Request = serde_json::from_str(line)?;
            self.studio.error = None;
            match request {
                Request::Refresh => {
                    if let Some(helper) = &self.helper { let _ = helper.frames.send(wire::HostMsg::Stats); }
                    for (id, process) in &self.procs {
                        if process.manifest.component.is_some() && self.component_status.contains_key(id) { continue; }
                        self.studio.plugins.entry(id.clone()).or_default().status = match process.state {
                            State::Ready => "Live", State::Starting { .. } => "Restarting", State::Stopping { .. } => "Unloaded",
                        }.into();
                    }
                }
                Request::Disable(id) | Request::Uninstall(id) => {
                    anyhow::ensure!(crate::manifest::valid_id(&id), "Invalid plugin id");
                    let uninstall = matches!(serde_json::from_str::<Request>(line)?, Request::Uninstall(_));
                    let mut prefs = self.preferences.clone();
                    prefs.disabled.insert(id.clone());
                    prefs.save()?;
                    self.preferences = prefs;
                    self.restarts.remove(&id);
                    if let Some(mut process) = self.procs.remove(&id) { let _ = process.child.kill(); let _ = process.child.wait(); }
                    self.styles.retain(|(plugin, _), _| plugin != &id);
                    self.component_status.insert(id.clone(), wire::Status::Stopped("Disabled".into()));
                    self.studio.plugins.entry(id.clone()).or_default().status = if uninstall { "Uninstalling…" } else { "Stopped: Disabled" }.into();
                    self.mark(&id);
                    if let Some(helper) = &self.helper {
                        helper.frames.send(wire::HostMsg::Remove { plugin: id.clone(), acknowledge: uninstall })?;
                        if uninstall { self.uninstalling.insert(id); }
                    } else if uninstall { self.finish_uninstall(&id); }
                }
                Request::Enable(id) => {
                    anyhow::ensure!(crate::manifest::valid_id(&id), "Invalid plugin id");
                    anyhow::ensure!(!self.uninstalling.contains(&id), "Wait for this plugin to finish uninstalling");
                    let mut prefs = self.preferences.clone(); prefs.disabled.remove(&id); prefs.uninstalled.remove(&id); prefs.save()?;
                    self.preferences = prefs;
                    self.component_status.remove(&id);
                    self.studio.plugins.entry(id.clone()).or_default().status = "Unloaded".into();
                    let opens: Vec<_> = self.faces.iter().filter(|(_, s)| s.plugin == id && s.component.is_some()).map(|(instance, slot)| component_open(instance, slot)).collect();
                    for message in opens { self.send_to_helper(message, now).map_err(|e| anyhow::anyhow!(e.message))?; }
                    self.mark(&id);
                }
                Request::Settings(id, settings) => {
                    anyhow::ensure!(crate::manifest::valid_id(&id) && settings.is_object(), "Invalid plugin settings");
                    anyhow::ensure!(!self.uninstalling.contains(&id), "Wait for this plugin to finish uninstalling");
                    let (manifest, _) = self.find(&id).ok_or_else(|| anyhow::anyhow!("Plugin {id} is not installed"))?;
                    let settings = crate::plugin_manager::resolve_settings(&manifest, &settings)?;
                    let mut prefs = self.preferences.clone(); prefs.settings.insert(id.clone(), settings.clone()); prefs.save()?; self.preferences = prefs;
                    let mut opens = Vec::new();
                    for (instance, slot) in self.faces.iter_mut().filter(|(_, s)| s.plugin == id) {
                        if let (Some(current), Some(saved)) = (slot.settings.as_object_mut(), settings.as_object()) { current.extend(saved.clone()); }
                        if slot.component.is_some() { opens.push(component_open(instance, slot)); }
                    }
                    if !self.preferences.disabled.contains(&id) {
                        for message in opens { self.send_to_helper(message, now).map_err(|e| anyhow::anyhow!(e.message))?; }
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result { self.studio.error = Some(format!("{error:#}")); }
        self.publish_studio();
    }

    fn finish_uninstall(&mut self, id: &str) {
        match crate::plugin_manager::uninstall(id) {
            Ok(()) => {
                self.studio.plugins.remove(id);
                self.preferences.uninstalled.insert(id.into());
                self.preferences.settings.remove(id);
                if let Err(error) = self.preferences.save() { self.studio.error = Some(format!("Uninstall {id}: {error:#}")); }
            }
            Err(error) => { self.studio.error = Some(format!("Uninstall {id}: {error:#}")); }
        }
        self.publish_studio();
    }

    pub fn new(waker: Waker) -> Self {
        crate::plugin_manager::connect(waker.clone());
        Self {
            studio: Default::default(),
            preferences: crate::plugin_manager::Preferences::load(),
            legacy_settings: HashMap::new(),
            uninstalling: Default::default(),
            procs: HashMap::new(),
            faces: BTreeMap::new(),
            changed: Vec::new(),
            roots: None,
            subs: BTreeMap::new(),
            restarts: HashMap::new(),
            dying: Vec::new(),
            job: None,
            waker,
            held: false,
            game_active: false,
            next_generation: 0,
            helper: None,
            helper_retry: None,
            component_status: HashMap::new(),
            styles: BTreeMap::new(),
            repeats: RepeatGuard::default(),
        }
    }

    /// Forwards a call, starting the plugin if needed.
    pub fn call(
        &mut self,
        target: Target,
        plugin: &str,
        method: String,
        params: Option<Value>,
        now: Instant,
    ) -> Option<Out> {
        let process = match self.running(plugin, now) {
            Ok(process) => process,
            Err(e) => return Some(Out::Reply(target, Err(e))),
        };
        if !process.manifest.methods.contains(&method) {
            let message = format!("plugin {plugin:?} has no method {method:?}");
            return Some(Out::Reply(
                target,
                Err(WidgetError::new(ErrorCode::NotFound, message)),
            ));
        }
        process.idle_since = None;
        match process.state {
            State::Ready => process.send(target, &method, params.as_ref(), now),
            _ => process.queued.push((target, method, params)),
        }
        None
    }

    /// Subscription `sub` gets each `event` of `plugin`, starting the plugin if needed.
    pub fn subscribe(
        &mut self,
        instance: &str,
        sub: &str,
        plugin: &str,
        event: &str,
        now: Instant,
    ) -> Result<(), WidgetError> {
        let process = self.running(plugin, now)?;
        if !process.manifest.events.contains(&event.to_owned()) {
            let message = format!("plugin {plugin:?} has no event {event:?}");
            return Err(WidgetError::new(ErrorCode::NotFound, message));
        }
        process.idle_since = None;
        self.subs
            .insert((instance.into(), sub.into()), (plugin.into(), event.into()));
        Ok(())
    }

    pub fn unsubscribe(&mut self, instance: &str, sub: &str) {
        self.subs.remove(&(instance.into(), sub.into()));
    }

    /// Drops an instance's subscriptions (it reloaded or went away).
    pub fn forget(&mut self, instance: &str) {
        self.subs.retain(|(i, _), _| i != instance);
    }

    fn listened(&self, plugin: &str) -> bool {
        self.subs.values().any(|(p, _)| p == plugin) || self.faces.values().any(|s| s.plugin == plugin && s.component.is_none())
    }

    /// A plugin's manifest and folder, from `roots` or the installed and bundled folders.
    fn find(&self, id: &str) -> Option<(PluginManifest, std::path::PathBuf)> {
        match &self.roots {
            Some(roots) => roots.iter().map(|root| root.join(id)).find_map(|dir| PluginManifest::load(&dir).ok().map(|manifest| (manifest, dir))),
            None => crate::registry::find_plugin(id),
        }
    }

    /// Style patches component plugins applied, for the bars and theme (docs/plugins.md, "Styling").
    pub fn style_patches(&self) -> impl Iterator<Item = &wire::StylePatch> {
        self.styles.values()
    }

    /// Sends a frame to the helper, starting it if it isn't running (and isn't backing off).
    fn send_to_helper(&mut self, message: wire::HostMsg, now: Instant) -> Result<(), WidgetError> {
        if let Some(helper) = &mut self.helper {
            helper.idle_since = None;
            let _ = helper.frames.send(message);
            return Ok(());
        }
        if self.helper_retry.is_some_and(|(at, _)| now < at) {
            return Ok(()); // it opens every placed face when it's back
        }
        // Starting it opens every placed component face, including this one.
        self.start_helper().map_err(|e| unavailable(format!("the plugin runtime didn't start: {e:#}")))
    }

    fn start_helper(&mut self) -> Result<()> {
        use std::sync::{Arc, Mutex};
        let exe = helper_exe().context("mypaper-plugins.exe isn't installed beside the shell")?;
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .context("running mypaper-plugins")?;
        if self.job.is_none() {
            self.job = Job::new().inspect_err(|e| log::warn!("no job object for plugins: {e:#}")).ok();
        }
        if let Some(job) = &self.job
            && let Err(e) = job.assign(&child)
        {
            log::warn!("the plugin runtime isn't in the job object: {e:#}");
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let mut stdin = child.stdin.take().context("no stdin")?;
        let inbox = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let (queue, waker) = (Arc::clone(&inbox), self.waker.clone());
        std::thread::Builder::new().name("plugins out".into()).spawn(move || {
            let wake = |line| waker.send(Msg::Plugin { id: HELPER.into(), generation, line });
            let mut stdout = BufReader::new(stdout);
            while let Ok(Some(message)) = wire::read::<wire::HelperMsg>(&mut stdout) {
                let first = {
                    let mut queue = queue.lock().expect("plugin inbox");
                    queue.push_back(message);
                    queue.len() == 1
                };
                if first {
                    wake(Some(String::new()));
                }
            }
            wake(None);
        })?;
        std::thread::Builder::new().name("plugins err".into()).spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::info!("[plugins] {line}");
            }
        })?;
        let (frames, rx) = mpsc::channel::<wire::HostMsg>();
        std::thread::Builder::new().name("plugins in".into()).spawn(move || {
            for message in rx {
                if wire::write(&mut stdin, &message).is_err() {
                    break;
                }
            }
        })?;
        if self.game_active {
            let _ = frames.send(wire::HostMsg::GameActivity(true));
        }
        if self.held {
            let _ = frames.send(wire::HostMsg::Held(true));
        }
        for (instance, slot) in self.faces.iter().filter(|(_, s)| s.component.is_some() && !self.preferences.disabled.contains(&s.plugin)) {
            let _ = frames.send(component_open(instance, slot));
        }
        log::info!("plugin runtime started");
        self.helper = Some(Helper { child, generation, frames, inbox, idle_since: None, notification_plugins: Vec::new(), demand: Vec::new() });
        Ok(())
    }

    /// Frames from the helper (`line: Some`), or its exit (`None`).
    fn on_helper(&mut self, generation: u64, line: Option<String>, now: Instant) -> Vec<Out> {
        let Some(helper) = self.helper.as_mut().filter(|h| h.generation == generation) else {
            return Vec::new();
        };
        if line.is_none() {
            let mut helper = self.helper.take().expect("just found");
            let _ = helper.child.wait();
            if self.faces.values().any(|s| s.component.is_some()) {
                let delay = self.helper_retry.map_or(RESTART_DELAY, |(_, d)| d);
                log::warn!("the plugin runtime exited; restarting it in {delay:?}");
                self.helper_retry = Some((now + delay, (delay * 2).min(Duration::from_secs(60))));
            }
            self.component_status.clear();
            for metrics in self.studio.plugins.values_mut() { metrics.memory = 0; metrics.status = "Restarting: runtime exited".into(); }
            let pending: Vec<_> = self.uninstalling.iter().cloned().collect();
            self.uninstalling.clear();
            for plugin in pending { self.finish_uninstall(&plugin); }
            self.publish_studio();
            let placed = self.faces.iter().filter(|(_, s)| s.component.is_some()).map(|(i, _)| i.clone());
            self.changed.extend(placed.collect::<Vec<_>>());
            return Vec::new();
        }
        let messages: Vec<_> = helper.inbox.lock().expect("plugin inbox").drain(..).collect();
        for message in messages {
            if self.studio.route(&message) { self.publish_studio(); }
            match message {
                wire::HelperMsg::View { instance, face, .. } => match serde_json::from_slice::<Face>(&face) {
                    Ok(face) => {
                        if let Some(slot) = self.faces.get_mut(&instance).filter(|s| s.component.is_some()) {
                            slot.view = Some(face);
                            self.changed.push(instance);
                        }
                    }
                    Err(e) => log::warn!("plugin view for {instance}: {e}"),
                },
                wire::HelperMsg::Status { plugin, status } => {
                    if status == wire::Status::Live {
                        self.helper_retry = None;
                    }
                    self.component_status.insert(plugin.clone(), status);
                    self.mark(&plugin);
                }
                wire::HelperMsg::Style { plugin, surface, patch } => {
                    log::info!("plugin {plugin} {} the {surface} style", if patch.is_some() { "patched" } else { "reset" });
                    match patch {
                        Some(patch) => self.styles.insert((plugin, surface), patch),
                        None => self.styles.remove(&(plugin, surface)),
                    };
                }
                wire::HelperMsg::Stats { .. } => {}
                wire::HelperMsg::Notifications { plugins } => {
                    if let Some(helper) = &mut self.helper { helper.notification_plugins = plugins; }
                }
                wire::HelperMsg::Demand { topics } => {
                    if let Some(helper) = &mut self.helper { helper.demand = topics; }
                }
                wire::HelperMsg::Removed { plugin } => {
                    if self.uninstalling.remove(&plugin) { self.finish_uninstall(&plugin); }
                }
                wire::HelperMsg::Launched { plugin, target } => {
                    log::debug!("plugin {plugin}: opening {target}");
                    open_and_raise(plugin, target);
                }
            }
        }
        Vec::new()
    }

    /// Stops an idle helper and starts a dead one again when faces need it.
    fn tick_helper(&mut self, now: Instant) {
        if self.game_active { return; }
        let placed = self.faces.values().any(|s| s.component.is_some() && !self.preferences.disabled.contains(&s.plugin));
        if let Some(helper) = &mut self.helper {
            if placed {
                helper.idle_since = None;
            } else if now >= *helper.idle_since.get_or_insert(now) + IDLE_STOP {
                log::info!("no component plugin placed; stopping the plugin runtime");
                let helper = self.helper.take().expect("just matched");
                let _ = helper.frames.send(wire::HostMsg::Shutdown);
                self.dying.push((helper.child, now + SHUTDOWN_GRACE * 3));
            }
        } else if placed
            && !self.held
            && let Some((at, delay)) = self.helper_retry
            && now >= at
            && let Err(e) = self.start_helper()
        {
            log::warn!("restarting the plugin runtime: {e:#}");
            self.helper_retry = Some((now + delay, (delay * 2).min(Duration::from_secs(60))));
        }
    }

    fn helper_deadline(&self) -> Option<Instant> {
        if self.game_active { return None; }
        match &self.helper {
            Some(helper) => helper.idle_since.map(|t| t + IDLE_STOP),
            None if self.held => None,
            None => self.helper_retry.map(|(at, _)| at).filter(|_| self.faces.values().any(|s| s.component.is_some() && !self.preferences.disabled.contains(&s.plugin))),
        }
    }

    /// Looks plugins up in `roots` only (tests and harnesses).
    pub fn with_roots(waker: Waker, roots: Vec<std::path::PathBuf>) -> Self {
        Self {
            roots: Some(roots),
            preferences: Default::default(),
            ..Self::new(waker)
        }
    }

    /// Places (or re-sizes) `instance` as `plugin`'s `face`, starting the plugin if needed. The
    /// plugin answers with a `view`; until then [`Plugins::face_view`] is the cached one.
    pub fn open_face(
        &mut self,
        instance: &str,
        plugin: &str,
        face: &str,
        size: SizeClass,
        settings: Value,
        now: Instant,
    ) -> Result<(), WidgetError> {
        if self.preferences.disabled.contains(plugin) { return Err(unavailable(format!("plugin {plugin} is disabled"))); }
        let mut settings = settings;
        if let Some((manifest, _)) = self.find(plugin) {
            let legacy = self.legacy_settings.entry(plugin.into()).or_insert_with(|| plugin_settings(plugin));
            if let Some(values) = settings.as_object_mut() {
                for schema in &manifest.settings {
                    if let Some(key) = schema["key"].as_str() && let Some(value) = legacy.get(key) {
                        values.entry(key.to_owned()).or_insert_with(|| value.clone());
                    }
                }
            }
            let saved = self.preferences.settings.get(plugin).cloned().unwrap_or_else(|| json!({}));
            if let Ok(Value::Object(defaults)) = crate::plugin_manager::resolve_settings(&manifest, &json!({}))
                && let Some(values) = settings.as_object_mut() {
                for (key, value) in defaults { values.entry(key).or_insert(value); }
                if let Some(saved) = saved.as_object() { values.extend(saved.clone()); }
            }
        }
        if let Some(slot) = self.faces.get_mut(instance)
            && slot.plugin == plugin
            && slot.face == face
            && slot.component.is_some()
        {
            if slot.size == size && slot.settings != settings {
                // Only the options changed: `event::options`, the face stays open.
                slot.settings = settings;
                let message = wire::HostMsg::Options { instance: instance.into(), settings: slot.settings.to_string() };
                self.send_to_helper(message, now)?;
            } else if slot.size != size || slot.settings != settings {
                slot.size = size;
                slot.settings = settings;
                let message = component_open(instance, slot);
                self.send_to_helper(message, now)?;
            }
            return Ok(());
        }
        if !self.faces.contains_key(instance)
            && let Some((manifest, dir)) = self.find(plugin).filter(|(m, _)| m.component.is_some())
        {
            if !manifest.faces.iter().chain(&manifest.bars).chain(&manifest.panels).any(|f| f.id == face) {
                let message = format!("plugin {plugin:?} has no face {face:?}");
                return Err(WidgetError::new(ErrorCode::NotFound, message));
            }
            let slot = Slot { plugin: plugin.into(), face: face.into(), size, settings, view: None, component: Some(dir) };
            let message = component_open(instance, &slot);
            self.faces.insert(instance.into(), slot);
            self.changed.push(instance.into());
            return self.send_to_helper(message, now);
        }
        if let Some(slot) = self.faces.get_mut(instance)
            && slot.plugin == plugin
            && slot.face == face
        {
            if slot.size != size || slot.settings != settings {
                slot.size = size;
                slot.settings = settings;
                let message = HostMessage::Open {
                    instance: instance.into(),
                    face: face.into(),
                    size,
                    settings: slot.settings.clone(),
                };
                if let Some(process) = self.procs.get(plugin).filter(|p| matches!(p.state, State::Ready)) {
                    process.write(sdk::host_message(&message));
                }
            }
            return Ok(());
        }
        let message = HostMessage::Open {
            instance: instance.into(),
            face: face.into(),
            size,
            settings: settings.clone(),
        };
        let slot = Slot {
            plugin: plugin.into(),
            face: face.into(),
            size,
            settings,
            view: None,
            component: None,
        };
        self.faces.insert(instance.into(), slot);
        let process = match self.running(plugin, now) {
            Ok(process) => process,
            Err(e) => {
                self.faces.remove(instance);
                return Err(e);
            }
        };
        if !process.manifest.faces.iter().any(|f| f.id == face) {
            self.faces.remove(instance);
            let message = format!("plugin {plugin:?} has no face {face:?}");
            return Err(WidgetError::new(ErrorCode::NotFound, message));
        }
        process.idle_since = None;
        if matches!(process.state, State::Ready) {
            process.write(sdk::host_message(&message));
        }
        Ok(())
    }

    /// The instance went away; the plugin stops 30 s after its last face closes.
    pub fn close_face(&mut self, instance: &str) {
        let Some(slot) = self.faces.remove(instance) else { return };
        if slot.component.is_some() {
            if let Some(helper) = &self.helper {
                let _ = helper.frames.send(wire::HostMsg::Close { instance: instance.into() });
            }
            return;
        }
        if let Some(process) = self.procs.get(&slot.plugin).filter(|p| matches!(p.state, State::Ready)) {
            process.write(sdk::host_message(&HostMessage::Close { instance: instance.into() }));
        }
    }

    /// A click on a view's `action` (or a submitted field's `value`). The same action again
    /// within 300 ms is dropped ([`RepeatGuard`]).
    pub fn face_action(&mut self, instance: &str, action: &str, value: Option<String>) {
        let Some(slot) = self.faces.get(instance) else {
            log::warn!("plugin action {action:?} on {instance}: no face open there");
            return;
        };
        if !self.repeats.allow(instance, action, value.as_deref(), Instant::now()) {
            log::warn!("plugin {}: action {action:?} on {instance} repeated within 300 ms; dropped", slot.plugin);
            return;
        }
        if slot.component.is_some() {
            let Some(helper) = &self.helper else {
                log::warn!("plugin {}: action {action:?} on {instance} dropped: the plugin runtime isn't running", slot.plugin);
                return;
            };
            // The click is the user's: let the app the plugin opens for it come to the front
            // (also raised by `Launched`, see `raise_handler`). The right lasts until the next input.
            // SAFETY: plain Win32 call; failure only means the opened app may not come forward.
            let allowed = unsafe {
                windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                    windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
                )
            };
            log::debug!("plugin {}: action {action:?} on {instance} -> helper (foreground right passed on: {allowed:?})", slot.plugin);
            if helper.frames.send(wire::HostMsg::Action { instance: instance.into(), action: action.into(), value }).is_err() {
                log::warn!("plugin {}: action {action:?} on {instance} lost: the runtime's pipe closed", slot.plugin);
            }
            return;
        }
        if let Some(process) = self.procs.get(&slot.plugin).filter(|p| matches!(p.state, State::Ready)) {
            let message = HostMessage::Action {
                instance: instance.into(),
                action: action.into(),
                value,
            };
            process.write(sdk::host_message(&message));
        }
    }

    /// What `instance` last drew (kept across plugin restarts).
    pub fn face_view(&self, instance: &str) -> Option<&Face> {
        self.faces.get(instance)?.view.as_ref()
    }

    pub fn face_status(&self, instance: &str) -> Option<FaceStatus> {
        let slot = self.faces.get(instance)?;
        if self.preferences.disabled.contains(&slot.plugin) { return Some(FaceStatus::Failed("Disabled".into())); }
        if slot.component.is_some() {
            return Some(match self.component_status.get(&slot.plugin) {
                _ if self.helper.is_none() && self.helper_retry.is_some() => FaceStatus::Restarting,
                Some(wire::Status::Live | wire::Status::Unloaded) => FaceStatus::Live,
                Some(wire::Status::Restarting(_)) => FaceStatus::Restarting,
                Some(wire::Status::Stopped(why)) => FaceStatus::Failed(why.clone()),
                None if slot.view.is_some() => FaceStatus::Live,
                None => FaceStatus::Starting,
            });
        }
        Some(match self.procs.get(&slot.plugin).map(|p| &p.state) {
            Some(State::Ready) => FaceStatus::Live,
            Some(_) => FaceStatus::Starting,
            None if self.restarts.contains_key(&slot.plugin) => FaceStatus::Restarting,
            None => FaceStatus::Failed("the plugin isn't running".into()),
        })
    }

    /// Instances to repaint: their view or status changed.
    pub fn take_changed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.changed)
    }

    fn mark(&mut self, plugin: &str) {
        let instances = self.faces.iter().filter(|(_, s)| s.plugin == plugin).map(|(i, _)| i.clone());
        self.changed.extend(instances);
    }

    /// The process for `id`, started if it isn't running.
    fn running(&mut self, id: &str, now: Instant) -> Result<&mut Process, WidgetError> {
        if self.preferences.disabled.contains(id) { return Err(unavailable(format!("plugin {id} is disabled"))); }
        if let Some(process) = self.procs.remove(id) {
            if let State::Stopping { deadline } = process.state {
                self.dying.push((process.child, deadline)); // let it finish; start afresh
            } else {
                return Ok(self.procs.entry(id.to_owned()).or_insert(process));
            }
        }
        let found = match &self.roots {
            Some(roots) => roots.iter().map(|root| root.join(id)).find_map(|dir| {
                PluginManifest::load(&dir).ok().map(|manifest| (manifest, dir))
            }),
            None => crate::registry::find_plugin(id),
        };
        let (manifest, dir) = found.ok_or_else(|| unavailable(format!("plugin {id:?} isn't installed")))?;
        let process = self
            .spawn(manifest, &dir, now)
            .map_err(|e| unavailable(format!("plugin {id:?} didn't start: {e:#}")))?;
        log::info!("plugin {id} started");
        Ok(self.procs.entry(id.to_owned()).or_insert(process))
    }

    fn spawn(&mut self, manifest: PluginManifest, dir: &Path, now: Instant) -> Result<Process> {
        let id = manifest.id.clone();
        let data_dir = crate::files::user("plugin-data")?.join(&id);
        std::fs::create_dir_all(&data_dir)?;
        let mut child = Command::new(program(dir, &manifest.command))
            .args(&manifest.args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .with_context(|| format!("running {}", manifest.command))?;
        if self.job.is_none() {
            self.job = Job::new()
                .inspect_err(|e| log::warn!("no job object for plugins: {e:#}"))
                .ok();
        }
        if let Some(job) = &self.job
            && let Err(e) = job.assign(&child)
        {
            log::warn!("plugin {id} isn't in the job object: {e:#}");
        }

        self.next_generation += 1;
        let generation = self.next_generation;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let mut stdin = child.stdin.take().context("no stdin")?;
        let waker = self.waker.clone();
        let reader_id = id.clone();
        std::thread::Builder::new()
            .name(format!("plugin {id} out"))
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let id = reader_id.clone();
                    waker.send(Msg::Plugin {
                        id,
                        generation,
                        line: Some(line),
                    });
                }
                waker.send(Msg::Plugin {
                    id: reader_id,
                    generation,
                    line: None,
                });
            })?;
        let log_id = id.clone();
        std::thread::Builder::new()
            .name(format!("plugin {id} err"))
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::info!("[plugin {log_id}] {line}");
                }
            })?;
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name(format!("plugin {id} in"))
            .spawn(move || {
                for line in rx {
                    if stdin
                        .write_all(line.as_bytes())
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;

        // Data plugins (no `sdk`) keep the protocol they were written for.
        let protocol = if manifest.sdk.is_some() { sdk::PROTOCOL } else { 1 };
        let params = json!({ "protocol": protocol, "sdk": format!("{}.{}", mypaper_plugin_sdk::SDK_MAJOR, mypaper_plugin_sdk::SDK_MINOR),
                             "plugin": id, "dataDir": crate::files::plain(&data_dir), "settings": plugin_settings(&id) });
        let _ = tx.send(rpc::request(rpc::INIT_ID, "initialize", Some(&params)));
        Ok(Process {
            manifest,
            generation,
            child,
            stdin: Some(tx),
            state: State::Starting {
                deadline: now + INIT_TIMEOUT,
            },
            pending: Pending::default(),
            queued: Vec::new(),
            idle_since: None,
        })
    }

    /// Handles a line from a plugin, or its exit (`line: None`).
    pub fn on_line(
        &mut self,
        id: &str,
        generation: u64,
        line: Option<String>,
        now: Instant,
    ) -> Vec<Out> {
        if id == crate::plugin_manager::STUDIO {
            if let Some(line) = line { self.studio_request(&line, now); }
            return Vec::new();
        }
        if id == HELPER {
            return self.on_helper(generation, line, now);
        }
        let Some(process) = self
            .procs
            .get_mut(id)
            .filter(|p| p.generation == generation)
        else {
            return Vec::new(); // a process we already replaced
        };
        let Some(line) = line else {
            let mut process = self.procs.remove(id).expect("just found");
            let _ = process.child.wait();
            let stopping = matches!(process.state, State::Stopping { .. });
            if !stopping {
                log::warn!("plugin {id} exited unexpectedly");
                if self.listened(id) {
                    let delay = self.restarts.get(id).map_or(RESTART_DELAY, |(_, d)| *d);
                    self.restarts.insert(
                        id.to_owned(),
                        (now + delay, (delay * 2).min(Duration::from_secs(60))),
                    );
                }
            } else {
                log::info!("plugin {id} stopped");
            }
            self.mark(id);
            return process.fail_all("the plugin exited");
        };
        if process.manifest.sdk.is_some() {
            match sdk::parse_from_plugin(&line) {
                FromPlugin::View(update) => {
                    if let Some(slot) = self.faces.get_mut(&update.instance).filter(|s| s.plugin == id)
                        && slot.view.as_ref() != Some(&update.tree)
                    {
                        slot.view = Some(update.tree);
                        self.changed.push(update.instance);
                    }
                    return Vec::new();
                }
                FromPlugin::Request { id: request, request: what } => {
                    process.write(serve(&process.manifest, &request, what));
                    return Vec::new();
                }
                FromPlugin::Unknown { id: request } => {
                    process.write(rpc::method_not_found(&request));
                    return Vec::new();
                }
                FromPlugin::Other => {}
            }
        }
        match rpc::parse(&line) {
            Incoming::Response {
                id: rpc::INIT_ID,
                outcome,
            } if matches!(process.state, State::Starting { .. }) => match outcome {
                Ok(result)
                    if result.get("protocol").and_then(Value::as_u64)
                        == Some(if process.manifest.sdk.is_some() { sdk::PROTOCOL as u64 } else { 1 }) =>
                {
                    process.state = State::Ready;
                    self.restarts.remove(id);
                    if self.held {
                        process.write(rpc::notification("suspend"));
                    }
                    for (target, method, params) in std::mem::take(&mut process.queued) {
                        process.send(target, &method, params.as_ref(), now);
                    }
                    // Faces placed while it started (or before it crashed) open now.
                    for (instance, slot) in self.faces.iter().filter(|(_, s)| s.plugin == id) {
                        let message = HostMessage::Open {
                            instance: instance.clone(),
                            face: slot.face.clone(),
                            size: slot.size,
                            settings: slot.settings.clone(),
                        };
                        process.write(sdk::host_message(&message));
                    }
                    self.mark(id);
                    Vec::new()
                }
                other => {
                    log::warn!("plugin {id} failed its handshake: {other:?}");
                    let _ = process.child.kill();
                    process.fail_all("the plugin failed its handshake")
                }
            },
            Incoming::Response { id: call, outcome } => match process.pending.take(call) {
                Some(target) => vec![Out::Reply(target, outcome.map_err(widget_error))],
                None => Vec::new(),
            },
            Incoming::Event { event, data } => {
                if self.held || !process.manifest.events.contains(&event) {
                    return Vec::new();
                }
                self.subs
                    .iter()
                    .filter(|(_, (p, e))| p == id && *e == event)
                    .map(|((instance, sub), _)| Out::Event {
                        instance: instance.clone(),
                        event: "sub",
                        data: json!({ "sub": sub, "value": data }),
                    })
                    .collect()
            }
            Incoming::Request { id: request } => {
                process.write(rpc::method_not_found(&request));
                Vec::new()
            }
            Incoming::Invalid(why) => {
                log::warn!("plugin {id}: {why}");
                Vec::new()
            }
        }
    }

    fn users(&self, id: &str, process: &Process) -> usize {
        process.pending.len()
            + process.queued.len()
            + self.subs.values().filter(|(p, _)| p == id).count()
            + self.faces.values().filter(|s| s.plugin == id && s.component.is_none()).count()
    }

    /// Timeouts, idle stops, kills after the grace period, and restarts.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        self.tick_helper(now);
        let ids: Vec<String> = self.procs.keys().cloned().collect();
        for id in ids {
            let users = self.users(&id, &self.procs[&id]);
            let process = self.procs.get_mut(&id).expect("listed above");
            out.extend(process.pending.expire(now).into_iter().map(|target| {
                Out::Reply(target, Err(unavailable("the plugin didn't answer in 30 s")))
            }));
            match process.state {
                State::Starting { deadline } if now >= deadline => {
                    log::warn!("plugin {id} didn't initialize in {INIT_TIMEOUT:?}");
                    let _ = process.child.kill();
                    out.extend(process.fail_all("the plugin didn't start"));
                    self.procs.remove(&id);
                    continue;
                }
                State::Stopping { deadline } if now >= deadline => {
                    log::warn!("plugin {id} ignored shutdown; killing it");
                    let _ = process.child.kill();
                    let _ = process.child.wait();
                    self.procs.remove(&id);
                    continue;
                }
                State::Ready | State::Starting { .. } if users == 0 => {
                    let since = *process.idle_since.get_or_insert(now);
                    if now >= since + IDLE_STOP {
                        log::info!("plugin {id} idle; stopping it");
                        process.stop(now);
                    }
                }
                _ => process.idle_since = None,
            }
        }
        self.dying.retain_mut(|(child, deadline)| {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return false;
            }
            if now >= *deadline {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            true
        });
        let due: Vec<String> = self
            .restarts
            .iter()
            .filter(|(id, (at, _))| now >= *at && !self.procs.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            if self.listened(&id)
                && !self.held
                && let Err(e) = self.running(&id, now)
            {
                log::warn!("restarting plugin {id}: {}", e.message);
            }
            if let Some((at, delay)) = self.restarts.get_mut(&id) {
                *at = now + *delay; // try again later unless the handshake clears it
            }
        }
        out
    }

    pub fn deadline(&self) -> Option<Instant> {
        let per_process = self.procs.iter().flat_map(|(id, process)| {
            let state = match process.state {
                State::Starting { deadline } | State::Stopping { deadline } => Some(deadline),
                State::Ready => None,
            };
            let idle = (self.users(id, process) == 0)
                .then(|| process.idle_since.map_or(Instant::now(), |t| t + IDLE_STOP));
            [state, process.pending.deadline(), idle]
        });
        let restarts = self
            .restarts
            .iter()
            .filter(|(id, _)| !self.held && !self.game_active && self.listened(id))
            .map(|(_, (at, _))| Some(*at));
        let dying = self.dying.iter().map(|(_, deadline)| Some(*deadline));
        per_process.chain(restarts).chain(dying).chain([self.helper_deadline()]).flatten().min()
    }

    /// Sends `suspend` / `resume` to running plugins.
    pub fn set_held(&mut self, held: bool) {
        if self.held == held {
            return;
        }
        self.held = held;
        if let Some(helper) = &self.helper {
            let _ = helper.frames.send(wire::HostMsg::Held(held));
        }
        let note = rpc::notification(if held || self.game_active { "suspend" } else { "resume" });
        for process in self
            .procs
            .values()
            .filter(|p| matches!(p.state, State::Ready))
        {
            process.write(note.clone());
        }
    }

    /// Relay core's background pause independently of desktop visibility and user pause.
    pub fn set_game_activity(&mut self, active: bool) {
        if self.game_active == active { return; }
        self.game_active = active;
        if let Some(helper) = &self.helper {
            let _ = helper.frames.send(wire::HostMsg::GameActivity(active));
        }
        let note = rpc::notification(if active || self.held { "suspend" } else { "resume" });
        for process in self.procs.values().filter(|p| matches!(p.state, State::Ready)) {
            process.write(note.clone());
        }
    }

    /// Forwards a system event (window, monitor, notification, media, power, theme) to the WASM
    /// helper, which delivers it only to plugins subscribed to its topic (docs/plugins.md,
    /// "Events"). Dropped when no helper runs, so sources can send unconditionally.
    pub fn system_event(&self, event: wire::SystemEvent) {
        if let Some(helper) = &self.helper {
            let _ = helper.frames.send(wire::HostMsg::System(event));
        }
    }

    /// Whether a visible plugin subscribes to system topic `topic` (`windows`, `monitors`,
    /// `media`, `power`, `theme`): the shell runs that source only while this holds.
    pub fn wants(&self, topic: &str) -> bool {
        !self.held && !self.game_active && self.helper.as_ref().is_some_and(|h| h.demand.iter().any(|t| t == topic))
    }

    /// Invokes `plugin`'s `commands`/`menu` contribution `id` (a shortcut, a desktop-menu item),
    /// starting the runtime if needed; the helper loads the plugin for it.
    pub fn command(&mut self, plugin: &str, id: &str, now: Instant) -> Result<(), WidgetError> {
        if self.preferences.disabled.contains(plugin) {
            return Err(unavailable(format!("plugin {plugin} is disabled")));
        }
        let (_, dir) = self.find(plugin).filter(|(m, _)| m.component.is_some()).ok_or_else(|| unavailable(format!("plugin {plugin} isn't an installed component")))?;
        let message = wire::HostMsg::Command { plugin: plugin.into(), dir: dir.to_string_lossy().into_owned(), id: id.into() };
        if self.helper.is_none() {
            self.send_to_helper(wire::HostMsg::Held(self.held), now)?; // starts it
        }
        self.send_to_helper(message, now)
    }

    /// Instances of permitted notification subscribers. The shell checks actual visibility
    /// before acquiring its shared Windows notification lease.
    pub fn notification_instances(&self) -> Vec<&str> {
        if self.held || self.game_active { return Vec::new(); }
        let Some(helper) = &self.helper else { return Vec::new() };
        self.faces.iter().filter(|(_, slot)| helper.notification_plugins.contains(&slot.plugin))
            .map(|(instance, _)| instance.as_str()).collect()
    }

    /// Stops every plugin before the host exits: `shutdown`, up to the grace
    /// period, then kill.
    pub fn shutdown(&mut self) {
        let now = Instant::now();
        for process in self.procs.values_mut() {
            if !matches!(process.state, State::Stopping { .. }) {
                process.stop(now);
            }
        }
        let mut children: Vec<Child> = self
            .procs
            .drain()
            .map(|(_, p)| p.child)
            .chain(self.dying.drain(..).map(|(c, _)| c))
            .chain(self.helper.take().map(|helper| {
                let _ = helper.frames.send(wire::HostMsg::Shutdown);
                helper.child
            }))
            .collect();
        let deadline = now + SHUTDOWN_GRACE;
        while Instant::now() < deadline
            && children
                .iter_mut()
                .any(|c| matches!(c.try_wait(), Ok(None)))
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        for child in &mut children {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
        }
    }
}

/// The URL scheme of a launch target (`t3code` of `t3code://threads/…`); none for a path
/// (`C:\…` has a one-letter "scheme").
fn scheme(target: &str) -> Option<&str> {
    let (scheme, _) = target.split_once(':')?;
    let valid = scheme.len() > 1 && scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    valid.then_some(scheme)
}

/// Opens a plugin's `launch.open` target from the shell, off its thread (ShellExecute starts the
/// app and can take a while), then raises the app that handles it. The shell got the click, so
/// what it starts runs in the user's context and may come forward; opened from the hidden helper,
/// T3 Code's second instance never handed the thread link to the running one.
fn open_and_raise(plugin: String, target: String) {
    let spawned = std::thread::Builder::new().name("launch".into()).stack_size(256 << 10).spawn(move || {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, w};
        // SAFETY: COM for this thread (ShellExecute may use it); the strings outlive the call.
        let code = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            ShellExecuteW(None, w!("open"), &HSTRING::from(target.as_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
        };
        if code.0 as usize <= 32 {
            log::warn!("plugin {plugin}: opening {target} failed (ShellExecute {})", code.0 as usize);
            return;
        }
        log::debug!("plugin {plugin}: opened {target}");
        raise_handler(&target);
    });
    if let Err(e) = spawned {
        log::warn!("no launch thread: {e}");
    }
}

/// Brings the app that handles a launched target to the front, from the shell (the process that
/// got the click, so Windows lets it set the foreground). One that is starting is waited for up
/// to 5 s. Stops as soon as the foreground changes otherwise (the user moved on, or the app came
/// forward by itself).
fn raise_handler(target: &str) {
    use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    use windows::core::{HSTRING, PWSTR};
    let Some(scheme) = scheme(target) else { return };
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: a buffer of the length we pass.
    let found = unsafe { AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, &HSTRING::from(scheme), PCWSTR::null(), Some(PWSTR(buffer.as_mut_ptr())), &mut len) };
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    if found.is_err() || end == 0 {
        log::debug!("launch {scheme}: no registered app to raise");
        return;
    }
    let exe = String::from_utf16_lossy(&buffer[..end]);
    // SAFETY: read-only query.
    let before = unsafe { GetForegroundWindow() }.0 as isize;
    let spawned = std::thread::Builder::new().name("raise".into()).stack_size(128 << 10).spawn(move || {
        for _ in 0..50 {
            // SAFETY: read-only query.
            let now = unsafe { GetForegroundWindow() }.0 as isize;
            if now != before {
                log::debug!("raise {exe}: the foreground changed; leaving it");
                return;
            }
            if let Some(window) = window_of(&exe) {
                let raised = bring_forward(window);
                log::debug!("raise {exe}: {}", if raised { "brought to the front" } else { "Windows refused the foreground" });
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        log::debug!("raise {exe}: no window within 5 s");
    });
    if let Err(e) = spawned {
        log::warn!("no raise thread: {e}");
    }
}

/// The topmost visible, unowned, titled top-level window of the process running `exe`.
fn window_of(exe: &str) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible};
    use windows::core::{BOOL, PWSTR};
    struct Search<'a> {
        exe: &'a str,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> BOOL {
        // SAFETY: `search` is the `Search` passed below, alive for the whole EnumWindows call;
        // the rest are read-only queries on a handle from EnumWindows, and the process handle is
        // closed before returning.
        unsafe {
            let search = &mut *(search.0 as *mut Search);
            if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) || GetWindowTextLengthW(hwnd) == 0 {
                return true.into();
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return true.into() };
            let mut path = [0u16; 1024];
            let mut len = path.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(process);
            if ok && String::from_utf16_lossy(&path[..len as usize]).eq_ignore_ascii_case(search.exe) {
                search.found = Some(hwnd);
                return false.into();
            }
            true.into()
        }
    }
    let mut search = Search { exe, found: None };
    // SAFETY: the callback only runs during this call, while `search` is alive.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut search as isize)) };
    search.found
}

/// Restores `window` if minimized and makes it the foreground window. If Windows refuses (the
/// right was lost to input queues being attached), joins the foreground thread's input for the
/// call, the standard way for the process that got the click.
fn bring_forward(window: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsGUIThread, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow};
    // SAFETY: window calls on a handle that may be stale (they then just fail); the thread input
    // is detached again before returning.
    unsafe {
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        if SetForegroundWindow(window).as_bool() {
            return true;
        }
        let _ = IsGUIThread(true); // AttachThreadInput needs this thread to have a queue
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        let attached = foreground != 0 && foreground != me && AttachThreadInput(me, foreground, true).as_bool();
        let _ = BringWindowToTop(window);
        let raised = SetForegroundWindow(window).as_bool();
        if attached {
            let _ = AttachThreadInput(me, foreground, false);
        }
        raised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_repeated_within_300_ms_goes_once() {
        let mut guard = RepeatGuard::default();
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        assert!(guard.allow("t3-2", "approve", None, t));
        assert!(!guard.allow("t3-2", "approve", None, ms(120)), "a repeat");
        assert!(!guard.allow("t3-2", "approve", None, ms(400)), "a chain of repeats goes as one");
        assert!(guard.allow("t3-2", "approve", None, ms(800)), "a click after the window");
        assert!(guard.allow("t3-2", "deny", None, ms(850)), "another action");
        assert!(guard.allow("t3-1", "deny", None, ms(860)), "another instance");
        assert!(guard.allow("t3-1", "deny", Some("x"), ms(870)), "another value");
    }

    #[test]
    fn helper_stats_reach_the_studio_snapshot_through_the_shell_bridge() {
        let (tx, _rx) = mpsc::channel();
        let mut plugins = Plugins::with_roots(Waker::new(tx, 0), Vec::new());
        let (frames, _requests) = mpsc::channel();
        let inbox = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from([
            wire::HelperMsg::Status { plugin: "test".into(), status: wire::Status::Live },
            wire::HelperMsg::Stats { plugin: "test".into(), memory: 1048576, cpu_us: 1234, events: 56 },
        ])));
        let child = Command::new("cmd").args(["/c", "exit", "0"]).creation_flags(CREATE_NO_WINDOW.0).spawn().unwrap();
        plugins.helper = Some(Helper { child, generation: 17, frames, inbox, idle_since: None, notification_plugins: Vec::new(), demand: Vec::new() });
        plugins.on_line(HELPER, 17, Some(String::new()), Instant::now());
        let stats = &plugins.studio.plugins["test"];
        assert_eq!((stats.memory, stats.cpu_us, stats.events), (1048576, 1234, 56));
        assert_eq!(stats.status, "Live");
        let mut helper = plugins.helper.take().unwrap(); helper.child.wait().unwrap();
    }

    #[test]
    fn plugin_errors_map_to_widget_codes() {
        let error = |code, name: Option<&str>| {
            widget_error(RpcError {
                code,
                message: "m".into(),
                name: name.map(str::to_owned),
            })
            .code
        };
        assert_eq!(error(-32601, None), ErrorCode::NotFound);
        assert_eq!(error(-32602, None), ErrorCode::InvalidParams);
        assert_eq!(error(-32000, None), ErrorCode::Internal);
        // error.data.code wins when it's one of ours.
        assert_eq!(error(-32000, Some("NotFound")), ErrorCode::NotFound);
        assert_eq!(error(-32601, Some("Bogus")), ErrorCode::NotFound);
    }

    #[test]
    fn undeclared_host_requests_are_refused() {
        let secrets = ["secrets".to_owned()];
        assert!(authorize(&secrets, &HostRequest::GetSecret { key: "token".into() }).is_ok());
        assert!(authorize(&secrets, &HostRequest::Launch { target: "calc".into() }).is_err());
        let set = HostRequest::SetSecret { key: "token".into(), value: None };
        assert!(authorize(&[], &set).is_err());
        let manifest = PluginManifest::parse(r#"{ "id": "x", "name": "X", "sdk": "1.0", "command": "x.exe" }"#, "x").unwrap();
        let line = serve(&manifest, &json!(5), HostRequest::Launch { target: "calc".into() });
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], 5);
        assert_eq!(reply["error"]["code"], sdk::PERMISSION_DENIED);
        assert_eq!(reply["error"]["data"]["code"], "PermissionDenied");
    }

    /// A real process that dies at once: the host keeps the face, says it's restarting, and
    /// starts it again after the backoff, never panicking.
    #[test]
    fn a_crashing_plugin_is_restarted_with_its_faces() {
        let root = std::env::temp_dir().join(format!("mypaper-crashy-{}", std::process::id()));
        let folder = root.join("crashy");
        std::fs::create_dir_all(&folder).unwrap();
        let manifest = r#"{ "id": "crashy", "name": "Crashy", "sdk": "1.0", "command": "cmd", "args": ["/c", "exit", "3"],
                            "faces": [{ "id": "now", "name": "Now", "size": { "default": [2, 2] } }] }"#;
        std::fs::write(folder.join(crate::manifest::PLUGIN_MANIFEST), manifest).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut plugins = Plugins::with_roots(Waker::new(tx, 0), vec![root.clone()]);
        let mut now = Instant::now();
        plugins.open_face("t3-1", "crashy", "now", SizeClass::Medium, Value::Null, now).unwrap();
        assert_eq!(plugins.face_status("t3-1"), Some(FaceStatus::Starting));
        let mut generations = std::collections::BTreeSet::new();
        for _ in 0..2 {
            loop {
                let Ok(Msg::Plugin { id, generation, line }) = rx.recv_timeout(Duration::from_secs(20)) else {
                    continue;
                };
                generations.insert(generation);
                let exited = line.is_none();
                plugins.on_line(&id, generation, line, now);
                if exited {
                    break;
                }
            }
            assert_eq!(plugins.face_status("t3-1"), Some(FaceStatus::Restarting));
            assert!(plugins.take_changed().contains(&"t3-1".to_owned()));
            now += Duration::from_secs(120); // past any backoff
            plugins.tick(now);
        }
        assert_eq!(generations.len(), 2, "started twice");
        plugins.close_face("t3-1");
        plugins.shutdown();
        let _ = std::fs::remove_dir_all(&root);
        if let Ok(data) = crate::files::user("plugin-data") {
            let _ = std::fs::remove_dir_all(data.join("crashy"));
        }
    }

    /// Launch targets with a URL scheme get their handler app raised; paths don't.
    #[test]
    fn launch_targets_name_their_scheme() {
        assert_eq!(super::scheme("t3code://threads/env/thread"), Some("t3code"));
        assert_eq!(super::scheme("https://example.com"), Some("https"));
        assert_eq!(super::scheme(r"C:\Program Files\app.exe"), None);
    }

    #[test]
    fn commands_resolve_in_the_folder_then_on_path() {
        let dir = crate::files::temp_dir("plugin-command");
        std::fs::write(dir.join("weather.exe"), b"").unwrap();
        assert_eq!(program(&dir, "weather.exe"), dir.join("weather.exe"));
        assert_eq!(program(&dir, "weather"), dir.join("weather.exe"));
        assert_eq!(program(&dir, "python"), std::path::PathBuf::from("python"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
