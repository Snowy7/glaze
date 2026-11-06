//! Capability tests against the probe plugin (`examples/probe`, built here for wasm32-wasip2):
//! each capability is refused without its permission and round-trips with it, against local
//! servers (a websocket echo server, named-pipe servers), plus the native module path end to end.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use mypaper_plugin_api::SizeClass;
use mypaper_plugin_api::wire::{self, HelperMsg, HostMsg, SystemEvent, WindowChange};
use mypaper_plugin_sdk::view::Face;

use crate::host::{Host, Input};
use crate::runtime::Runtime;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `<target>` of this test binary (`<target>/debug/deps/x.exe`).
fn target_dir() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().parent().unwrap().to_owned()
}

fn probe_wasm() -> &'static PathBuf {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let target = target_dir().join("wasm-guests");
        let status = Command::new("cargo")
            .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
            .arg(repo().join("examples/probe/Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success(), "building examples/probe (rustup target add wasm32-wasip2)");
        target.join("wasm32-wasip2/release/probe.wasm")
    })
}

/// A host with probe plugins installed, and the loop's input channel to pump.
struct Rig {
    root: PathBuf,
    host: Host<Vec<u8>>,
    rx: Receiver<Input>,
    /// instance → the probe's latest lines
    lines: std::collections::HashMap<String, Vec<String>>,
}

impl Rig {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("mypaper-caps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rt = Runtime::new(root.join("cache"), root.join("data")).unwrap();
        let (tx, rx) = mpsc::channel();
        Self { host: Host::new(rt, Vec::new(), tx), rx, root, lines: Default::default() }
    }

    /// Installs a probe as `id` with `permissions` and opens its face as instance `id`.
    fn install(&mut self, id: &str, permissions: &[&str], native: &[&str]) -> &mut Self {
        let dir = self.root.join("plugins").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(probe_wasm(), dir.join("probe.wasm")).unwrap();
        let manifest = serde_json::json!({
            "id": id, "name": "Probe", "version": "0.1.0", "component": "probe.wasm", "api": "0.1",
            "faces": [{ "id": "main", "name": "Probe" }], "permissions": permissions, "native": native,
        });
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        for dll in native {
            std::fs::copy(native_echo(), dir.join(dll)).unwrap();
        }
        let message = HostMsg::Open { plugin: id.into(), dir: dir.to_string_lossy().into_owned(), instance: id.into(), face: "main".into(), size: SizeClass::Medium, settings: "{}".into() };
        self.host.on_host(message, Instant::now());
        self.wait(id, "open");
        self
    }

    fn act(&mut self, instance: &str, action: &str, value: &str) {
        self.host.on_host(HostMsg::Action { instance: instance.into(), action: action.into(), value: Some(value.into()) }, Instant::now());
        self.read();
    }

    /// Takes the frames written so far and keeps each instance's latest lines.
    fn read(&mut self) {
        let bytes = std::mem::take(self.host.out());
        let mut input = bytes.as_slice();
        while let Some(frame) = wire::read::<HelperMsg>(&mut input).unwrap() {
            if let HelperMsg::View { instance, face, .. } = frame {
                let face: Face = serde_json::from_slice(&face).unwrap();
                let json = serde_json::to_value(&face.body).unwrap();
                let lines = json["children"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str().map(str::to_owned)).collect();
                self.lines.insert(instance, lines);
            }
        }
    }

    fn has(&self, instance: &str, want: &str) -> bool {
        self.lines.get(instance).is_some_and(|lines| lines.iter().any(|l| l.contains(want)))
    }

    /// Pumps worker input into the host until `instance` shows a line containing `want`.
    fn wait(&mut self, instance: &str, want: &str) -> &str {
        let deadline = Instant::now() + Duration::from_secs(20);
        self.read();
        while !self.has(instance, want) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{instance} never showed {want:?}: {:?}", self.lines.get(instance));
            if let Ok(input) = self.rx.recv_timeout(left.min(Duration::from_millis(50))) {
                self.host.on_input(input, Instant::now());
            }
            self.host.tick(Instant::now());
            self.read();
        }
        self.lines[instance].iter().rev().find(|l| l.contains(want)).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.host.on_host(HostMsg::Shutdown, Instant::now());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// ---------------------------------------------------------------------------------------------
// A minimal websocket echo server (RFC 6455: handshake, masked client frames, unmasked replies)

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..20 => ((b & c) | (!b & d), 0x5A827999),
                20..40 => (b ^ c ^ d, 0x6ED9EBA1),
                40..60 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= chunk.len() { A[(n >> (18 - i * 6) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Accepts websocket clients and handshakes them; `echo` serves each on its own thread, else
/// they're only held open (for measuring the client side). The request heads are recorded.
fn ws_server(echo: bool) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&heads);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let key = head.lines().find_map(|l| l.strip_prefix("Sec-WebSocket-Key: ").or_else(|| l.strip_prefix("sec-websocket-key: "))).unwrap_or_default().trim().to_owned();
            let accept = base64(&sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()));
            let protocol = if head.contains("obswebsocket.json") { "Sec-WebSocket-Protocol: obswebsocket.json\r\n" } else { "" };
            write!(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{protocol}\r\n").unwrap();
            seen.lock().unwrap().push(head);
            if echo {
                std::thread::spawn(move || echo_frames(stream));
            } else {
                held.push(stream);
            }
        }
    });
    (port, heads)
}

