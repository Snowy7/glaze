use crate::{
    protocol::{Frame, Kind},
    server::module_path,
    windows,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    os::windows::{io::OwnedHandle, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Isolation {
    #[default]
    Shared,
    Plugin,
}

#[derive(Clone, Debug)]
pub struct PluginSpec {
    pub id: String,
    pub folder: PathBuf,
    pub native: Vec<String>,
    pub permissions: Vec<String>,
    pub isolation: Isolation,
}

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub call_timeout: Duration,
    pub startup_timeout: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub isolate_after: u32,
}
impl Default for HostConfig {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(5),
            startup_timeout: Duration::from_secs(5),
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(10),
            isolate_after: 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEvent {
    pub plugin: String,
    pub module: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
struct EventBuffer {
    vendor: VecDeque<NativeEvent>,
    restarts: VecDeque<NativeEvent>,
}
impl EventBuffer {
    fn pop(&mut self) -> Option<NativeEvent> {
        self.restarts
            .pop_front()
            .or_else(|| self.vendor.pop_front())
    }
}

/// Vendor traffic is bounded; lifecycle notifications cannot be lost to a flood.
#[derive(Default)]
struct EventQueue {
    buffer: Mutex<EventBuffer>,
    available: Condvar,
}
impl EventQueue {
    fn push(&self, event: NativeEvent, restart: bool) {
        let Ok(mut buffer) = self.buffer.lock() else {
            return;
        };
        if restart {
            buffer.restarts.push_back(event);
        } else if buffer.vendor.len() < 256 {
            buffer.vendor.push_back(event);
        } else {
            return;
        }
        self.available.notify_one();
    }
    fn try_pop(&self) -> Option<NativeEvent> {
        self.buffer.try_lock().ok()?.pop()
    }
    fn wait_pop(&self, timeout: Duration) -> Option<NativeEvent> {
        let buffer = self.buffer.lock().ok()?;
        let (mut buffer, _) = self
            .available
            .wait_timeout_while(buffer, timeout, |buffer| {
                buffer.vendor.is_empty() && buffer.restarts.is_empty()
            })
            .ok()?;
        buffer.pop()
    }
}

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub pid: u32,
    pub modules: Vec<(String, String)>,
    pub isolated: bool,
}

type Reply<T> = Sender<io::Result<T>>;
enum Request {
    Activate(PluginSpec, Reply<()>),
    Deactivate(String, Reply<()>),
    Call {
        plugin: String,
        module: String,
        method: String,
        bytes: Vec<u8>,
        reply: Reply<Vec<u8>>,
    },
    Hosts(Reply<Vec<HostInfo>>),
    Stop,
    TransportLost {
        key: String,
        pid: u32,
    },
}

/// Own one supervisor per `mypaper-plugins` process. Construction starts no child.
/// Methods block: invoke on a worker lane. Drop kills and reaps every child.
pub struct Supervisor {
    tx: Sender<Request>,
    events: Arc<EventQueue>,
    worker: Option<JoinHandle<()>>,
}
impl Supervisor {
    pub fn new(host_exe: PathBuf, config: HostConfig) -> Self {
        let config = HostConfig {
            call_timeout: config
                .call_timeout
                .clamp(Duration::from_millis(1), Duration::from_secs(60)),
            startup_timeout: config
                .startup_timeout
                .clamp(Duration::from_millis(1), Duration::from_secs(60)),
            initial_backoff: config
                .initial_backoff
                .clamp(Duration::from_millis(1), Duration::from_secs(60)),
            max_backoff: config
                .max_backoff
                .clamp(Duration::from_millis(1), Duration::from_secs(60)),
            isolate_after: config.isolate_after.max(1),
        };
        let (tx, rx) = mpsc::channel();
        let events = Arc::new(EventQueue::default());
        let worker_events = events.clone();
        let wake = tx.clone();
        let worker =
            thread::spawn(move || State::new(host_exe, config, worker_events, wake).run(rx));
        Self {
            tx,
            events,
            worker: Some(worker),
        }
    }
    fn request<T>(&self, request: impl FnOnce(Reply<T>) -> Request) -> io::Result<T> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(request(tx)).map_err(|_| stopped())?;
        rx.recv().map_err(|_| stopped())?
    }
    /// Register an active plugin and load only its explicitly declared DLLs.
    pub fn activate(&self, plugin: PluginSpec) -> io::Result<()> {
        self.request(|tx| Request::Activate(plugin, tx))
    }
    /// Recycle affected processes, unloading vendor code by process termination.
    pub fn deactivate(&self, plugin: &str) -> io::Result<()> {
        self.request(|tx| Request::Deactivate(plugin.to_owned(), tx))
    }
    pub fn call(
        &self,
        plugin: &str,
        module: &str,
        method: &str,
        bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        if bytes.len() > crate::protocol::MAX_FRAME - 2048
            || method.len() > 512
            || method.contains('\0')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native call exceeds protocol limits",
            ));
        }
        self.request(|reply| Request::Call {
            plugin: plugin.to_owned(),
            module: module.to_owned(),
            method: method.to_owned(),
            bytes: bytes.to_vec(),
            reply,
        })
    }
    /// Bounded vendor queue (256); lifecycle notifications have a separate queue.
    pub fn try_event(&self) -> Option<NativeEvent> {
        self.events.try_pop()
    }
    pub fn recv_event_timeout(&self, timeout: Duration) -> Option<NativeEvent> {
        self.events.wait_pop(timeout)
    }
    pub fn hosts(&self) -> io::Result<Vec<HostInfo>> {
        self.request(Request::Hosts)
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "native supervisor stopped")
}

