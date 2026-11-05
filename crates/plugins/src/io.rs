//! Websockets and named pipes (docs/plugins.md › "Websockets, pipes, notifications, system
//! events and images"). Host imports never block: `connect` and `open` hand back an id and queue
//! a [`Conn`]; the host loop spawns its thread, which reports
//! traffic to the loop as events. Each open connection owns its threads (a websocket a reader
//! and a writer, since WinHTTP's synchronous receive can't be woken; a pipe one overlapped-I/O
//! thread), never one per plugin, and they sleep in the kernel between messages.
//!
//! Backpressure both ways: `send`/`write` refuse once [`SEND_QUEUE`] bytes are queued, and a
//! connection stops reading while [`IN_FLIGHT`] of its events wait undelivered in the loop, so a
//! slow plugin slows the peer instead of growing the helper.
use std::collections::VecDeque;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, ERROR_MORE_DATA, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects};
use windows::core::{HSTRING, PCWSTR, w};

use crate::host::Input;
use crate::runtime::mypaper::plugin::pipes::PipeEvent;
use crate::runtime::mypaper::plugin::websocket::{Message, SocketEvent};

/// Open websockets per plugin, and open pipes per plugin (each).
pub const MAX_CONNECTIONS: usize = 8;
/// One inbound message, and one outbound queue.
pub const MAX_MESSAGE: usize = 4 << 20;
pub const SEND_QUEUE: usize = 4 << 20;
/// Undelivered inbound events before a connection stops reading.
pub const IN_FLIGHT: usize = 64;
const CHUNK: usize = 16 << 10;

#[derive(Default)]
struct Queue {
    out: VecDeque<Message>,
    bytes: usize,
    closing: bool,
    /// The connection's thread ended (peer closed, error, or our close finished).
    done: bool,
    undelivered: usize,
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    /// Pipes wait on kernel events, not the condvar.
    wake: Option<Event>,
}

impl Shared {
    fn poke(&self) {
        self.cv.notify_all();
        if let Some(wake) = &self.wake {
            // SAFETY: the event handle lives as long as `Shared`.
            let _ = unsafe { SetEvent(wake.0) };
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().expect("io queue")
    }
}

/// The plugin's side of one connection, kept in its store. Dropping it (the plugin closed it, was
/// unloaded or trapped) closes the connection.
pub struct Conn(Arc<Shared>);

fn size(message: &Message) -> usize {
    match message {
        Message::Text(t) => t.len(),
        Message::Binary(b) => b.len(),
    }
}

impl Conn {
    fn new(pipe: bool) -> Result<Self, String> {
        let wake = if pipe { Some(Event::new()?) } else { None };
        Ok(Self(Arc::new(Shared { queue: Mutex::default(), cv: Condvar::new(), wake })))
    }

    /// Queues an outbound message; refuses past [`SEND_QUEUE`] or after close.
    pub fn send(&self, message: Message) -> Result<(), String> {
        let mut queue = self.0.lock();
        if queue.closing || queue.done {
            return Err("closed".into());
        }
        let len = size(&message);
        if len > MAX_MESSAGE || queue.bytes + len > SEND_QUEUE {
            return Err("the send queue is full; wait for the peer".into());
        }
        queue.bytes += len;
        queue.out.push_back(message);
        drop(queue);
        self.0.poke();
        Ok(())
    }

    /// One inbound event reached the plugin: the connection may read again.
    pub fn delivered(&self) {
        let mut queue = self.0.lock();
        queue.undelivered = queue.undelivered.saturating_sub(1);
        let resume = queue.undelivered + 1 == IN_FLIGHT;
        drop(queue);
        if resume {
            self.0.poke();
        }
    }

