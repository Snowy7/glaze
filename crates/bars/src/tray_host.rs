//! The tray host (docs/bars.md, "Tray host"): how the bars get the real notification-area icons
//! without code injection, the ManagedShell / RetroBar way.
//!
//! Apps add their icons with `Shell_NotifyIcon`, which sends `WM_COPYDATA` to the first top-level
//! window of class `Shell_TrayWnd` (`FindWindow` walks the Z-order). So, on its own thread, the
//! host:
//!
//! 1. creates a hidden, topmost `Shell_TrayWnd` (with a `TrayNotifyWnd` child), kept above
//!    Explorer's, sized like the bar that shows the tray;
//! 2. broadcasts `TaskbarCreated`, so every running app adds its icons again, now to us;
//! 3. keeps each icon (owner window, uID or GUID, callback message, version, tooltip, a copy of
//!    the `HICON` as a PNG `data:` URL) and **forwards every message to Explorer's real tray**, so
//!    Explorer stays correct (balloons become its toasts; nothing is lost if we go away);
//! 4. forwards everything else meant for the taskbar: app-bar messages (re-homing their shared
//!    memory for Explorer, as `SHAppBarMessage` tags it with the receiver's process), `WM_COMMAND`,
//!    the Start command and the `WM_USER` range;
//! 5. sends clicks back to an icon's owner as its callback message (`tray_data::click_messages`),
//!    after `AllowSetForegroundWindow`, so its menu or window comes to the front.
//!
//! It runs while some bar is configured with a tray module and the native taskbar is replaced
//! (`taskbar.mode: hide`, or `tray.host`), whether or not that bar is on screen right now (a
//! fullscreen game hiding the bars mustn't make every app re-add its icons). Stopping destroys
//! the window and broadcasts `TaskbarCreated` again, so apps re-register with Explorer. When
//! Explorer's process ends, or its tray window goes, the host steps aside at once (a second
//! `Shell_TrayWnd` during Explorer's start keeps it from initializing its taskbar: Zebar #301)
//! and comes back a few seconds after Explorer's tray is there again: on Explorer's own
//! `TaskbarCreated`, or the next periodic look for it.
//!
//! Only `explorer.exe`'s tray is ever forwarded to, and nested forwards are capped, so two hosts
//! (ours and another shell's) can't bounce a message between them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, STREAM_SEEK_SET,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_BINARY, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentThreadId, GetProcessTimes, INFINITE, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Shell::{SHAllocShared, SHFreeShared, SHLockShared, SHUnlockShared};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CHILDID_SELF, CS_DBLCLKS, CopyIcon, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyWindow, DispatchMessageW, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, FindWindowW, GWL_STYLE, GetIconInfo,
    GetWindowLongPtrW, GetWindowThreadProcessId, HICON, HWND_BROADCAST, HWND_TOPMOST, ICONINFO,
    ISMEX_NOTIFY, ISMEX_SEND, InSendMessageEx, IsWindow, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, OBJID_WINDOW, PM_REMOVE, PeekMessageW, PostMessageW,
    PostThreadMessageW, QS_ALLINPUT, RegisterClassExW, RegisterWindowMessageW, ReplyMessage,
    SC_TASKLIST, SMTO_ABORTIFHUNG, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SendMessageTimeoutW, SendNotifyMessageW, SetWindowLongPtrW, SetWindowPos,
    TranslateMessage, UnregisterClassW, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_COPYDATA, WM_NCDESTROY, WM_QUIT, WM_SYSCOMMAND, WM_USER, WM_WINDOWPOSCHANGED,
    WM_WINDOWPOSCHANGING, WNDCLASSEXW, WNDPROC, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::tray::{reg_dword, reg_string, resolve_known_folder};
use crate::tray_data::{
    self, ABM_GETTASKBARPOS, ABM_QUERYPOS, ABM_SETPOS, APPBARDATA_SIZE, COPYDATA_APPBAR,
    COPYDATA_ICON_RECT, COPYDATA_NOTIFY, Click, ICON_OFFSET, Icons, Owner, Setting, TrayItem,
};
use crate::wake::{Msg, Waker};

const TRAY_CLASS: PCWSTR = w!("Shell_TrayWnd");
const NOTIFY_CLASS: PCWSTR = w!("TrayNotifyWnd");
/// Posted to the host's thread: commands are waiting in its channel.
const WM_HOST_COMMAND: u32 = WM_APP + 40;
/// How long a forward to Explorer may take (a hung Explorer must not hang the apps, or us).
const FORWARD_TIMEOUT_MS: u32 = 2_000;
/// The same while the host stops: what's still queued gets a short chance.
const SHUTDOWN_TIMEOUT_MS: u32 = 200;
/// Forwards nested inside forwards (Explorer's answer can make an app call us again). A deeper
/// chain is a window bouncing messages back to us; it gets 0.
const MAX_DEPTH: u32 = 4;
/// Icon changes come in bursts (every app re-adding after `TaskbarCreated`): one report each.
const REPORT_SETTLE: Duration = Duration::from_millis(80);
/// Animated icons change many times a second: at most one report this often.
const REPORT_GAP: Duration = Duration::from_secs(1);
/// Explorer rewrites `NotifyIconSettings` as icons change: re-read it once the writes settle,
/// and at most this often.
const SETTINGS_SETTLE: Duration = Duration::from_secs(2);
const SETTINGS_GAP: Duration = Duration::from_secs(5);
/// After Explorer (re)starts, let it build its taskbar before a second `Shell_TrayWnd` appears.
const EXPLORER_SETTLE: Duration = Duration::from_secs(4);
/// A periodic check (one `FindWindow`): while hosting, that ours is still the first tray
/// (Explorer can raise its own without an event we hear); while not, whether Explorer's tray is
/// back (its `TaskbarCreated` can be missed).
const CHECK_EVERY: Duration = Duration::from_secs(3);

/// When the host last broadcast `TaskbarCreated` (`GetTickCount64`), so the bars don't take their
/// own broadcast for an Explorer restart.
static OWN_BROADCAST: AtomicU64 = AtomicU64::new(0);

