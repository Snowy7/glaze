//! Opt-in fullscreen/presentation hint, not a game classifier. No polling or injection.
//! Install on a message-pumping thread; drop removes hooks/window and publication.
//! Cross-process clients watch `game-activity.json` and wait on the publisher's
//! process handle as well, so a crash cannot leave them paused indefinitely.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result, bail, ensure};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LRESULT, RECT, WAIT_ABANDONED, WAIT_OBJECT_0,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, ReleaseMutex, WaitForSingleObject,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_SYSTEM_DESKTOPSWITCH, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GetClassNameW, GetForegroundWindow, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, KillTimer, OBJID_WINDOW, RegisterClassW,
    SetTimer, WINEVENT_OUTOFCONTEXT, WM_DISPLAYCHANGE, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;

const FILE: &str = "game-activity.json";
const RECHECK: usize = 1;
/// Only while paused: recover from a missed foreground/geometry event or delayed shell hint.
const RECHECK_MS: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Idle,
    Fullscreen,
    Direct3d,
    Busy,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub pause_background: bool,
    pub reason: Reason,
    pub foreground_pid: u32,
}

/// Pure policy: notification suppression is only a conservative hint. A fullscreen
/// editor/video/presentation can pause background work too; quiet hours alone cannot.
pub fn classify(
    notification_state: Option<i32>,
    foreground_covers_monitor: bool,
    foreground_pid: u32,
) -> Activity {
    let reason = match notification_state {
        Some(n) if n == QUNS_RUNNING_D3D_FULL_SCREEN.0 => Reason::Direct3d,
        Some(n) if n == QUNS_PRESENTATION_MODE.0 => Reason::Presentation,
        Some(n) if n == QUNS_BUSY.0 && foreground_covers_monitor => Reason::Busy,
        _ if foreground_covers_monitor => Reason::Fullscreen,
        _ => Reason::Idle,
    };
    Activity {
        pause_background: reason != Reason::Idle,
        reason,
        foreground_pid,
    }
}

/// Physical screen rectangles; a maximized work-area window does not count as fullscreen.
pub fn covers_monitor(window: [i32; 4], monitor: [i32; 4]) -> bool {
    let [l, t, r, b] = window.map(i64::from);
    let [ml, mt, mr, mb] = monitor.map(i64::from);
    mr > ml && mb > mt && r > l && b > t && l <= ml + 1 && t <= mt + 1 && r >= mr - 1 && b >= mb - 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub publisher_pid: u32,
    pub publisher_started: u64,
    pub activity: Activity,
}

pub fn path() -> Result<PathBuf> {
    Ok(crate::palette::local_dir()?.join(FILE))
}

/// A checked publisher identity. Add `handle()` to the consumer's wait set;
/// a signaled process means the signal is no longer authoritative.
pub struct Owner(HANDLE);

impl Owner {
    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: exclusively owned OpenProcess handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn started(handle: HANDLE) -> Result<u64> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: valid process handle and writable out parameters.
    unsafe {
        GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)?;
    }
    Ok(u64::from(creation.dwLowDateTime) | (u64::from(creation.dwHighDateTime) << 32))
}

impl Snapshot {
    pub fn open_owner(&self) -> Result<Owner> {
        // SAFETY: query/synchronize only, no process mutation or injection.
        let owner = Owner(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                self.publisher_pid,
            )?
        });
        ensure!(
            started(owner.0)? == self.publisher_started,
            "game signal publisher PID was reused"
        );
        // SAFETY: owned process handle; nonblocking lifetime check.
        let lifetime = unsafe { WaitForSingleObject(owner.0, 0) };
        ensure!(
            lifetime == windows::Win32::Foundation::WAIT_TIMEOUT,
            "game signal publisher exited"
        );
        Ok(owner)
    }
}

/// Missing, corrupt, disabled or dead publisher = no signal. Keep the returned owner
/// in the wait set; re-read on directory notifications, not on a timer.
pub fn read() -> Option<(Snapshot, Owner)> {
    let snapshot: Snapshot =
        serde_json::from_str(&std::fs::read_to_string(path().ok()?).ok()?).ok()?;
    let owner = snapshot.open_owner().ok()?;
    Some((snapshot, owner))
}