    fn close(&self) {
        self.0.lock().closing = true;
        self.0.poke();
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.close();
    }
}

/// Counts an inbound event against the credit, blocking while [`IN_FLIGHT`] are undelivered
/// (websocket reader only; the pipe thread checks [`Shared::may_read`] instead).
fn take_credit(shared: &Shared) -> bool {
    let mut queue = shared.lock();
    while queue.undelivered >= IN_FLIGHT && !queue.closing {
        queue = shared.cv.wait(queue).expect("io queue");
    }
    queue.undelivered += 1;
    !queue.closing
}

// ---------------------------------------------------------------------------------------------
// Websockets (WinHTTP: Windows owns TLS, proxies and certificates)

pub struct SocketRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub protocols: Vec<String>,
}

/// Headers carry tokens: only the URL and the header names are printed.
impl std::fmt::Debug for SocketRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("SocketRequest").field("url", &self.url).field("headers", &names).field("protocols", &self.protocols).finish()
    }
}

/// Queues a websocket; [`start_socket`] spawns it once the call returns.
pub fn socket() -> Result<Conn, String> {
    Conn::new(false)
}

struct WsHandle {
    raw: *mut c_void,
    closed: AtomicBool,
}
// SAFETY: WinHTTP handles may be used from any thread; one receive and one send may be in flight
// at once, which is exactly the reader/writer split here. `close` runs once.
unsafe impl Send for WsHandle {}
unsafe impl Sync for WsHandle {}
impl WsHandle {
    fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            // SAFETY: closed exactly once (the swap); the other thread's pending call then fails.
            unsafe {
                let _ = WinHttpCloseHandle(self.raw);
            }
        }
    }
}
impl Drop for WsHandle {
    fn drop(&mut self) {
        self.close();
    }
}

struct Http(*mut c_void);
impl Drop for Http {
    fn drop(&mut self) {
        // SAFETY: each successful WinHTTP open is owned here and closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}
fn http(raw: *mut c_void) -> Result<Http, String> {
    if raw.is_null() { Err(windows::core::Error::from_thread().to_string()) } else { Ok(Http(raw)) }
}

/// One WinHTTP session for every websocket (a session per socket cost ~100 KiB each).
fn session() -> Result<*mut c_void, String> {
    static SESSION: Mutex<usize> = Mutex::new(0);
    let mut session = SESSION.lock().expect("winhttp session");
    if *session == 0 {
        // SAFETY: opened once and kept for the process (WinHTTP sessions are thread-safe).
        let raw = unsafe { WinHttpOpen(w!("mypaper-plugins/0.1"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) };
        if raw.is_null() {
            return Err(windows::core::Error::from_thread().to_string());
        }
        *session = raw as usize;
    }
    Ok(*session as *mut c_void)
}

/// The handshake: an upgrade GET with the plugin's headers and subprotocols.
fn handshake(request: &SocketRequest) -> Result<WsHandle, String> {
    let (scheme, rest) = request.url.split_once("://").ok_or("expected a ws(s) URL")?;
    let secure = match scheme {
        "wss" => true,
        "ws" => false,
        _ => return Err("expected a ws(s) URL".into()),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "bad port")?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    let path = rest.find('/').map_or("/".to_owned(), |i| rest[i..].to_owned());
    let mut headers: String = request.headers.iter().filter(|(n, v)| !n.contains(['\r', '\n', ':']) && !v.contains(['\r', '\n'])).map(|(n, v)| format!("{n}: {v}\r\n")).collect();
    if !request.protocols.is_empty() {
        headers.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", request.protocols.join(", ")));
    }
    let headers: Vec<u16> = headers.encode_utf16().collect();
    // SAFETY: handles are RAII values and every string outlives the call using it.
    unsafe {
        let connection = http(WinHttpConnect(session()?, &HSTRING::from(host), port, 0))?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let req = http(WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags))?;
        WinHttpSetTimeouts(req.0, 10_000, 10_000, 10_000, 15_000).map_err(|e| e.to_string())?;
        WinHttpSetOption(Some(req.0), WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, None).map_err(|e| e.to_string())?;
        WinHttpSendRequest(req.0, (!headers.is_empty()).then_some(headers.as_slice()), None, 0, 0, 0).map_err(|e| e.to_string())?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
        let (mut status, mut length) = (0u32, 4u32);
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&raw mut status).cast()), &mut length, std::ptr::null_mut()).map_err(|e| e.to_string())?;
        if status != 101 {
            return Err(format!("the server answered HTTP {status}, not an upgrade"));
        }
        let raw = WinHttpWebSocketCompleteUpgrade(req.0, None);
        if raw.is_null() {
            return Err(windows::core::Error::from_thread().to_string());
        }
        // Idle sockets stay open: no receive timeout after the handshake.
        let infinite = 0u32.to_le_bytes();
        let _ = WinHttpSetOption(Some(raw), WINHTTP_OPTION_RECEIVE_TIMEOUT, Some(&infinite));
        Ok(WsHandle { raw, closed: AtomicBool::new(false) })
    }
}