/// Echoes text and binary frames; answers a close with a close.
fn echo_frames(mut stream: std::net::TcpStream) {
    loop {
        let mut head = [0u8; 2];
        if stream.read_exact(&mut head).is_err() {
            return;
        }
        let opcode = head[0] & 0x0f;
        let mut len = (head[1] & 0x7f) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).unwrap();
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext).unwrap();
            len = u64::from_be_bytes(ext) as usize;
        }
        let mut mask = [0u8; 4];
        stream.read_exact(&mut mask).unwrap();
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).unwrap();
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
        let mut reply = vec![0x80 | opcode];
        if opcode == 1 {
            payload.splice(0..0, b"echo:".iter().copied());
        }
        if payload.len() < 126 {
            reply.push(payload.len() as u8);
        } else {
            reply.push(126);
            reply.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        reply.extend_from_slice(&payload);
        if stream.write_all(&reply).is_err() || opcode == 8 {
            return;
        }
    }
}

#[test]
fn websockets_round_trip_with_headers_and_subprotocols() {
    let (port, heads) = ws_server(true);
    let origin = format!("ws://127.0.0.1:{port}");
    let mut rig = Rig::new("ws");
    rig.install("probe", &[&format!("network:{origin}")], &[]).install("bare", &[], &[]);
    rig.act("probe", "ws", &format!("{origin}/socket?x=1"));
    rig.wait("probe", "ws opened");
    let head = heads.lock().unwrap()[0].to_ascii_lowercase();
    assert!(head.starts_with("get /socket?x=1 http/1.1") && head.contains("authorization: bearer probe") && head.contains("obswebsocket.json"), "{head}");
    rig.act("probe", "ws-send", "hello");
    rig.wait("probe", "ws text echo:hello");
    rig.act("probe", "ws-binary", "abc");
    rig.wait("probe", "ws binary 3");
    rig.act("probe", "ws-close", "");
    assert_eq!(rig.wait("probe", "ws closed"), "ws closed None");

    // No permission, another origin, or a non-websocket scheme: refused at the call.
    rig.act("bare", "ws", &format!("{origin}/"));
    assert!(rig.wait("bare", "ws err").contains("Denied"));
    rig.act("probe", "ws", "ws://127.0.0.1:1/");
    assert!(rig.wait("probe", "ws err").contains("Denied"));
    rig.act("probe", "ws", &format!("http://127.0.0.1:{port}/"));
    assert!(rig.lines["probe"].last().unwrap().contains("Denied"));

    // The per-plugin cap: the ninth socket is refused while eight are open.
    for _ in 0..8 {
        rig.act("probe", "ws", &format!("{origin}/"));
    }
    rig.act("probe", "ws", &format!("{origin}/"));
    assert!(rig.lines["probe"].last().unwrap().contains("at most 8"), "{:?}", rig.lines["probe"].last());
}

#[test]
fn game_pause_keeps_socket_connected_and_coalesces_its_events() {
    let (port, heads) = ws_server(true);
    let origin = format!("ws://127.0.0.1:{port}");
    let mut rig = Rig::new("game-socket");
    rig.install("probe", &[&format!("network:{origin}")], &[]);
    rig.act("probe", "ws", &origin);
    rig.wait("probe", "ws opened");
    rig.act("probe", "ws-send", "first");
    rig.act("probe", "ws-send", "latest");
    rig.host.on_host(HostMsg::GameActivity(true), Instant::now());
    rig.read();
    let before = rig.lines["probe"].clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    for _ in 0..2 {
        let input = rig.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
        assert!(matches!(input, Input::Socket { event: crate::runtime::mypaper::plugin::websocket::SocketEvent::Message(_), .. }));
        rig.host.on_input(input, Instant::now());
    }
    rig.host.tick(Instant::now() + Duration::from_secs(600));
    assert!(rig.host.deadline().is_none());
    rig.read();
    assert_eq!(rig.lines["probe"], before);
    rig.host.on_host(HostMsg::GameActivity(false), Instant::now());
    rig.read();
    assert!(rig.has("probe", "ws text echo:latest"));
    assert!(!rig.has("probe", "ws text echo:first"));
    rig.act("probe", "ws-send", "after");
    rig.wait("probe", "ws text echo:after");
    assert_eq!(heads.lock().unwrap().len(), 1, "same connection after resume");
}