/// Watches atomic replacement/removal. Dropping releases the directory watcher.
pub fn watch(changed: impl Fn() + Send + 'static) -> Result<notify::RecommendedWatcher> {
    let file = path()?;
    let parent = file.parent().context("game signal path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let target = file.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| event.paths.iter().any(|p| p == &target)) {
            changed();
        }
    })?;
    watcher.watch(parent, notify::RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

struct State {
    snapshot: Snapshot,
    file: PathBuf,
    window: HWND,
    recheck: bool,
}

struct Lease(HANDLE);

impl Lease {
    fn acquire() -> Result<Self> {
        // SAFETY: session-local mutex; only its owner may publish/remove the signal file.
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\mypaper.game-activity.v1"))? };
        // SAFETY: valid owned handle, zero timeout.
        let acquired = unsafe { WaitForSingleObject(handle, 0) };
        if !matches!(acquired, WAIT_OBJECT_0 | WAIT_ABANDONED) {
            // SAFETY: not acquired, so only close (do not release ownership).
            let _ = unsafe { CloseHandle(handle) };
            bail!("another game activity publisher is running");
        }
        Ok(Self(handle))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // SAFETY: acquired and released on the watcher's installing thread.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Only constructed when `system.game_activity` is true. Owns no worker thread/timer.
/// One publisher per desktop session (core owns it); bound to the installing thread.
pub struct GameWatcher {
    hooks: Vec<HWINEVENTHOOK>,
    window: HWND,
    _thread: PhantomData<Rc<()>>,
    _lease: Lease,
}

impl GameWatcher {
    pub fn start() -> Result<Self> {
        ensure!(
            STATE.with_borrow(|state| state.is_none()),
            "game watcher already installed on this thread"
        );
        let lease = Lease::acquire()?;
        // SAFETY: query-only handle owned below.
        let owner = Owner(unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, std::process::id())?
        });
        let snapshot = Snapshot {
            publisher_pid: std::process::id(),
            publisher_started: started(owner.0)?,
            activity: sample(),
        };
        let file = path()?;
        publish(&file, &snapshot)?;
        STATE.with_borrow_mut(|state| {
            *state = Some(State {
                snapshot,
                file,
                window: HWND::default(),
                recheck: false,
            })
        });
        let mut watcher = Self {
            hooks: Vec::new(),
            window: HWND::default(),
            _thread: PhantomData,
            _lease: lease,
        };
        // SAFETY: process module is loaded for the lifetime of this code.
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("mypaper-game-activity"),
            ..Default::default()
        };
        // SAFETY: class strings/callback are static; already registered on restart is fine.
        unsafe {
            RegisterClassW(&class);
        }
        // A hidden top-level window receives WM_SETTINGCHANGE; a message-only one would not.
        // SAFETY: registered class and no window data; drop destroys the owned window.
        watcher.window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        STATE.with_borrow_mut(|state| {
            state.as_mut().expect("initialized above").window = watcher.window
        });
        for event in [
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_DESKTOPSWITCH,
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            EVENT_OBJECT_LOCATIONCHANGE,
        ] {
            // SAFETY: static out-of-context callback, removed on this thread in drop.
            let hook = unsafe {
                SetWinEventHook(
                    event,
                    event,
                    None,
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_invalid() {
                bail!("game activity SetWinEventHook failed");
            }
            watcher.hooks.push(hook);
        }
        refresh();
        Ok(watcher)
    }
}

impl Drop for GameWatcher {
    fn drop(&mut self) {
        if !self.window.is_invalid() {
            // SAFETY: our hidden window and timer; removes the paused-only fallback.
            let _ = unsafe { KillTimer(Some(self.window), RECHECK) };
        }
        for hook in self.hooks.drain(..) {
            // SAFETY: this watcher owns each hook on its installing thread.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        if let Some(state) = STATE.with_borrow_mut(Option::take)
            && let Err(error) = std::fs::remove_file(state.file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("removing game signal: {error}");
        }
        if !self.window.is_invalid() {
            // SAFETY: owned hidden window, created on this thread.
            let _ = unsafe { DestroyWindow(self.window) };
        }
    }
}

fn publish(path: &std::path::Path, snapshot: &Snapshot) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("game signal path has no parent")?)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(snapshot)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}

fn refresh() {
    let activity = sample();
    STATE.with(|state| {
        // Win32 calls can reenter; the outer callback publishes the current observation.
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let Some(state) = state.as_mut() else { return };
        if activity != state.snapshot.activity {
            let snapshot = Snapshot {
                activity,
                ..state.snapshot
            };
            match publish(&state.file, &snapshot) {
                Ok(()) => state.snapshot = snapshot,
                Err(error) => log::warn!("publishing game activity: {error:#}"),
            }
        }
        let paused = state.snapshot.activity.pause_background;
        if !state.window.is_invalid() && paused != state.recheck {
            // SAFETY: timer posts to our existing message-pumping hidden window. It is
            // armed only at a pause edge, never reset by frequent WinEvents, and removed
            // on resume/drop. No worker or idle polling loop is added.
            unsafe {
                if paused {
                    state.recheck = SetTimer(Some(state.window), RECHECK, RECHECK_MS, None) != 0;
                    if !state.recheck {
                        log::warn!("could not arm game activity recheck");
                    }
                } else {
                    let _ = KillTimer(Some(state.window), RECHECK);
                    state.recheck = false;
                }
            }
        }
    });
}

fn sample() -> Activity {
    // SAFETY: query APIs only, all out buffers valid locals.
    unsafe {
        // Core also hosts legacy windows; scope physical-coordinate queries to this sample.
        let previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let foreground = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        let mut class = [0u16; 128];
        let length = GetClassNameW(foreground, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        let desktop = matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        );
        let mut rect = RECT::default();
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let fullscreen = !desktop
            && pid != std::process::id()
            && IsWindowVisible(foreground).as_bool()
            && !IsIconic(foreground).as_bool()
            && GetWindowRect(foreground, &mut rect).is_ok()
            && GetMonitorInfoW(
                MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            && covers_monitor(
                [rect.left, rect.top, rect.right, rect.bottom],
                [
                    monitor.rcMonitor.left,
                    monitor.rcMonitor.top,
                    monitor.rcMonitor.right,
                    monitor.rcMonitor.bottom,
                ],
            );
        let activity = classify(
            SHQueryUserNotificationState().ok().map(|s| s.0),
            fullscreen,
            pid,
        );
        if !previous_dpi.is_invalid() {
            SetThreadDpiAwarenessContext(previous_dpi);
        }
        activity
    }
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if event == EVENT_OBJECT_LOCATIONCHANGE {
        // SAFETY: query only, callback runs on the installing thread.
        if object != OBJID_WINDOW.0 || child != 0 || window != unsafe { GetForegroundWindow() } {
            return;
        }
    }
    refresh();
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if matches!(message, WM_SETTINGCHANGE | WM_DISPLAYCHANGE)
        || message == WM_TIMER && wparam.0 == RECHECK
    {
        refresh();
    }
    // SAFETY: forwarding the window procedure's unchanged arguments.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{QUNS_ACCEPTS_NOTIFICATIONS, QUNS_QUIET_TIME};

    #[test]
    fn busy_does_not_latch_a_game_after_exit_minimize_or_focus_away() {
        assert!(classify(Some(QUNS_BUSY.0), true, 42).pause_background);
        // A vanished/minimized game, or a different foreground window, no longer covers
        // a monitor even when Windows keeps its notification-suppression hint set.
        for foreground in [0, 42, 99] {
            let state = classify(Some(QUNS_BUSY.0), false, foreground);
            assert!(!state.pause_background, "Busy without coverage: {state:?}");
            assert_eq!(state.reason, Reason::Idle);
        }
    }

    #[test]
    fn conservative_policy_is_explicit_about_its_hints() {
        for (hint, reason) in [
            (QUNS_PRESENTATION_MODE, Reason::Presentation),
            (QUNS_RUNNING_D3D_FULL_SCREEN, Reason::Direct3d),
        ] {
            let state = classify(Some(hint.0), false, 42);
            assert!(state.pause_background);
            assert_eq!(state.reason, reason);
        }
        assert_eq!(classify(Some(QUNS_BUSY.0), true, 42).reason, Reason::Busy);
        for hint in [
            Some(QUNS_ACCEPTS_NOTIFICATIONS.0),
            Some(QUNS_QUIET_TIME.0),
            None,
        ] {
            assert!(!classify(hint, false, 42).pause_background);
            assert_eq!(classify(hint, true, 42).reason, Reason::Fullscreen);
        }
    }

    #[test]
    fn monitor_coverage_handles_negative_coordinates_work_area_and_bad_rectangles() {
        assert!(covers_monitor([-2560, 0, 0, 1440], [-2560, 0, 0, 1440]));
        assert!(covers_monitor([-2568, -8, 8, 1448], [-2560, 0, 0, 1440]));
        assert!(!covers_monitor([-2560, 44, 0, 1400], [-2560, 0, 0, 1440]));
        assert!(!covers_monitor([0, 0, 0, 0], [0, 0, 0, 0]));
        assert!(!covers_monitor([2560, 1440, 0, 0], [0, 0, 2560, 1440]));
    }
}