/// Runs a websocket: handshake, then a reader (this thread) and a writer.
pub fn start_socket(conn: &Conn, id: u64, plugin: String, request: SocketRequest, tx: Sender<Input>) {
    let shared = Arc::clone(&conn.0);
    let spawned = std::thread::Builder::new().name("ws".into()).stack_size(128 << 10).spawn(move || {
        let post = |event| tx.send(Input::Socket { plugin: plugin.clone(), event }).is_ok();
        let ws = match handshake(&request) {
            Ok(ws) => Arc::new(ws),
            Err(e) => {
                shared.lock().done = true;
                post(SocketEvent::Closed((id, Some(e))));
                return;
            }
        };
        post(SocketEvent::Opened(id));
        let (writer_ws, writer_shared) = (Arc::clone(&ws), Arc::clone(&shared));
        let writer = std::thread::Builder::new().name("ws send".into()).stack_size(64 << 10).spawn(move || write_socket(&writer_ws, &writer_shared));
        let reason = read_socket(&ws, &shared, id, &post);
        shared.lock().done = true;
        shared.poke();
        ws.close();
        if let Ok(writer) = writer {
            let _ = writer.join();
        }
        post(SocketEvent::Closed((id, reason)));
    });
    if let Err(e) = spawned {
        log::warn!("no websocket thread: {e}");
    }
}

/// Reads whole messages until the peer closes; returns the close reason.
fn read_socket(ws: &WsHandle, shared: &Shared, id: u64, post: &dyn Fn(SocketEvent) -> bool) -> Option<String> {
    let mut chunk = vec![0u8; CHUNK];
    let mut message = Vec::new();
    loop {
        let (mut read, mut kind) = (0u32, WINHTTP_WEB_SOCKET_BUFFER_TYPE(0));
        // SAFETY: `chunk` is writable for its length; the handle is open until `close`.
        let error = unsafe { WinHttpWebSocketReceive(ws.raw, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read, &mut kind) };
        if error != 0 {
            return (!shared.lock().closing).then(|| windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(error)).message());
        }
        message.extend_from_slice(&chunk[..read as usize]);
        if message.len() > MAX_MESSAGE {
            return Some("a message exceeded 4 MiB".into());
        }
        let event = match kind {
            WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => Message::Binary(std::mem::take(&mut message)),
            WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => Message::Text(String::from_utf8_lossy(&std::mem::take(&mut message)).into_owned()),
            WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => {
                let (mut status, mut reason, mut len) = (0u16, [0u8; 123], 0u32);
                // SAFETY: `reason` is writable for its length (the protocol's maximum).
                let _ = unsafe { WinHttpWebSocketQueryCloseStatus(ws.raw, &mut status, Some(reason.as_mut_ptr().cast()), reason.len() as u32, &mut len) };
                let text = String::from_utf8_lossy(&reason[..len as usize]).into_owned();
                let closing = shared.lock().closing;
                return (!closing && status != 1000).then(|| format!("closed by the server ({status}) {text}").trim_end().to_owned());
            }
            _ => continue, // a fragment: keep reading
        };
        if !take_credit(shared) || !post(SocketEvent::Message((id, event))) {
            return None;
        }
    }
}