// ---------------------------------------------------------------------------------------------
// Named-pipe servers

mod pipe_server {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
    use windows::core::HSTRING;

    pub struct Server(pub HANDLE);
    // SAFETY: a pipe handle is usable from any thread.
    unsafe impl Send for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            // SAFETY: owned handle.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// A listening instance of `\\.\pipe\<name>` (clients may connect before `accept`).
    pub fn listen(name: &str) -> Server {
        // SAFETY: plain pipe creation; the handle is owned by `Server`.
        let handle = unsafe { CreateNamedPipeW(&HSTRING::from(format!(r"\\.\pipe\{name}")), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 255, 65536, 65536, 0, None) };
        assert!(!handle.is_invalid(), "CreateNamedPipeW");
        Server(handle)
    }

    impl Server {
        pub fn accept(&self) {
            // SAFETY: synchronous connect on our handle (already-connected is fine).
            let _ = unsafe { ConnectNamedPipe(self.0, None) };
        }
        pub fn read(&self) -> Option<Vec<u8>> {
            let mut buffer = vec![0u8; 65536];
            let mut n = 0u32;
            // SAFETY: synchronous read into an owned buffer.
            unsafe { ReadFile(self.0, Some(&mut buffer), Some(&mut n), None) }.ok()?;
            buffer.truncate(n as usize);
            (n > 0).then_some(buffer)
        }
        pub fn write(&self, bytes: &[u8]) {
            let mut n = 0u32;
            // SAFETY: synchronous write of borrowed bytes.
            unsafe {
                let _ = WriteFile(self.0, Some(bytes), Some(&mut n), None);
                let _ = FlushFileBuffers(self.0);
            }
        }
    }
}

#[test]
fn pipes_round_trip_and_refuse_undeclared_names() {
    let name = format!("mypaper-probe-{}", std::process::id());
    let server = pipe_server::listen(&name);
    std::thread::spawn(move || {
        server.accept();
        while let Some(bytes) = server.read() {
            server.write(&[b"echo:".as_slice(), &bytes].concat());
        }
    });
    let mut rig = Rig::new("pipe");
    rig.install("probe", &[&format!("pipe:{name}")], &[]);
    rig.act("probe", "pipe", &name);
    rig.act("probe", "pipe-write", "hi");
    rig.wait("probe", "pipe data echo:hi");
    rig.act("probe", "pipe-close", "");
    rig.act("probe", "pipe", "other-pipe");
    assert!(rig.wait("probe", "pipe err").contains("Denied"));
    rig.act("probe", "pipe-http", "other-pipe|http://x/");
    assert!(rig.wait("probe", "pipe-http err").contains("Denied"));
}

#[test]
fn http_over_a_pipe_reads_chunked_replies() {
    let name = format!("mypaper-probe-http-{}", std::process::id());
    let server = pipe_server::listen(&name);
    let seen = Arc::new(Mutex::new(String::new()));
    let head = Arc::clone(&seen);
    std::thread::spawn(move || {
        server.accept();
        *head.lock().unwrap() = String::from_utf8_lossy(&server.read().unwrap()).into_owned();
        server.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"ok\r\n4\r\n\":1}\r\n0\r\n\r\n");
    });
    let mut rig = Rig::new("pipehttp");
    rig.install("probe", &[&format!("pipe:{name}")], &[]);
    rig.act("probe", "pipe-http", &format!("{name}|http://docker/v1.43/version"));
    assert_eq!(rig.wait("probe", "http 200"), r#"http 200 {"ok":1}"#);
    let head = seen.lock().unwrap().clone();
    assert!(head.starts_with("GET /v1.43/version HTTP/1.1\r\nHost: docker\r\n"), "{head}");
}