struct Connection {
    child: Child,
    _job: OwnedHandle,
    tx: Option<Sender<Frame>>,
    rx: Receiver<io::Result<Frame>>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}
impl Connection {
    fn start(
        exe: &PathBuf,
        timeout: Duration,
        key: String,
        events: Arc<EventQueue>,
        wake: Sender<Request>,
    ) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!(
            "\\\\.\\pipe\\mypaper-native-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let pipe = windows::create_pipe(&name)?;
        let mut child = Command::new(exe)
            .args(["--pipe", &name])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let job = match windows::job(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        if let Err(error) = windows::connect_server(&pipe, child.id(), timeout) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let mut read_pipe = match pipe.try_clone() {
            Ok(file) => file,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let (read_tx, rx) = mpsc::sync_channel(128);
        let read_error = read_tx.clone();
        let pid = child.id();
        let lost_key = key.clone();
        let read_wake = wake.clone();
        let reader = thread::spawn(move || {
            loop {
                let frame = match Frame::read(&mut read_pipe) {
                    Ok(frame) if frame.kind == Kind::Event => {
                        forward(&events, frame);
                        continue;
                    }
                    frame => frame,
                };
                let done = frame.is_err();
                if read_tx.send(frame).is_err() || done {
                    let _ = read_wake.send(Request::TransportLost { key: lost_key, pid });
                    break;
                }
            }
        });
        let (tx, write_rx) = mpsc::channel::<Frame>();
        let writer = thread::spawn(move || {
            let mut pipe = pipe;
            while let Ok(frame) = write_rx.recv() {
                if let Err(error) = frame.write(&mut pipe) {
                    let _ = read_error.send(Err(error));
                    let _ = wake.send(Request::TransportLost { key, pid });
                    break;
                }
            }
        });
        Ok(Self {
            child,
            _job: job,
            tx: Some(tx),
            rx,
            reader: Some(reader),
            writer: Some(writer),
        })
    }
    fn rpc(&mut self, frame: Frame, timeout: Duration, events: &EventQueue) -> io::Result<Vec<u8>> {
        let request = frame.request;
        self.tx
            .as_ref()
            .ok_or_else(stopped)?
            .send(frame)
            .map_err(|_| stopped())?;
        let end = Instant::now() + timeout;
        loop {
            let frame = self
                .rx
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => {
                        io::Error::new(io::ErrorKind::TimedOut, "native host call timed out")
                    }
                    _ => stopped(),
                })??;
            if frame.kind == Kind::Event {
                forward(events, frame);
                continue;
            }
            if frame.request != request {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native response request mismatch",
                ));
            }
            return match frame.kind {
                Kind::Ok => Ok(frame.bytes),
                Kind::Error => Err(io::Error::other(
                    String::from_utf8_lossy(&frame.bytes).into_owned(),
                )),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid native response",
                )),
            };
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.tx.take();
        let (_, empty) = mpsc::channel();
        drop(std::mem::replace(&mut self.rx, empty));
        for thread in [self.reader.take(), self.writer.take()]
            .into_iter()
            .flatten()
        {
            let _ = thread.join();
        }
    }
}
fn forward(events: &EventQueue, frame: Frame) {
    events.push(
        NativeEvent {
            plugin: frame.plugin,
            module: frame.module,
            name: frame.name,
            bytes: frame.bytes,
        },
        false,
    );
}