/// Did the host broadcast `TaskbarCreated` in the last couple of seconds?
pub fn own_broadcast() -> bool {
    let at = OWN_BROADCAST.load(Ordering::Relaxed);
    // SAFETY: no preconditions.
    at != 0 && unsafe { GetTickCount64() }.saturating_sub(at) < 2_000
}

fn taskbar_created() -> u32 {
    // SAFETY: a static string.
    unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
}

fn broadcast_taskbar_created() {
    // SAFETY: no preconditions.
    OWN_BROADCAST.store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    // SAFETY: a notification broadcast; it doesn't wait for anyone.
    let _ = unsafe { SendNotifyMessageW(HWND_BROADCAST, taskbar_created(), WPARAM(0), LPARAM(0)) };
}

/// Where the tray shows: the band of the bar with the tray module (screen pixels) and its edge
/// (`ABE_*`). Apps place their popups from it (`ABM_GETTASKBARPOS`, the tray window's rect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub rect: [i32; 4],
    pub edge: u32,
}

enum Command {
    Click {
        id: String,
        click: Click,
        cursor: (i32, i32),
        /// The icon's rect on screen, for `Shell_NotifyIconGetRect`.
        anchor: Option<[i32; 4]>,
    },
    Place(Option<Band>),
    ExplorerRestarted,
    /// An exe's description, read off the host's thread (`describe`).
    Named {
        exe: String,
        name: String,
    },
}

struct Worker {
    thread: u32,
    commands: Sender<Command>,
    handle: JoinHandle<()>,
}

/// The main thread's handle on the host.
pub struct TrayHost {
    worker: Option<Worker>,
    waker: Waker,
    band: Option<Band>,
}

impl TrayHost {
    pub fn new(waker: Waker) -> Self {
        Self {
            worker: None,
            waker,
            band: None,
        }
    }

    pub fn active(&self) -> bool {
        self.worker.is_some()
    }

    pub fn set_active(&mut self, on: bool) {
        match (on, self.worker.take()) {
            (true, None) => {
                let (tx, rx) = mpsc::channel();
                let (ready_tx, ready_rx) = mpsc::channel();
                let (waker, band, own) = (self.waker.clone(), self.band, tx.clone());
                let spawned =
                    std::thread::Builder::new()
                        .name("tray-host".into())
                        .spawn(move || {
                            // Before saying it's ready: a quit posted from now on is kept.
                            crate::wake::make_queue();
                            // SAFETY: no preconditions.
                            let _ = ready_tx.send(unsafe { GetCurrentThreadId() });
                            run(&rx, own, waker, band);
                        });
                match spawned {
                    Ok(handle) => match ready_rx.recv() {
                        Ok(thread) => {
                            log::info!("tray: hosting the notification area");
                            self.worker = Some(Worker {
                                thread,
                                commands: tx,
                                handle,
                            });
                        }
                        Err(_) => log::warn!("tray: the host thread didn't start"),
                    },
                    Err(e) => log::warn!("tray: no host thread: {e}"),
                }
            }
            (false, Some(worker)) => {
                crate::wake::quit_thread(worker.thread, worker.handle);
                log::info!("tray: stopped hosting; apps re-register with Explorer");
            }
            (_, worker) => self.worker = worker,
        }
    }

    fn send(&self, command: Command) {
        if let Some(worker) = &self.worker
            && worker.commands.send(command).is_ok()
        {
            // SAFETY: posting to a thread we started.
            let _ =
                unsafe { PostThreadMessageW(worker.thread, WM_HOST_COMMAND, WPARAM(0), LPARAM(0)) };
        }
    }

    pub fn click(&self, id: String, click: Click, cursor: (i32, i32), anchor: Option<[i32; 4]>) {
        self.send(Command::Click {
            id,
            click,
            cursor,
            anchor,
        });
    }

    pub fn place(&mut self, band: Option<Band>) {
        if band != self.band {
            self.band = band;
            self.send(Command::Place(band));
        }
    }