#[test]
fn notifications_foreground_and_windows_need_their_permissions() {
    let mut rig = Rig::new("perms");
    rig.install("bare", &[], &[]).install("probe", &["windows"], &[]);
    rig.act("bare", "notify", "hi");
    assert!(rig.wait("bare", "notify").contains("Denied"));
    rig.act("bare", "foreground", "");
    assert!(rig.wait("bare", "foreground").contains("Denied"));
    rig.act("bare", "subscribe", "windows");
    assert!(rig.wait("bare", "subscribe").contains("Denied"));
    rig.act("probe", "foreground", "");
    assert!(rig.wait("probe", "foreground").contains("Ok("));
}

#[test]
fn system_events_reach_only_subscribers_and_moves_coalesce() {
    let mut rig = Rig::new("system");
    rig.install("probe", &["windows"], &[]).install("bare", &[], &[]).install("quiet", &["windows"], &[]);
    rig.act("probe", "subscribe", "windows,power");
    rig.act("bare", "subscribe", "power,media");
    let now = Instant::now();
    let moved = |id: u64, title: &str| HostMsg::System(SystemEvent::Window { change: WindowChange::Moved, id, title: title.into(), process: "x.exe".into() });
    for i in 0..50 {
        rig.host.on_host(moved(7, &format!("drag {i}")), now);
    }
    rig.host.on_host(moved(8, "other"), now);
    rig.host.on_host(HostMsg::System(SystemEvent::Power), now);
    rig.host.on_host(HostMsg::System(SystemEvent::Media(r#"{"title":"song"}"#.into())), now);
    rig.read();
    assert!(rig.has("probe", "power") && rig.has("bare", "power") && !rig.has("quiet", "power"));
    assert!(rig.has("bare", "media") && !rig.has("probe", "media"));
    assert!(!rig.has("probe", "window"), "moves wait for the frame");
    assert!(rig.host.deadline().is_some_and(|d| d <= now + crate::host::MOVE_FRAME));
    rig.host.tick(now + crate::host::MOVE_FRAME);
    rig.read();
    let windows: Vec<&String> = rig.lines["probe"].iter().filter(|l| l.starts_with("window")).collect();
    assert_eq!(windows, ["window WindowChange::Moved 7 drag 49", "window WindowChange::Moved 8 other"]);
    assert!(!rig.has("quiet", "window") && !rig.has("bare", "window"));
}

#[test]
fn images_go_to_named_shared_memory() {
    use windows::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile};
    let mut rig = Rig::new("image");
    rig.install("probe", &[], &[]);
    rig.act("probe", "image", "");
    let line = rig.wait("probe", "image Ok").to_owned();
    let id: u32 = line.trim_start_matches("image Ok(").trim_end_matches(')').parse().unwrap();
    let name = windows::core::HSTRING::from(crate::images::name(std::process::id(), id));
    // SAFETY: opens the probe's mapping read-only and reads within its 16 + 16 bytes.
    unsafe {
        let section = OpenFileMappingW(FILE_MAP_READ.0, false, &name).unwrap();
        let view = MapViewOfFile(section, FILE_MAP_READ, 0, 0, 32);
        let bytes = std::slice::from_raw_parts(view.Value.cast::<u8>(), 32);
        assert_eq!(&bytes[..4], b"MPI1");
        assert_eq!([bytes[4], bytes[8], bytes[12]], [2, 2, 8]);
        assert!(bytes[16..].iter().all(|&b| b == 255));
        let _ = UnmapViewOfFile(view);
        let _ = windows::Win32::Foundation::CloseHandle(section);
    }
    rig.act("probe", "image-bad", "");
    assert!(rig.lines["probe"].last().unwrap().contains("Invalid"));
}

#[test]
fn notification_permission_demand_and_visibility() {
    let mut rig = Rig::new("notification-demand");
    rig.install("probe", &["notifications"], &[]).install("bare", &[], &[]).install("quiet", &["notifications"], &[]);
    rig.act("bare", "subscribe", "notifications");
    assert!(rig.wait("bare", "subscribe").contains("Denied"));
    rig.host.on_host(HostMsg::Action {instance:"probe".into(),action:"subscribe".into(),value:Some("notifications".into())},Instant::now());
    let bytes = std::mem::take(rig.host.out());
    let mut input = bytes.as_slice();
    let mut demand = Vec::new();
    while let Some(frame) = wire::read::<HelperMsg>(&mut input).unwrap() {
        if let HelperMsg::Notifications {plugins} = frame {demand=plugins;}
    }
    assert_eq!(demand,vec!["probe"]);
    rig.host.on_host(HostMsg::System(SystemEvent::Notification(r#"{"kind":"added","id":7}"#.into())),Instant::now());
    rig.read();
    assert!(rig.has("probe","notification"));
    assert!(!rig.has("bare","notification {") && !rig.has("quiet","notification {"));
    rig.host.on_host(HostMsg::Held(true),Instant::now());
    let bytes = std::mem::take(rig.host.out());
    let mut input = bytes.as_slice();
    let mut released=false;
    while let Some(frame) = wire::read::<HelperMsg>(&mut input).unwrap() {
        if let HelperMsg::Notifications {plugins} = frame {released=plugins.is_empty();}
    }
    assert!(released,"held plugins release notification demand");
    rig.host.on_host(HostMsg::Held(false),Instant::now());
    rig.read();
    rig.host.on_host(HostMsg::Close {instance:"probe".into()},Instant::now());
    let bytes = std::mem::take(rig.host.out());
    let mut input = bytes.as_slice();
    let mut released=false;
    while let Some(frame) = wire::read::<HelperMsg>(&mut input).unwrap() {
        if let HelperMsg::Notifications {plugins} = frame {released=plugins.is_empty();}
    }
    assert!(released,"last closed face releases demand immediately");
}

/// The native-echo example as a DLL (compiled directly with rustc, like the native-host suite).
fn native_echo() -> &'static PathBuf {
    static DLL: OnceLock<PathBuf> = OnceLock::new();
    DLL.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("mypaper-caps-echo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dll = dir.join("native_echo.dll");
        let status = Command::new("rustc")
            .args(["--edition=2024", "--crate-type=cdylib", "-Cpanic=abort", "-Copt-level=2", "-Clinker=rust-lld.exe"])
            .arg(repo().join("examples/native-echo/src/lib.rs"))
            .arg("-o")
            .arg(&dll)
            .status()
            .unwrap();
        assert!(status.success());
        dll
    })
}

#[test]
fn native_modules_round_trip_end_to_end() {
    let status = Command::new("cargo").args(["build", "-q", "-p", "mypaper-native-host", "--target-dir"]).arg(target_dir()).current_dir(repo()).status().unwrap();
    assert!(status.success(), "building mypaper-native-host");
    // SAFETY: set before any lane starts; only the native lane reads it.
    unsafe { std::env::set_var("MYPAPER_NATIVE_HOST", target_dir().join("debug/mypaper-native-host.exe")) };
    let mut rig = Rig::new("native");
    rig.install("bare", &[], &[]).install("probe", &["native-code"], &["native_echo.dll"]);
    rig.act("bare", "native", "echo");
    assert!(rig.wait("bare", "native err").contains("Denied"));
    rig.act("probe", "native", "echo");
    rig.wait("probe", "native ok hi");
    rig.act("probe", "native", "background");
    rig.wait("probe", "native event background hi");
}

/// Prints the helper's private memory per open websocket and per open pipe (client side only:
/// the servers here hold connections without threads). `cargo test -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure_memory_per_connection() {
    fn private() -> usize {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        // SAFETY: a correctly sized counters struct for this process.
        unsafe {
            let _ = GetProcessMemoryInfo(windows::Win32::System::Threading::GetCurrentProcess(), (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(), size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32);
        }
        counters.PrivateUsage
    }
    const N: usize = 8;
    let (port, heads) = ws_server(false);
    let name = format!("mypaper-probe-measure-{}", std::process::id());
    let servers: Vec<_> = (0..N).map(|_| pipe_server::listen(&name)).collect();
    let mut rig = Rig::new("measure");
    rig.install("probe", &[&format!("network:ws://127.0.0.1:{port}"), &format!("pipe:{name}")], &[]);
    // Warm one of each so one-time costs (WinHTTP, thread pools) aren't counted per connection.
    rig.act("probe", "ws", &format!("ws://127.0.0.1:{port}/"));
    rig.wait("probe", "ws opened");
    rig.act("probe", "ws-close", "");
    rig.wait("probe", "ws closed");
    std::thread::sleep(Duration::from_millis(300));
    let before = private();
    for _ in 0..N {
        rig.act("probe", "ws", &format!("ws://127.0.0.1:{port}/"));
    }
    while heads.lock().unwrap().len() < N + 1 {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let sockets = private();
    for _ in 0..N {
        rig.act("probe", "pipe", &name);
    }
    std::thread::sleep(Duration::from_millis(300));
    let pipes = private();
    println!("per websocket: {} KiB private; per pipe: {} KiB private", (sockets - before) / N / 1024, (pipes.saturating_sub(sockets)) / N / 1024);
    drop(servers);
}
