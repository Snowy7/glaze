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

pub struct Plugins {
    procs: HashMap<String, Process>,
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
}

impl Plugins {
    pub fn new(waker: Waker) -> Self {
        Self {
            procs: HashMap::new(),
            subs: BTreeMap::new(),
            restarts: HashMap::new(),
            dying: Vec::new(),
            job: None,
            waker,
            held: false,
            next_generation: 0,
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
        self.subs.values().any(|(p, _)| p == plugin)
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
        let (manifest, dir) = crate::registry::find_plugin(id)
            .ok_or_else(|| unavailable(format!("plugin {id:?} isn't installed")))?;
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

        let params =
            json!({ "protocol": 1, "plugin": id, "dataDir": crate::files::plain(&data_dir) });
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
            return process.fail_all("the plugin exited");
        };
        match rpc::parse(&line) {
            Incoming::Response {
                id: rpc::INIT_ID,
                outcome,
            } if matches!(process.state, State::Starting { .. }) => match outcome {
                Ok(result) if result.get("protocol").and_then(Value::as_u64) == Some(1) => {
                    process.state = State::Ready;
                    self.restarts.remove(id);
                    if self.held {
                        process.write(rpc::notification("suspend"));
                    }
                    for (target, method, params) in std::mem::take(&mut process.queued) {
                        process.send(target, &method, params.as_ref(), now);
                    }
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
    }

    /// Timeouts, idle stops, kills after the grace period, and restarts.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
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
        per_process.chain(restarts).chain(dying).flatten().min()
    }

    /// Sends `suspend` / `resume` to running plugins.
    pub fn set_held(&mut self, held: bool) {
        if self.held == held {
            return;
        }
        self.held = held;
        let note = rpc::notification(if held { "suspend" } else { "resume" });
        for process in self
            .procs
            .values()
            .filter(|p| matches!(p.state, State::Ready))
        {
            process.write(note.clone());
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
    fn commands_resolve_in_the_folder_then_on_path() {
        let dir = crate::files::temp_dir("plugin-command");
        std::fs::write(dir.join("weather.exe"), b"").unwrap();
        assert_eq!(program(&dir, "weather.exe"), dir.join("weather.exe"));
        assert_eq!(program(&dir, "weather"), dir.join("weather.exe"));
        assert_eq!(program(&dir, "python"), std::path::PathBuf::from("python"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
