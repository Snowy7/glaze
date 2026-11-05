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
    next_generation: u64,
    helper: Option<Helper>,
    /// When to start the helper again after it died with faces placed, and the next delay.
    helper_retry: Option<(Instant, Duration)>,
    /// Component plugins' statuses, from the helper.
    component_status: HashMap<String, wire::Status>,
    /// Style patches from component plugins, by `(plugin, surface)` (docs/plugins.md, "Styling").
    styles: BTreeMap<(String, String), wire::StylePatch>,
}

impl Plugins {
    pub fn new(waker: Waker) -> Self {
        Self {
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
            next_generation: 0,
            helper: None,
            helper_retry: None,
            component_status: HashMap::new(),
            styles: BTreeMap::new(),
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
    fn to_helper(&mut self, message: wire::HostMsg, now: Instant) -> Result<(), WidgetError> {
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
        if self.held {
            let _ = frames.send(wire::HostMsg::Held(true));
        }
        for (instance, slot) in self.faces.iter().filter(|(_, s)| s.component.is_some()) {
            let _ = frames.send(component_open(instance, slot));
        }
        log::info!("plugin runtime started");
        self.helper = Some(Helper { child, generation, frames, inbox, idle_since: None });
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
            let placed = self.faces.iter().filter(|(_, s)| s.component.is_some()).map(|(i, _)| i.clone());
            self.changed.extend(placed.collect::<Vec<_>>());
            return Vec::new();
        }
        let messages: Vec<_> = helper.inbox.lock().expect("plugin inbox").drain(..).collect();
        for message in messages {
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
            }
        }
        Vec::new()
    }

    /// Stops an idle helper and starts a dead one again when faces need it.
    fn tick_helper(&mut self, now: Instant) {
        let placed = self.faces.values().any(|s| s.component.is_some());
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
        {
            if let Err(e) = self.start_helper() {
                log::warn!("restarting the plugin runtime: {e:#}");
                self.helper_retry = Some((now + delay, (delay * 2).min(Duration::from_secs(60))));
            }
        }
    }

    fn helper_deadline(&self) -> Option<Instant> {
        match &self.helper {
            Some(helper) => helper.idle_since.map(|t| t + IDLE_STOP),
            None if self.held => None,
            None => self.helper_retry.map(|(at, _)| at).filter(|_| self.faces.values().any(|s| s.component.is_some())),
        }
    }

    /// Looks plugins up in `roots` only (tests and harnesses).
    pub fn with_roots(waker: Waker, roots: Vec<std::path::PathBuf>) -> Self {
        Self {
            roots: Some(roots),
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
        if let Some(slot) = self.faces.get_mut(instance)
            && slot.plugin == plugin
            && slot.face == face
            && slot.component.is_some()
        {
            if slot.size != size || slot.settings != settings {
                slot.size = size;
                slot.settings = settings;
                let message = component_open(instance, slot);
                self.to_helper(message, now)?;
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
            return self.to_helper(message, now);
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

    /// A click on a view's `action` (or a submitted field's `value`).
    pub fn face_action(&mut self, instance: &str, action: &str, value: Option<String>) {
        let Some(slot) = self.faces.get(instance) else { return };
        if slot.component.is_some() {
            if let Some(helper) = &self.helper {
                let _ = helper.frames.send(wire::HostMsg::Action { instance: instance.into(), action: action.into(), value });
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
            .filter(|(id, _)| !self.held && self.listened(id))
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
        let note = rpc::notification(if held { "suspend" } else { "resume" });
        for process in self
            .procs
            .values()
            .filter(|p| matches!(p.state, State::Ready))
        {
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

#[cfg(test)]
mod tests {
    use super::*;

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