    /// Explorer broadcast `TaskbarCreated` (it restarted): come back once it settled.
    pub fn explorer_restarted(&self) {
        self.send(Command::ExplorerRestarted);
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

// --- The host thread ------------------------------------------------------------------------

/// Explorer's tray, as the host forwards to it.
#[derive(Clone, Copy)]
struct Explorer {
    tray: HWND,
    pid: u32,
}

struct State {
    waker: Waker,
    /// The host's own channel and thread, for helpers that report back (`describe`).
    commands: Sender<Command>,
    thread: u32,
    /// Our `Shell_TrayWnd`, while hosting.
    window: Option<HWND>,
    explorer: Option<Explorer>,
    band: Option<Band>,
    icons: Icons,
    settings: Vec<Setting>,
    order: Vec<u64>,
    /// exe → its name, for icons with no tooltip.
    names: HashMap<String, String>,
    /// id → the icon's rect on screen, from its last click.
    anchors: HashMap<String, [i32; 4]>,
    sent: Option<Vec<TrayItem>>,
    report_at: Option<Instant>,
    reported: Option<Instant>,
    wic: Option<IWICImagingFactory>,
    /// Explorer's tray window, or ours, was destroyed: the loop steps aside.
    step_aside: bool,
    /// Icon messages still to pass on to Explorer, with the icon copies each carries (destroyed
    /// once Explorer has them). Forwarded from the loop, one at a time, so a burst (every app
    /// re-adding at once) never nests inside itself or delays app-bar calls.
    forwards: std::collections::VecDeque<(WPARAM, Vec<u8>, Vec<HICON>)>,
}

impl State {
    /// Reports the icons once a burst settles, and at most every [`REPORT_GAP`].
    fn report_soon(&mut self, settle: Duration) {
        let at = Instant::now() + settle;
        let at = self.reported.map_or(at, |last| at.max(last + REPORT_GAP));
        self.report_at.get_or_insert(at);
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs `f` on the host's state, unless it's borrowed (a message nested inside a forward that
/// holds it: none does, but a wrong guess must not panic in a window procedure).
fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

fn hwnd(raw: u32) -> HWND {
    HWND(raw as usize as *mut c_void)
}

fn find_explorer() -> Option<Explorer> {
    let tray = mypaper_bars::taskbar::explorer_tray()?;
    let mut pid = 0;
    // SAFETY: a read-only query.
    unsafe { GetWindowThreadProcessId(tray, Some(&mut pid)) };
    Some(Explorer { tray, pid })
}

/// How long Explorer's process has been running.
fn explorer_age(pid: u32) -> Option<Duration> {
    // SAFETY: a query handle we close; the FILETIMEs outlive the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        ok.ok()?;
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        // FILETIME counts 100 ns from 1601.
        let created = Duration::from_nanos(
            (ticks(created).checked_sub(116_444_736_000_000_000)?).saturating_mul(100),
        );
        Some(now.saturating_sub(created))
    }
}

fn register_classes() -> bool {
    // SAFETY: plain registration with a 'static window procedure and class names.
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            return false;
        };
        let class = |name: PCWSTR, proc: WNDPROC| WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: proc,
            hInstance: instance.into(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&class(TRAY_CLASS, Some(tray_proc))) != 0
            && RegisterClassExW(&class(NOTIFY_CLASS, Some(notify_proc))) != 0
    }
}

fn unregister_classes() {
    // SAFETY: our own classes, after their windows are gone.
    unsafe {
        if let Ok(instance) = GetModuleHandleW(None) {
            let _ = UnregisterClassW(TRAY_CLASS, Some(instance.into()));
            let _ = UnregisterClassW(NOTIFY_CLASS, Some(instance.into()));
        }
    }
}

/// Creates our `Shell_TrayWnd` (never visible) above Explorer's.
fn create_window(band: Option<Band>) -> Option<HWND> {
    let [x, y, width, height] = band.map_or([0, 0, 0, 0], |b| b.rect);
    // SAFETY: classes registered on this thread; the windows are ours.
    unsafe {
        let tray = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            TRAY_CLASS,
            PCWSTR::null(),
            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )
        .inspect_err(|e| log::warn!("tray: no Shell_TrayWnd: {e}"))
        .ok()?;
        let _ = CreateWindowExW(
            Default::default(),
            NOTIFY_CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            width,
            height,
            Some(tray),
            None,
            None,
            None,
        );
        raise(tray);
        Some(tray)
    }
}

/// Puts our window on top of the topmost band, where `FindWindow` meets it first.
fn raise(window: HWND) {
    // SAFETY: our own window; never activated or shown.
    let _ = unsafe {
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

/// Re-raises our window if another `Shell_TrayWnd` got above it.
fn ensure_on_top() {
    let Some(window) = with_state(|s| s.window).flatten() else {
        return;
    };
    // SAFETY: a lookup by class.
    let first = unsafe { FindWindowW(TRAY_CLASS, PCWSTR::null()) }.ok();
    if first != Some(window) {
        log::debug!("tray: Explorer's tray got above ours; raising ours");
        raise(window);
    }
}

fn run(commands: &Receiver<Command>, own: Sender<Command>, waker: Waker, band: Option<Band>) {
    // SAFETY: COM for WIC on this thread; balanced below.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    // SAFETY: plain COM creation.
    let wic = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .inspect_err(|e| log::warn!("tray: no icon images (WIC): {e}"))
        .ok();
    let registered = register_classes();
    if !registered {
        log::warn!("tray: couldn't register Shell_TrayWnd; not hosting");
    }
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            waker,
            commands: own,
            // SAFETY: no preconditions.
            thread: unsafe { GetCurrentThreadId() },
            window: None,
            explorer: None,
            band,
            icons: Icons::default(),
            settings: Vec::new(),
            order: Vec::new(),
            names: HashMap::new(),
            anchors: HashMap::new(),
            sent: None,
            report_at: None,
            reported: None,
            wic,
            step_aside: false,
            forwards: std::collections::VecDeque::new(),
        })
    });
    let settings = SettingsWatch::open();
    reread_settings();
    let mut settings_read = Instant::now();

    let mut session = Session::default();
    let mut join_at = registered.then(Instant::now);
    let mut settings_at: Option<Instant> = None;
    let mut check_at = registered.then(|| Instant::now() + CHECK_EVERY);
    'run: loop {
        let now = Instant::now();
        if join_at.is_some_and(|at| at <= now) {
            join_at = None;
            match session.join() {
                Join::Hosting => {}
                Join::Later(wait) => join_at = Some(now + wait),
                Join::NoExplorer => {} // until Explorer's tray is back
            }
        }
        if settings_at.is_some_and(|at| at <= now) {
            settings_at = None;
            settings_read = now;
            reread_settings();
        }
        if check_at.is_some_and(|at| at <= now) {
            check_at = Some(now + CHECK_EVERY);
            if session.hosting() {
                ensure_on_top();
            } else if join_at.is_none() && find_explorer().is_some() {
                // Explorer's tray is back without a TaskbarCreated reaching us: let it settle.
                join_at = Some(now + EXPLORER_SETTLE);
            }
        }
        report_if_due(now);

        let deadline = [
            join_at,
            settings_at,
            check_at,
            with_state(|s| s.report_at).flatten(),
        ]
        .into_iter()
        .flatten()
        .min();
        let timeout = deadline.map_or(INFINITE, |at| {
            at.saturating_duration_since(Instant::now()).as_millis() as u32
        });
        let mut handles = Vec::new();
        if let Some(process) = session.process {
            handles.push(process);
        }
        if let Some(watch) = &settings {
            handles.push(watch.event);
        }
        // SAFETY: waits on our own queue and handles we own.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        match handles.get(index) {
            Some(&handle) if Some(handle) == session.process => {
                log::info!("tray: Explorer exited; stepping aside until it's back");
                session.leave();
            }
            Some(_) => {
                settings_at.get_or_insert(
                    (Instant::now() + SETTINGS_SETTLE).max(settings_read + SETTINGS_GAP),
                );
                if let Some(watch) = &settings {
                    watch.arm();
                }
            }
            None => {}
        }

        let mut msg = MSG::default();
        // SAFETY: the standard message pump on the thread that owns the windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                match msg.message {
                    WM_QUIT => break 'run,
                    WM_HOST_COMMAND => {
                        while let Ok(command) = commands.try_recv() {
                            match command {
                                Command::Click {
                                    id,
                                    click,
                                    cursor,
                                    anchor,
                                } => click_icon(&id, click, cursor, anchor),
                                Command::Place(band) => place(band),
                                Command::ExplorerRestarted => {
                                    if session.hosting() && session.explorer_changed() {
                                        session.leave();
                                    }
                                    if !session.hosting() {
                                        join_at = Some(Instant::now() + EXPLORER_SETTLE);
                                    }
                                }
                                Command::Named { exe, name } => {
                                    with_state(|s| {
                                        s.names.insert(exe, name);
                                        s.report_soon(REPORT_SETTLE);
                                    });
                                }
                            }
                        }
                    }
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
        forward_queued(FORWARD_TIMEOUT_MS);
        if with_state(|s| std::mem::take(&mut s.step_aside)).unwrap_or(false) {
            session.leave();
        }
    }

    forward_queued(SHUTDOWN_TIMEOUT_MS);
    session.leave();
    drop(settings);
    STATE.with(|cell| cell.borrow_mut().take());
    if registered {
        unregister_classes();
    }
    if com {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

enum Join {
    Hosting,
    Later(Duration),
    NoExplorer,
}

/// One stretch of hosting, between Explorer starts.
#[derive(Default)]
struct Session {
    /// Explorer's process, signaled when it exits.
    process: Option<HANDLE>,
    hooks: Vec<HWINEVENTHOOK>,
}

impl Session {
    fn hosting(&self) -> bool {
        with_state(|s| s.window.is_some()).unwrap_or(false)
    }

    /// Explorer's tray isn't the one we forward to any more.
    fn explorer_changed(&self) -> bool {
        let now = find_explorer().map(|e| e.tray);
        with_state(|s| s.explorer.map(|e| e.tray) != now).unwrap_or(true)
    }

    /// Becomes the tray, if Explorer's is there and settled.
    fn join(&mut self) -> Join {
        let Some(explorer) = find_explorer() else {
            return Join::NoExplorer;
        };
        if let Some(age) = explorer_age(explorer.pid)
            && age < EXPLORER_SETTLE
        {
            return Join::Later(EXPLORER_SETTLE - age);
        }
        let band = with_state(|s| s.band).flatten();
        let Some(window) = create_window(band) else {
            return Join::NoExplorer;
        };
        // SAFETY: a wait-only handle we close in `leave`.
        self.process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, explorer.pid) }.ok();
        // Explorer's own windows: its tray raised, destroyed, shown, hidden or moved.
        self.hooks = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
        ]
        .into_iter()
        // SAFETY: out-of-context hooks on one process, delivered to this thread's loop; removed in
        // `leave`.
        .map(|(min, max)| unsafe {
            SetWinEventHook(
                min,
                max,
                None,
                Some(on_explorer_event),
                explorer.pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        })
        .filter(|hook| !hook.is_invalid())
        .collect();
        with_state(|s| {
            s.window = Some(window);
            s.explorer = Some(explorer);
        });
        broadcast_taskbar_created();
        log::info!("tray: Shell_TrayWnd up; asked apps to add their icons");
        Join::Hosting
    }

    /// Stops being the tray: our window goes, and apps register with Explorer again. Not while
    /// Explorer's tray is gone: nobody would take their icons, and Explorer broadcasts its own
    /// `TaskbarCreated` when it's back.
    fn leave(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: a hook from SetWinEventHook.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        if let Some(process) = self.process.take() {
            // SAFETY: our handle.
            let _ = unsafe { CloseHandle(process) };
        }
        let window = with_state(|s| {
            s.explorer = None;
            s.window.take()
        })
        .flatten();
        if let Some(window) = window {
            // SAFETY: our own window, on its thread.
            let _ = unsafe { DestroyWindow(window) };
            if find_explorer().is_some() {
                broadcast_taskbar_created();
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.leave();
    }
}

unsafe extern "system" fn on_explorer_event(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // The window itself (not its caret, scroll bars or child elements).
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let explorer = with_state(|s| s.explorer.map(|e| e.tray)).flatten();
    if explorer != Some(window) {
        return;
    }
    // SAFETY: a read-only query; a destroyed handle reads as gone.
    if event == EVENT_OBJECT_DESTROY && !unsafe { IsWindow(Some(window)) }.as_bool() {
        // Explorer's taskbar went away inside a living process (it rebuilds it): step aside now,
        // so it starts over without a second Shell_TrayWnd around.
        log::info!("tray: Explorer's tray window was destroyed; stepping aside");
        with_state(|s| {
            s.explorer = None;
            s.step_aside = true;
        });
    } else if event != EVENT_OBJECT_DESTROY {
        ensure_on_top();
    }
}

fn place(band: Option<Band>) {
    let window = with_state(|s| {
        s.band = band;
        s.window
    })
    .flatten();
    if let (Some(window), Some(band)) = (window, band) {
        let [x, y, width, height] = band.rect;
        // SAFETY: our own window; it stays hidden.
        let _ = unsafe {
            SetWindowPos(
                window,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_HIDEWINDOW,
            )
        };
        raise(window);
    }
}

// --- The window procedure --------------------------------------------------------------------

unsafe extern "system" fn tray_proc(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COPYDATA if lparam.0 != 0 => {
            // SAFETY: WM_COPYDATA's lParam is a COPYDATASTRUCT valid for this call.
            let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            let bytes = if data.lpData.is_null() || data.cbData == 0 {
                Vec::new()
            } else {
                // SAFETY: the system copied cbData bytes to lpData for this call.
                unsafe {
                    std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize)
                }
                .to_vec()
            };
            let started = Instant::now();
            let kind = data.dwData;
            let result = match kind {
                COPYDATA_NOTIFY => on_notify(wparam, bytes),
                COPYDATA_APPBAR => on_appbar(wparam, bytes),
                COPYDATA_ICON_RECT => on_rect_query(&bytes),
                other => forward_copydata(wparam, other, &bytes),
            };
            let took = started.elapsed();
            if took > Duration::from_millis(100) {
                log::info!("tray: WM_COPYDATA {kind} took {took:?}");
            }
            return result;
        }
        // Never shown, whatever shows it: the show is taken out on its way in, and the style
        // cleared if one got through anyway.
        WM_WINDOWPOSCHANGING if lparam.0 != 0 => {
            // SAFETY: WM_WINDOWPOSCHANGING's lParam is a WINDOWPOS the sender lets us change.
            let pos = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
            pos.flags &= !SWP_SHOWWINDOW;
        }
        WM_WINDOWPOSCHANGED => {
            // SAFETY: our own window's style.
            unsafe {
                let style = GetWindowLongPtrW(window, GWL_STYLE);
                if style & WS_VISIBLE.0 as isize != 0 {
                    SetWindowLongPtrW(window, GWL_STYLE, style & !(WS_VISIBLE.0 as isize));
                }
            }
        }
        // Commands meant for the taskbar: its menu, Start, and its private messages.
        WM_COMMAND => return forward(msg, wparam, lparam),
        WM_SYSCOMMAND if wparam.0 & 0xFFF0 == SC_TASKLIST as usize => {
            return forward(msg, wparam, lparam);
        }
        // Registered messages (≥ 0xC000) are broadcasts like TaskbarCreated: Explorer has its own.
        WM_USER..0xC000 => return forward(msg, wparam, lparam),
        // Explorer's tray ignores it too; only the host closes its window.
        WM_CLOSE => return LRESULT(0),
        // Destroyed by someone else (`leave` takes the window out of the state first).
        WM_NCDESTROY => {
            with_state(|s| {
                if s.window == Some(window) {
                    log::info!("tray: our Shell_TrayWnd was destroyed; stepping aside");
                    s.window = None;
                    s.step_aside = true;
                }
            });
        }
        _ => {}
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

/// `TrayNotifyWnd`, the child apps look up for the tray's rectangle: nothing of its own to do,
/// and it can't be closed either.
unsafe extern "system" fn notify_proc(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLOSE {
        return LRESULT(0);
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

fn explorer() -> Option<Explorer> {
    with_state(|s| s.explorer).flatten()
}

thread_local! {
    /// Synchronous forwards in progress on the host's thread (see [`MAX_DEPTH`]).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Runs a synchronous forward, or answers `refused` when too many are nested already.
fn nested<T>(refused: T, send: impl FnOnce() -> T) -> T {
    let depth = DEPTH.get();
    if depth >= MAX_DEPTH {
        log::warn!("tray: forwards nested {depth} deep; refusing (a loop between trays?)");
        return refused;
    }
    DEPTH.set(depth + 1);
    let result = send();
    DEPTH.set(depth);
    result
}

/// Passes a message on to Explorer's tray the way it came: sent (with its result), or posted.
fn forward(msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(explorer) = explorer() else {
        return LRESULT(0);
    };
    // SAFETY: plain message calls on Explorer's window; the timeout bounds a hung Explorer.
    unsafe {
        let how = InSendMessageEx(None);
        if how & ISMEX_NOTIFY != 0 {
            let _ = SendNotifyMessageW(explorer.tray, msg, wparam, lparam);
            LRESULT(0)
        } else if how & ISMEX_SEND != 0 {
            nested(LRESULT(0), || {
                let mut result = 0usize;
                SendMessageTimeoutW(
                    explorer.tray,
                    msg,
                    wparam,
                    lparam,
                    SMTO_ABORTIFHUNG,
                    FORWARD_TIMEOUT_MS,
                    Some(&mut result),
                );
                LRESULT(result as isize)
            })
        } else {
            let _ = PostMessageW(Some(explorer.tray), msg, wparam, lparam);
            LRESULT(0)
        }
    }
}

/// Sends `bytes` to Explorer's tray as a `WM_COPYDATA` of kind `kind`, and returns its answer
/// (0 when it didn't answer).
fn forward_copydata(wparam: WPARAM, kind: usize, bytes: &[u8]) -> LRESULT {
    explorer()
        .and_then(|explorer| copydata_to(explorer.tray, wparam, kind, bytes, FORWARD_TIMEOUT_MS))
        .unwrap_or(LRESULT(0))
}

/// `None` when the send failed or timed out: the receiver may still read what it was given
/// later, so whatever the message points at must stay alive.
fn copydata_to(
    target: HWND,
    wparam: WPARAM,
    kind: usize,
    bytes: &[u8],
    timeout_ms: u32,
) -> Option<LRESULT> {
    let data = COPYDATASTRUCT {
        dwData: kind,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut c_void,
    };
    nested(None, || {
        let mut result = 0usize;
        // SAFETY: `data` and `bytes` outlive the synchronous call; the system copies them across.
        let sent = unsafe {
            SendMessageTimeoutW(
                target,
                WM_COPYDATA,
                wparam,
                LPARAM(&raw const data as isize),
                SMTO_ABORTIFHUNG,
                timeout_ms,
                Some(&mut result),
            )
        };
        (sent.0 != 0).then_some(LRESULT(result as isize))
    })
}

/// `Shell_NotifyIcon`: keep the icon, answer the app at once, then tell Explorer the same.
fn on_notify(wparam: WPARAM, mut bytes: Vec<u8>) -> LRESULT {
    let Some(notify) = tray_data::parse_notify(&bytes) else {
        return forward_copydata(wparam, COPYDATA_NOTIFY, &bytes);
    };
    // Apps often destroy their icons right after the call returns: copy them first. Explorer
    // gets the copies (it copies them again while we wait), then we draw the icon and let go.
    let copies: Vec<(usize, HICON)> = notify
        .handles()
        .into_iter()
        .filter_map(|(at, handle)| {
            // SAFETY: icons are session-wide USER objects; a stale handle just fails.
            let copy = unsafe { CopyIcon(HICON(handle as usize as *mut c_void)) }.ok()?;
            bytes[at..at + 4].copy_from_slice(&(copy.0 as usize as u32).to_le_bytes());
            Some((at, copy))
        })
        .collect();
    let icon = copies
        .iter()
        .find(|&&(at, _)| at == ICON_OFFSET)
        .map(|&(_, copy)| copy);
    let applied = with_state(|s| {
        let applied = s.icons.apply(&notify, None, || owner_of(notify.hwnd));
        if applied.changed {
            s.report_soon(REPORT_SETTLE);
        }
        applied
    });
    let accepted = applied.is_some_and(|a| a.accepted);
    // SAFETY: answers the app's SendMessage; it doesn't wait for Explorer or for the drawing.
    let _ = unsafe { ReplyMessage(LRESULT(isize::from(accepted))) };
    if let Some(icon) = icon {
        let image = with_state(|s| s.wic.clone())
            .flatten()
            .and_then(|wic| icon_data_url(icon, &wic));
        with_state(|s| {
            if s.icons.set_image(&notify, image) {
                s.report_soon(REPORT_SETTLE);
            }
        });
    }
    let copies: Vec<HICON> = copies.into_iter().map(|(_, copy)| copy).collect();
    let queued = with_state(|s| s.forwards.push_back((wparam, bytes, copies.clone()))).is_some();
    if !queued {
        for copy in copies {
            // SAFETY: our copy, nobody else has it.
            let _ = unsafe { DestroyIcon(copy) };
        }
    }
    LRESULT(isize::from(accepted))
}

/// Passes the queued icon messages on to Explorer (see `State::forwards`), each allowed
/// `timeout_ms`.
fn forward_queued(timeout_ms: u32) {
    while let Some((wparam, bytes, copies)) = with_state(|s| s.forwards.pop_front()).flatten() {
        let delivered = explorer().is_none_or(|explorer| {
            copydata_to(explorer.tray, wparam, COPYDATA_NOTIFY, &bytes, timeout_ms).is_some()
        });
        if !delivered {
            // Explorer may still take the message later and copy the icons then: they stay.
            log::debug!("tray: Explorer didn't take an icon message; keeping its icons");
            continue;
        }
        for copy in copies {
            // SAFETY: our copy; Explorer took its own during the forward (or there's none).
            let _ = unsafe { DestroyIcon(copy) };
        }
    }
}

/// The owner window's process and exe.
fn owner_of(window: u32) -> Owner {
    let mut pid = 0;
    // SAFETY: read-only queries; the handle is closed.
    unsafe {
        GetWindowThreadProcessId(hwnd(window), Some(&mut pid));
    }
    Owner {
        pid,
        exe: process_exe(pid).unwrap_or_default().to_lowercase(),
    }
}

fn process_exe(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    // SAFETY: a query handle we close; the buffer outlives the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

/// `SHAppBarMessage`. Explorer answers; we only re-home the shared memory of the two messages
/// that carry it, and answer `ABM_GETTASKBARPOS` with the bar that shows the tray.
fn on_appbar(wparam: WPARAM, mut bytes: Vec<u8>) -> LRESULT {
    let Some(message) = tray_data::parse_appbar(&bytes) else {
        return forward_copydata(wparam, COPYDATA_APPBAR, &bytes);
    };
    match message.message {
        ABM_GETTASKBARPOS => {
            let Some(band) = with_state(|s| s.band).flatten() else {
                return forward_copydata(wparam, COPYDATA_APPBAR, &bytes);
            };
            // SAFETY: the sender's shared APPBARDATA, locked and unlocked here.
            unsafe {
                let data =
                    SHLockShared(HANDLE(message.shared as usize as *mut c_void), message.pid);
                if data.is_null() {
                    return LRESULT(0);
                }
                let view = std::slice::from_raw_parts_mut(data.cast::<u8>(), APPBARDATA_SIZE);
                tray_data::write_taskbar_pos(view, band.edge, band.rect);
                let _ = SHUnlockShared(data);
            }
            LRESULT(1)
        }
        ABM_QUERYPOS | ABM_SETPOS => {
            let Some(explorer) = explorer() else {
                return LRESULT(0);
            };
            rehomed_appbar(wparam, &mut bytes, message, explorer)
        }
        _ => forward_copydata(wparam, COPYDATA_APPBAR, &bytes),
    }
}

/// The sender put its `APPBARDATA` in memory shared with *us*; Explorer can't open it. Copy it to
/// a block shared with Explorer, forward, and copy Explorer's answer back (ManagedShell's
/// `AppBarManager`, PR #48).
fn rehomed_appbar(
    wparam: WPARAM,
    bytes: &mut [u8],
    message: tray_data::AppBarMessage,
    explorer: Explorer,
) -> LRESULT {
    // SAFETY: shared blocks locked and unlocked in pairs; ours is freed before returning.
    unsafe {
        let theirs = SHLockShared(HANDLE(message.shared as usize as *mut c_void), message.pid);
        if theirs.is_null() {
            return forward_copydata(wparam, COPYDATA_APPBAR, bytes);
        }
        let ours = SHAllocShared(Some(theirs), APPBARDATA_SIZE as u32, explorer.pid);
        if ours.is_invalid() {
            let _ = SHUnlockShared(theirs);
            return LRESULT(0);
        }
        tray_data::retarget_appbar(bytes, ours.0 as usize as u64, explorer.pid);
        let Some(result) = copydata_to(
            explorer.tray,
            wparam,
            COPYDATA_APPBAR,
            bytes,
            FORWARD_TIMEOUT_MS,
        ) else {
            // Explorer may still open the block once it gets to the message: it stays (a leak of
            // one small block beats Explorer reading freed memory).
            let _ = SHUnlockShared(theirs);
            return LRESULT(0);
        };
        let answer = SHLockShared(ours, explorer.pid);
        if !answer.is_null() {
            std::ptr::copy_nonoverlapping(
                answer.cast::<u8>(),
                theirs.cast::<u8>(),
                APPBARDATA_SIZE,
            );
            let _ = SHUnlockShared(answer);
        }
        let _ = SHUnlockShared(theirs);
        let _ = SHFreeShared(ours, explorer.pid);
        result
    }
}

/// `Shell_NotifyIconGetRect`: where the icon is, so the app can put its popup beside it.
fn on_rect_query(bytes: &[u8]) -> LRESULT {
    let Some(query) = tray_data::parse_rect_query(bytes) else {
        return LRESULT(0);
    };
    let rect = with_state(|s| {
        let icon = s.icons.list.iter().find(|i| match query.guid {
            Some(guid) => i.guid == Some(guid),
            None => i.hwnd == query.hwnd && i.uid == query.uid,
        })?;
        s.anchors.get(&icon.id()).copied().or_else(|| {
            // Never clicked: under the pointer, which is where the user last reached for it.
            let mut point = POINT::default();
            // SAFETY: `point` outlives the call.
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.ok()?;
            Some([point.x - 12, point.y - 12, 24, 24])
        })
    })
    .flatten();
    rect.map_or(LRESULT(0), |rect| {
        LRESULT(tray_data::rect_reply(query.message, rect))
    })
}

// --- Clicks and reports -----------------------------------------------------------------------

fn click_icon(id: &str, click: Click, cursor: (i32, i32), anchor: Option<[i32; 4]>) {
    let target = with_state(|s| {
        if let Some(anchor) = anchor {
            s.anchors.insert(id.to_owned(), anchor);
        }
        s.icons
            .find(id)
            .map(|i| (i.hwnd, i.uid, i.version, i.callback, i.pid))
    })
    .flatten();
    let Some((owner, uid, version, callback, pid)) = target else {
        return log::info!("tray: no icon {id:?}");
    };
    if !alive(owner, pid) {
        prune();
        return;
    }
    // SAFETY: lets the icon's owner take the foreground our click gave us (its menu, its window).
    let _ = unsafe { AllowSetForegroundWindow(pid) };
    for (wparam, lparam) in tray_data::click_messages(click, version, uid, cursor) {
        // SAFETY: posted to the owner's window; it doesn't wait for a busy app.
        let _ =
            unsafe { SendNotifyMessageW(hwnd(owner), callback, WPARAM(wparam), LPARAM(lparam)) };
    }
}

/// The owner window still exists and belongs to the same process (handles get recycled).
fn alive(owner: u32, pid: u32) -> bool {
    let mut now = 0;
    // SAFETY: read-only queries; a stale handle reads as gone.
    unsafe {
        IsWindow(Some(hwnd(owner))).as_bool() && {
            GetWindowThreadProcessId(hwnd(owner), Some(&mut now));
            now == pid
        }
    }
}

fn prune() {
    with_state(|s| {
        if s.icons.prune(|i| alive(i.hwnd, i.pid)) {
            s.report_soon(Duration::ZERO);
        }
    });
}

fn report_if_due(now: Instant) {
    let due = with_state(|s| s.report_at.is_some_and(|at| at <= now)).unwrap_or(false);
    if !due {
        return;
    }
    prune();
    with_state(|s| {
        s.report_at = None;
        s.reported = Some(now);
        // An exe not described yet shows its file name until `describe` reports back.
        let mut unnamed = Vec::new();
        let names = &s.names;
        let items = tray_data::items(&s.icons.list, &s.settings, &s.order, |icon| {
            names.get(&icon.exe).cloned().unwrap_or_else(|| {
                unnamed.push(icon.exe.clone());
                crate::apps::file_stem(&icon.exe)
            })
        });
        unnamed.sort();
        unnamed.dedup();
        for exe in &unnamed {
            s.names.insert(exe.clone(), crate::apps::file_stem(exe));
        }
        if !unnamed.is_empty() {
            describe(unnamed, s.commands.clone(), s.thread);
        }
        if s.sent.as_ref() != Some(&items) {
            log::debug!(
                "tray: {} icons ({} pinned): {:?}",
                items.len(),
                items.iter().filter(|i| i.pinned).count(),
                items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>()
            );
            s.waker.send(Msg::Tray(items.clone()));
            s.sent = Some(items);
        }
    });
}

/// Reads the exes' version descriptions (file reads) on a helper thread, so the host's thread
/// never blocks on a slow disk while apps wait on it; each comes back as `Command::Named`.
fn describe(exes: Vec<String>, commands: Sender<Command>, thread: u32) {
    let spawned = std::thread::Builder::new()
        .name("tray-names".into())
        .spawn(move || {
            for exe in exes {
                if let Some(name) = crate::icons::version_description(&exe)
                    && commands.send(Command::Named { exe, name }).is_err()
                {
                    return;
                }
            }
            // SAFETY: posting to the host's thread; if it's gone, nobody reads the message.
            let _ = unsafe { PostThreadMessageW(thread, WM_HOST_COMMAND, WPARAM(0), LPARAM(0)) };
        });
    if let Err(e) = spawned {
        log::debug!("tray: not reading app names: {e}");
    }
}

// --- NotifyIconSettings -------------------------------------------------------------------------

const SETTINGS_KEY: PCWSTR = w!(r"Control Panel\NotifyIconSettings");

/// A change notification on `NotifyIconSettings`.
struct SettingsWatch {
    key: HKEY,
    event: HANDLE,
}

impl SettingsWatch {
    fn open() -> Option<Self> {
        // SAFETY: a key and an event we close on drop.
        unsafe {
            let mut key = HKEY::default();
            RegOpenKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY, None, KEY_NOTIFY, &mut key)
                .ok()
                .ok()?;
            let event = CreateEventW(None, false, false, PCWSTR::null()).ok()?;
            let watch = Self { key, event };
            watch.arm();
            Some(watch)
        }
    }

    /// Asks for the next change (each notification fires once).
    fn arm(&self) {
        // SAFETY: our key and event.
        let _ = unsafe {
            RegNotifyChangeKeyValue(
                self.key,
                true,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                Some(self.event),
                true,
            )
        };
    }
}

impl Drop for SettingsWatch {
    fn drop(&mut self) {
        // SAFETY: our handles.
        unsafe {
            let _ = RegCloseKey(self.key);
            let _ = CloseHandle(self.event);
        }
    }
}

fn reread_settings() {
    let (settings, order) = read_settings();
    with_state(|s| {
        if s.settings != settings || s.order != order {
            s.settings = settings;
            s.order = order;
            s.report_soon(Duration::ZERO);
        }
    });
}

/// Every entry's exe, uID or GUID and pin, and the user's order.
fn read_settings() -> (Vec<Setting>, Vec<u64>) {
    let mut settings = Vec::new();
    let mut order = Vec::new();
    // SAFETY: registry reads into buffers that outlive each call; the key is closed below.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY, None, KEY_READ, &mut key).is_err() {
            return (settings, order);
        }
        if let Some(bytes) = reg_bytes(key, w!("UIOrderList")) {
            order = tray_data::parse_order(&bytes);
        }
        for index in 0.. {
            let mut name = [0u16; 64];
            let mut len = name.len() as u32;
            let status = RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            );
            if status.is_err() {
                break;
            }
            let text = String::from_utf16_lossy(&name[..len as usize]);
            let Ok(id) = text.parse::<u64>() else {
                continue;
            };
            let sub = HSTRING::from(text);
            let exe = reg_string(key, &sub, w!("ExecutablePath")).unwrap_or_default();
            settings.push(Setting {
                key: id,
                exe: resolve_known_folder(&exe).to_lowercase(),
                uid: reg_dword(key, &sub, w!("UID")),
                guid: reg_string(key, &sub, w!("IconGuid")).and_then(|g| tray_data::parse_guid(&g)),
                promoted: reg_dword(key, &sub, w!("IsPromoted")) == Some(1),
            });
        }
        let _ = RegCloseKey(key);
    }
    (settings, order)
}

/// A binary value of the key itself.
fn reg_bytes(key: HKEY, name: PCWSTR) -> Option<Vec<u8>> {
    // SAFETY: two-call pattern; the buffer is as large as the first call reported.
    unsafe {
        let mut bytes = 0u32;
        RegGetValueW(
            key,
            PCWSTR::null(),
            name,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut data = vec![0u8; bytes as usize];
        RegGetValueW(
            key,
            PCWSTR::null(),
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        data.truncate(bytes as usize);
        Some(data)
    }
}

// --- Icons → PNG ---------------------------------------------------------------------------------

/// An `HICON` as a PNG `data:` URL.
pub fn icon_data_url(icon: HICON, wic: &IWICImagingFactory) -> Option<String> {
    let (width, height, pixels) = icon_pixels(icon)?;
    let png = encode_png(wic, width, height, &pixels).ok()?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// The icon's pixels, top-down BGRA with straight alpha (icons without an alpha channel take
/// it from their mask).
fn icon_pixels(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = ICONINFO::default();
    // SAFETY: GetIconInfo makes bitmaps we delete below; GetDIBits fills buffers of the size the
    // header describes.
    unsafe {
        GetIconInfo(icon, &mut info).ok()?;
        let read = |bitmap: HBITMAP| -> Option<(i32, i32, Vec<u8>)> {
            let mut header = BITMAP::default();
            if GetObjectW(
                bitmap.into(),
                size_of::<BITMAP>() as i32,
                Some((&raw mut header).cast()),
            ) == 0
            {
                return None;
            }
            let (width, height) = (header.bmWidth, header.bmHeight.abs());
            if width <= 0 || height <= 0 || width > 256 || height > 512 {
                return None;
            }
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(None);
            let lines = GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            (lines == height).then_some((width, height, pixels))
        };
        let color = (!info.hbmColor.is_invalid())
            .then(|| read(info.hbmColor))
            .flatten();
        let mask = (!info.hbmMask.is_invalid())
            .then(|| read(info.hbmMask))
            .flatten();
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor.into());
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }
        let (width, height, mut pixels) = match (color, &mask) {
            (Some(color), _) => color,
            // Monochrome: the mask is the AND half on top of the XOR (the picture) half.
            (None, Some((width, height, mask))) => {
                let half = height / 2;
                let picture = mask[(width * half * 4) as usize..].to_vec();
                (*width, half, picture)
            }
            (None, None) => return None,
        };
        let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
        if !has_alpha {
            let mask = mask.map(|(_, _, m)| m).unwrap_or_default();
            for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                // An AND-mask bit set (white) is transparent.
                let transparent = mask.get(i * 4).is_some_and(|&m| m != 0);
                pixel[3] = if transparent { 0 } else { 255 };
            }
        }
        Some((width as u32, height as u32, pixels))
    }
}

fn encode_png(
    wic: &IWICImagingFactory,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> windows::core::Result<Vec<u8>> {
    // SAFETY: COM calls on objects we create; the pixel buffer is width × height × 4.
    unsafe {
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or(windows::core::Error::from(
            windows::Win32::Foundation::E_FAIL,
        ))?;
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WritePixels(height, width * 4, bgra)?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut size = 0u64;
        stream.Seek(
            0,
            windows::Win32::System::Com::STREAM_SEEK_END,
            Some(&mut size),
        )?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut png = vec![0u8; size as usize];
        let mut read = 0u32;
        stream
            .Read(png.as_mut_ptr().cast(), png.len() as u32, Some(&mut read))
            .ok()?;
        png.truncate(read as usize);
        Ok(png)
    }
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), output);
        }
    }
}