struct Group {
    members: Vec<(String, String)>,
    connection: Option<Connection>,
    failures: u32,
    restart_at: Instant,
    started: bool,
}
struct State {
    exe: PathBuf,
    config: HostConfig,
    events: Arc<EventQueue>,
    plugins: BTreeMap<String, PluginSpec>,
    wake: Sender<Request>,
    groups: BTreeMap<String, Group>,
    isolated: BTreeSet<(String, String)>,
    crashes: BTreeMap<(String, String), u32>,
    sequence: u64,
}
impl State {
    fn new(
        exe: PathBuf,
        config: HostConfig,
        events: Arc<EventQueue>,
        wake: Sender<Request>,
    ) -> Self {
        Self {
            exe,
            config,
            events,
            wake,
            plugins: BTreeMap::new(),
            groups: BTreeMap::new(),
            isolated: BTreeSet::new(),
            crashes: BTreeMap::new(),
            sequence: 0,
        }
    }
    fn key(&self, plugin: &str, module: &str) -> String {
        if self
            .isolated
            .contains(&(plugin.to_owned(), module.to_owned()))
        {
            format!("module:{plugin}:{module}")
        } else if self
            .plugins
            .get(plugin)
            .is_some_and(|spec| spec.isolation == Isolation::Plugin)
        {
            format!("plugin:{plugin}")
        } else {
            "shared".to_owned()
        }
    }
    fn reconcile(&mut self) {
        let mut wanted = BTreeMap::<String, Vec<(String, String)>>::new();
        for spec in self.plugins.values() {
            for module in &spec.native {
                wanted
                    .entry(self.key(&spec.id, module))
                    .or_default()
                    .push((spec.id.clone(), module.clone()));
            }
        }
        self.groups.retain(|key, _| wanted.contains_key(key));
        for (key, members) in wanted {
            let group = self.groups.entry(key).or_insert_with(|| Group {
                members: Vec::new(),
                connection: None,
                failures: 0,
                restart_at: Instant::now(),
                started: false,
            });
            if group.members != members {
                group.connection.take();
                group.members = members;
                group.restart_at = Instant::now();
            }
        }
    }
    fn frame(&mut self, kind: Kind, plugin: &str, module: &str, name: &str, data: &[u8]) -> Frame {
        self.sequence += 1;
        let millis = self.config.call_timeout.as_millis().clamp(1, 60_000) as u32;
        let mut bytes = millis.to_le_bytes().to_vec();
        bytes.extend(data);
        Frame {
            kind,
            request: self.sequence,
            plugin: plugin.to_owned(),
            module: module.to_owned(),
            name: name.to_owned(),
            bytes,
        }
    }
    fn start(&mut self, key: &str) -> io::Result<()> {
        let Some(group) = self.groups.get(key) else {
            return Ok(());
        };
        if group.connection.is_some() {
            return Ok(());
        }
        if group.restart_at > Instant::now() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "native host is backing off",
            ));
        }
        let members = group.members.clone();
        let restarted = group.started;
        let mut connection = Connection::start(
            &self.exe,
            self.config.startup_timeout,
            key.to_owned(),
            self.events.clone(),
            self.wake.clone(),
        )?;
        for (plugin, module) in &members {
            let folder = self.plugins[plugin]
                .folder
                .to_str()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "plugin folder is not UTF-8")
                })?
                .as_bytes()
                .to_vec();
            let frame = self.frame(Kind::Load, plugin, module, "", &folder);
            if let Err(error) = connection.rpc(frame, self.config.call_timeout, &self.events) {
                drop(connection);
                self.failed(key, Some((plugin.clone(), module.clone())));
                return Err(error);
            }
        }
        if let Some(group) = self.groups.get_mut(key) {
            group.connection = Some(connection);
            group.started = true;
        }
        if restarted {
            for (plugin, module) in members {
                self.events.push(
                    NativeEvent {
                        plugin,
                        module,
                        name: "native-module-restarted".to_owned(),
                        bytes: Vec::new(),
                    },
                    true,
                );
            }
        }
        Ok(())
    }
    fn failed(&mut self, key: &str, culprit: Option<(String, String)>) {
        let Some(group) = self.groups.get_mut(key) else {
            return;
        };
        group.connection.take();
        group.started = true;
        group.failures = group.failures.saturating_add(1);
        let backoff = self
            .config
            .initial_backoff
            .saturating_mul(1_u32 << group.failures.saturating_sub(1).min(16))
            .min(self.config.max_backoff);
        group.restart_at = Instant::now() + backoff;
        let suspects = culprit
            .map(|key| vec![key])
            .unwrap_or_else(|| group.members.clone());
        let mut changed = false;
        for module in suspects {
            let failures = self.crashes.entry(module.clone()).or_default();
            *failures += 1;
            if *failures >= self.config.isolate_after.max(1) {
                changed |= self.isolated.insert(module);
            }
        }
        if changed {
            self.reconcile();
            for group in self
                .groups
                .values_mut()
                .filter(|group| group.connection.is_none())
            {
                group.started = true;
                group.restart_at = Instant::now() + backoff;
            }
        }
    }
    fn activate(&mut self, mut spec: PluginSpec) -> io::Result<()> {
        if spec.id.is_empty()
            || spec.id.len() > 128
            || spec.id.contains([':', '\0'])
            || self.plugins.contains_key(&spec.id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid or already active plugin id",
            ));
        }
        if !spec.native.is_empty()
            && !spec
                .permissions
                .iter()
                .any(|permission| permission == "native-code")
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native modules require native-code permission",
            ));
        }
        spec.folder = spec.folder.canonicalize()?;
        for module in &spec.native {
            if module.len() > 255 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "module name too long",
                ));
            }
            module_path(&spec.folder, module)?;
        }
        spec.native.sort();
        spec.native.dedup();
        let id = spec.id.clone();
        self.plugins.insert(id.clone(), spec);
        self.reconcile();
        let keys: Vec<_> = self
            .groups
            .iter()
            .filter(|(_, group)| group.members.iter().any(|(plugin, _)| plugin == &id))
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Err(error) = self.start(&key) {
                self.plugins.remove(&id);
                self.reconcile();
                return Err(error);
            }
        }
        Ok(())
    }
    fn tick(&mut self) {
        let keys: Vec<_> = self.groups.keys().cloned().collect();
        for key in keys {
            if self.groups.get(&key).is_some_and(|group| {
                group.connection.is_none() && group.restart_at <= Instant::now()
            }) && self.start(&key).is_err()
                && self
                    .groups
                    .get(&key)
                    .is_some_and(|group| group.restart_at <= Instant::now())
            {
                self.failed(&key, None);
            }
        }
    }
    fn run(mut self, rx: Receiver<Request>) {
        loop {
            let next_restart = self
                .groups
                .values()
                .filter(|group| group.connection.is_none())
                .map(|group| group.restart_at)
                .min();
            let request = if let Some(at) = next_restart {
                rx.recv_timeout(at.saturating_duration_since(Instant::now()))
            } else {
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            match request {
                Ok(Request::Activate(spec, reply)) => {
                    let result = self.activate(spec);
                    let _ = reply.send(result);
                }
                Ok(Request::Deactivate(plugin, reply)) => {
                    self.plugins.remove(&plugin);
                    self.reconcile();
                    self.tick();
                    let _ = reply.send(Ok(()));
                }
                Ok(Request::Call {
                    plugin,
                    module,
                    method,
                    bytes,
                    reply,
                }) => {
                    let result = if !self
                        .plugins
                        .get(&plugin)
                        .is_some_and(|spec| spec.native.contains(&module))
                    {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "native module is not declared by this active plugin",
                        ))
                    } else {
                        let key = self.key(&plugin, &module);
                        self.start(&key).and_then(|()| {
                            let frame = self.frame(Kind::Call, &plugin, &module, &method, &bytes);
                            let result = self
                                .groups
                                .get_mut(&key)
                                .and_then(|group| group.connection.as_mut())
                                .ok_or_else(stopped)?
                                .rpc(frame, self.config.call_timeout, &self.events);
                            // Module return errors preserve the host; transport errors recycle it.
                            if result
                                .as_ref()
                                .is_err_and(|error| error.kind() != io::ErrorKind::Other)
                            {
                                self.failed(&key, Some((plugin.clone(), module.clone())));
                            }
                            result
                        })
                    };
                    let _ = reply.send(result);
                }
                Ok(Request::Hosts(reply)) => {
                    let _ = reply.send(Ok(self
                        .groups
                        .iter()
                        .filter_map(|(key, group)| {
                            group.connection.as_ref().map(|connection| HostInfo {
                                pid: connection.child.id(),
                                modules: group.members.clone(),
                                isolated: key != "shared",
                            })
                        })
                        .collect()));
                }
                Ok(Request::TransportLost { key, pid }) => {
                    if self
                        .groups
                        .get(&key)
                        .and_then(|group| group.connection.as_ref())
                        .is_some_and(|connection| connection.child.id() == pid)
                    {
                        self.failed(&key, None);
                    }
                }
                Ok(Request::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            self.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_flood_is_bounded_and_cannot_hide_restart_notifications() {
        let events = EventQueue::default();
        let event = NativeEvent {
            plugin: "echo".into(),
            module: "echo.dll".into(),
            name: "data".into(),
            bytes: vec![1],
        };
        for _ in 0..300 {
            events.push(event.clone(), false);
        }
        events.push(
            NativeEvent {
                name: "native-module-restarted".into(),
                ..event
            },
            true,
        );
        assert_eq!(
            events.wait_pop(Duration::ZERO).unwrap().name,
            "native-module-restarted"
        );
        let mut count = 0;
        while events.try_pop().is_some() {
            count += 1;
        }
        assert_eq!(count, 256);
    }
}