/// Sends queued messages in order; on close, sends the close frame and gives the peer 3 s.
fn write_socket(ws: &WsHandle, shared: &Shared) {
    loop {
        let mut queue = shared.lock();
        while queue.out.is_empty() && !queue.closing && !queue.done {
            queue = shared.cv.wait(queue).expect("io queue");
        }
        if queue.done {
            return;
        }
        let Some(message) = queue.out.pop_front() else {
            drop(queue);
            // SAFETY: the handle is open; a 1000 close frame with no reason.
            unsafe {
                let _ = WinHttpWebSocketShutdown(ws.raw, 1000, None, 0);
            }
            let queue = shared.lock();
            let (queue, _) = shared.cv.wait_timeout_while(queue, Duration::from_secs(3), |q| !q.done).expect("io queue");
            drop(queue);
            ws.close(); // the peer never answered: abort the reader's receive
            return;
        };
        queue.bytes -= size(&message);
        drop(queue);
        let (kind, bytes) = match &message {
            Message::Text(t) => (WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, t.as_bytes()),
            Message::Binary(b) => (WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE, b.as_slice()),
        };
        // SAFETY: `bytes` is alive for this synchronous send.
        let error = unsafe { WinHttpWebSocketSend(ws.raw, kind, (!bytes.is_empty()).then_some(bytes)) };
        if error != 0 {
            ws.close();
            return;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Named pipes

struct Event(HANDLE);
// SAFETY: an event handle is a kernel object usable from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Event {
    fn new() -> Result<Self, String> {
        // SAFETY: an unnamed manual-reset event, closed on drop.
        unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map(Self).map_err(|e| e.to_string())
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Whether a `pipe:<name>` names a pipe and nothing else: `\`-separated parts, none empty, `.` or
/// `..` (Win32 would normalize `\\.\pipe\..\x` out of the pipe namespace), no `/` or `:`.
pub fn pipe_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 200 && !name.contains(['/', ':', '\0']) && name.split('\\').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn pipe_path(name: &str) -> HSTRING {
    HSTRING::from(format!(r"\\.\pipe\{name}"))
}

/// Queues a pipe; [`start_pipe`] spawns it once the call returns.
pub fn pipe() -> Result<Conn, String> {
    Conn::new(true)
}

struct Pipe(HANDLE);
impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once, after its I/O was cancelled and waited for.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Runs a pipe on one thread: overlapped reads and writes, waiting on their events and the wake.
pub fn start_pipe(conn: &Conn, id: u64, plugin: String, name: String, tx: Sender<Input>) {
    let shared = Arc::clone(&conn.0);
    let spawned = std::thread::Builder::new().name("pipe".into()).stack_size(128 << 10).spawn(move || {
        let post = |event| tx.send(Input::Pipe { plugin: plugin.clone(), event }).is_ok();
        if let Err(e) = run_pipe(&shared, id, &name, &post) {
            log::warn!("plugin {plugin}: pipe {name}: {e}");
        }
        shared.lock().done = true;
        post(PipeEvent::Closed(id));
    });
    if let Err(e) = spawned {
        log::warn!("no pipe thread: {e}");
    }
}

fn run_pipe(shared: &Shared, id: u64, name: &str, post: &dyn Fn(PipeEvent) -> bool) -> Result<(), String> {
    // SAFETY: a plain open of an existing pipe for overlapped I/O; closed by `Pipe`.
    let handle = unsafe { CreateFileW(&pipe_path(name), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, None) }.map_err(|e| e.message())?;
    let pipe = Pipe(handle);
    let (read_done, write_done) = (Event::new()?, Event::new()?);
    let wake = shared.wake.as_ref().expect("pipes have a wake event");
    let mut read_ov = OVERLAPPED { hEvent: read_done.0, ..Default::default() };
    let mut write_ov = OVERLAPPED { hEvent: write_done.0, ..Default::default() };
    let mut buffer = vec![0u8; CHUNK];
    let (mut reading, mut writing): (bool, Option<Vec<u8>>) = (false, None);
    let result = loop {
        // Start a read when there's credit, and a write when something is queued.
        let mut queue = shared.lock();
        if queue.closing {
            break Ok(());
        }
        if !reading && queue.undelivered < IN_FLIGHT {
            // SAFETY: `buffer` and `read_ov` stay alive and unmoved until the read completes or
            // is cancelled and waited for below.
            let started = unsafe { ReadFile(pipe.0, Some(&mut buffer), None, Some(&mut read_ov)) };
            match started {
                Ok(()) => reading = true,
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() || e.code() == ERROR_MORE_DATA.to_hresult() => reading = true,
                Err(e) => break Err(e.message()),
            }
        }
        if writing.is_none()
            && let Some(message) = queue.out.pop_front()
        {
            queue.bytes -= size(&message);
            let bytes = match message {
                Message::Binary(b) => b,
                Message::Text(t) => t.into_bytes(),
            };
            // SAFETY: the bytes live in `writing` until this write completes or is cancelled.
            let started = unsafe { WriteFile(pipe.0, Some(&bytes), None, Some(&mut write_ov)) };
            match started {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
                Err(e) => break Err(e.message()),
            }
            writing = Some(bytes);
        }
        drop(queue);
        // SAFETY: valid event handles owned above.
        let _ = unsafe { ResetEvent(wake.0) };
        let handles = [read_done.0, write_done.0, wake.0];
        let count = if writing.is_some() { 3 } else { 3 };
        // SAFETY: as above.
        let woke = unsafe { WaitForMultipleObjects(&handles[..count], false, INFINITE) };
        if woke.0 == WAIT_OBJECT_0.0 && reading {
            let mut n = 0u32;
            // SAFETY: the read on `read_ov` completed (its event is set).
            match unsafe { GetOverlappedResult(pipe.0, &read_ov, &mut n, false) } {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {}
                Err(_) => break Ok(()), // the server closed its end
            }
            reading = false;
            let _ = unsafe { ResetEvent(read_done.0) };
            shared.lock().undelivered += 1;
            if !post(PipeEvent::Data((id, buffer[..n as usize].to_vec()))) {
                break Ok(());
            }
        } else if woke.0 == WAIT_OBJECT_0.0 + 1 && writing.is_some() {
            let mut n = 0u32;
            // SAFETY: the write on `write_ov` completed (its event is set).
            let written = unsafe { GetOverlappedResult(pipe.0, &write_ov, &mut n, false) };
            let _ = unsafe { ResetEvent(write_done.0) };
            writing = None;
            if written.is_err() {
                break Ok(());
            }
        }
    };
    // Cancel what's in flight and wait for it, so the buffers outlive the kernel's use of them.
    // SAFETY: the handle is open; waiting with `true` blocks until each operation finished.
    unsafe {
        let _ = CancelIoEx(pipe.0, None);
        let mut n = 0u32;
        if reading {
            let _ = GetOverlappedResult(pipe.0, &read_ov, &mut n, true);
        }
        if writing.is_some() {
            let _ = GetOverlappedResult(pipe.0, &write_ov, &mut n, true);
        }
    }
    drop(pipe);
    result
}

// ---------------------------------------------------------------------------------------------
// HTTP over a pipe (Docker engine, Tailscale LocalAPI)

pub struct PipeReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// One blocking HTTP/1.1 request over `\\.\pipe\<name>` on a worker thread (`Connection: close`).
pub fn pipe_http(name: &str, method: &str, url: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Result<PipeReply, String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty()).unwrap_or("localhost");
    let target = rest.find('/').map_or("/", |i| &rest[i..]);
    let mut request = format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (n, v) in headers.iter().filter(|(n, v)| !n.contains(['\r', '\n', ':']) && !v.contains(['\r', '\n'])) {
        request.push_str(&format!("{n}: {v}\r\n"));
    }
    let body = body.unwrap_or_default();
    if !body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH") {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    let mut file = open_pipe_blocking(name)?;
    file.write_all(request.as_bytes()).and_then(|()| file.write_all(body)).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; CHUNK];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.raw_os_error() == Some(109) => break, // ERROR_BROKEN_PIPE: the server closed
            Err(e) => return Err(e.to_string()),
        };
        raw.extend_from_slice(&chunk[..n]);
        if raw.len() > MAX_MESSAGE {
            return Err("response exceeds 4 MiB".into());
        }
        if let Some(Ok(reply)) = parse_response(&raw, false) {
            return Ok(reply);
        }
    }
    parse_response(&raw, true).unwrap_or(Err("the pipe closed before a response".into()))
}

/// Opens a pipe for blocking I/O, waiting up to 2 s while every instance is busy.
fn open_pipe_blocking(name: &str) -> Result<std::fs::File, String> {
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    let path = format!(r"\\.\pipe\{name}");
    for _ in 0..2 {
        match std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => return Ok(file),
            Err(e) if e.raw_os_error() == Some(231) => {
                // ERROR_PIPE_BUSY
                // SAFETY: a null-terminated path for this call.
                let _ = unsafe { WaitNamedPipeW(&pipe_path(name), 2000) };
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("the pipe stayed busy".into())
}

/// `Some` once `raw` holds a whole response (`eof`: the body is whatever arrived).
fn parse_response(raw: &[u8], eof: bool) -> Option<Result<PipeReply, String>> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.split("\r\n");
    let status = lines.next()?.split(' ').nth(1).and_then(|s| s.parse().ok());
    let Some(status) = status else { return Some(Err("not an HTTP response".into())) };
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned())).collect();
    let header = |name: &str| headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let rest = &raw[end + 4..];
    let body = if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        dechunk(rest)?
    } else if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        if rest.len() < len {
            return None;
        }
        rest[..len].to_vec()
    } else if eof || status == 204 || status == 304 {
        rest.to_vec()
    } else {
        return None;
    };
    Some(Ok(PipeReply { status, headers, body }))
}

/// A whole chunked body, or `None` until its last chunk arrived.
fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = rest.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(String::from_utf8_lossy(&rest[..line]).split(';').next()?.trim(), 16).ok()?;
        rest = &rest[line + 2..];
        if size == 0 {
            return Some(body);
        }
        if rest.len() < size + 2 {
            return None;
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_stay_in_the_pipe_namespace() {
        assert!(pipe_name_ok("docker_engine"));
        assert!(pipe_name_ok(r"ProtectedPrefix\Administrators\Tailscale\tailscaled"));
        assert!(pipe_name_ok("discord-ipc-0"));
        for bad in ["", r"..\C:", r"a\..\b", r"\a", r"a\\b", "a/b", r"a\.", "c:x"] {
            assert!(!pipe_name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn responses_parse_with_length_chunks_or_eof() {
        let ok = |raw: &[u8], eof| parse_response(raw, eof).unwrap().unwrap();
        assert_eq!(ok(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi", false).body, b"hi");
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhi", false).is_none());
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n3\r\n th\r\n0\r\n\r\n";
        assert_eq!(ok(chunked, false).body, b"hi th");
        assert!(parse_response(&chunked[..chunked.len() - 5], false).is_none());
        let reply = ok(b"HTTP/1.0 404 Not Found\r\nX-A: b\r\n\r\nnope", true);
        assert_eq!((reply.status, reply.body.as_slice(), reply.headers[0].1.as_str()), (404, b"nope".as_slice(), "b"));
    }
}
