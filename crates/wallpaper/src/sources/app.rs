//! `app` wallpapers: an external renderer (a Unity player build, say) that
//! draws into screen 0's window. One instance; the other screens are left
//! alone (multi-monitor apps are not supported yet).
//!
//! The app is launched non-activating (`SW_SHOWNOACTIVATE`) inside a Job
//! Object with `KILL_ON_JOB_CLOSE`, so it dies with the source (theme switch)
//! and never outlives mypaper, even if mypaper is killed. `{hwnd}` in the
//! arguments is the screen window, for apps that take a parent handle (Unity's
//! `-parentHWND`).
//!
//! Apps that make their own top-level window instead (or ignore the parent
//! argument) are adopted: a WinEvent hook scoped to the process id watches for
//! windows for up to [`ADOPT_TIMEOUT`], and the first suitable one is
//! reparented into the screen window as a borderless child filling it.
//!
//! Lightness: while screen 0 is hidden, the whole process is suspended
//! (`NtSuspendProcess`) and its working set trimmed. A suspended process
//! costs no CPU or GPU at all, which hiding a window alone would not (Unity
//! keeps rendering). Nothing sends the app messages while it is suspended
//! (that would block this thread), so layout only runs while it is running.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use mypaper_theme::{AppWallpaper, LoadedTheme};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, RECT};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, PROCESS_INFORMATION, ResumeThread,
    STARTF_USESHOWWINDOW, STARTUPINFOW, TerminateProcess,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, GA_PARENT, GW_OWNER, GWL_EXSTYLE,
    GWL_STYLE, GetAncestor, GetClientRect, GetDesktopWindow, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, KillTimer, OBJID_WINDOW, SW_SHOWNOACTIVATE, SWP_ASYNCWINDOWPOS,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetParent, SetTimer,
    SetWindowLongPtrW, SetWindowPos, WINEVENT_OUTOFCONTEXT, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_DLGFRAME, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_NOACTIVATE,
    WS_EX_STATICEDGE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_MINIMIZE,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};
use windows::core::{PCWSTR, PWSTR};

use super::{Frame, Redraw, Source, Visibility};
use crate::gpu::Gpu;

/// How long to wait for the app to show a window before assuming it draws
/// straight into the screen window (or not at all).
const ADOPT_TIMEOUT: Duration = Duration::from_secs(5);

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}

/// The spawned app, drawing into screen 0.
pub struct AppSource {
    shared: Rc<Shared>,
    // Declared last: closing the job is what kills the app.
    _job: Job,
}

impl AppSource {
    /// Spawns the app and starts watching for its window. Fails if the
    /// executable is missing or cannot be started.
    pub fn new(theme: &LoadedTheme, config: &AppWallpaper, screens: &[HWND]) -> Result<Self> {
        let target = *screens.first().context("no screens")?;
        let exe = theme.asset(&config.command);
        if !exe.is_file() {
            bail!("app {} does not exist", exe.display());
        }
        let args = substitute_hwnd(&config.args, target.0 as usize);

        let job = Job::new()?;
        let (process, thread) =
            spawn(&job, &exe, &args).with_context(|| format!("starting {}", exe.display()))?;
        let shared = Rc::new(Shared {
            process,
            target,
            pending: Cell::new(true),
            window: Cell::new(None),
            visible: Cell::new(true),
            suspended: Cell::new(false),
        });
        // Watch before the first instruction runs, so no window is missed.
        let watching = watch(&shared);
        // SAFETY: `thread` is the suspended main thread from `spawn`, closed once here.
        unsafe {
            ResumeThread(thread);
            let _ = CloseHandle(thread);
        }
        watching?;
        log::info!(
            "app: started {} (pid {})",
            exe.display(),
            shared.process.pid
        );
        Ok(Self { shared, _job: job })
    }
}

impl Source for AppSource {
    fn render(&mut self, _gpu: &Gpu, _frame: Frame<'_>) {}

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::OnChange
    }

    fn native(&self, _screen: usize) -> bool {
        true
    }

    fn layout(&mut self, screen: usize, _hwnd: HWND, _size: (u32, u32)) {
        if screen == 0
            && !self.shared.suspended.get()
            && let Some(window) = self.shared.window.get()
        {
            fill(window, self.shared.target);
        }
    }

    /// A suspended app keeps its window and last frame, so `Still` and
    /// `Hidden` both just suspend it.
    fn set_visibility(&mut self, screen: usize, _hwnd: HWND, visibility: Visibility) {
        if screen == 0 {
            self.shared.visible.set(visibility == Visibility::Live);
            self.shared.sync();
        }
    }
}

impl Drop for AppSource {
    fn drop(&mut self) {
        if let Some(waiting) = take_waiting(self.shared.process.pid) {
            waiting.stop();
        }
        // The job closes right after this and terminates the app.
    }
}

/// State shared with the window-event callbacks, which run on this thread
/// from the message loop.
struct Shared {
    process: Process,
    /// Screen 0's window: where the app's window goes.
    target: HWND,
    /// Still looking for the app's window. A process must not be suspended
    /// before it has created (and we have embedded) its window.
    pending: Cell<bool>,
    /// The app's window once found.
    window: Cell<Option<HWND>>,
    /// What the engine last said about screen 0.
    visible: Cell<bool>,
    suspended: Cell<bool>,
}

impl Shared {
    /// Suspends or resumes the process to match visibility.
    fn sync(&self) {
        let suspend = !self.visible.get() && !self.pending.get();
        if suspend != self.suspended.replace(suspend) {
            if suspend {
                self.process.suspend();
            } else {
                self.process.resume();
            }
        }
    }
}

// --- Process and job -------------------------------------------------------

/// A process handle (all-access, from `CreateProcessW`) and its id.
struct Process {
    handle: HANDLE,
    pid: u32,
}

impl Process {
    fn suspend(&self) {
        // SAFETY: a valid process handle with suspend access.
        let status = unsafe { NtSuspendProcess(self.handle) };
        if status < 0 {
            log::warn!("app: NtSuspendProcess failed: {status:#x}");
        } else {
            log::debug!("app: suspended pid {}", self.pid);
        }
        // Push its memory to the standby list; it pages back in on demand.
        // SAFETY: a valid process handle with quota access.
        let _ = unsafe { EmptyWorkingSet(self.handle) };
    }

    fn resume(&self) {
        // SAFETY: a valid process handle with suspend access.
        let status = unsafe { NtResumeProcess(self.handle) };
        if status < 0 {
            log::warn!("app: NtResumeProcess failed: {status:#x}");
        } else {
            log::debug!("app: resumed pid {}", self.pid);
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

/// A job object that terminates everything in it when its last handle closes.
struct Job(HANDLE);

impl Job {
    fn new() -> Result<Self> {
        // SAFETY: creates an unnamed job we own; the closure only passes pointers valid for the call.
        unsafe {
            let job = Self(CreateJobObjectW(None, PCWSTR::null())?);
            let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation:
                    windows::Win32::System::JobObjects::JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..Default::default()
                    },
                ..Default::default()
            };
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of_val(&info) as u32,
            )
            .context("configuring the job object")?;
            Ok(job)
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle; closing it kills the app (KILL_ON_JOB_CLOSE).
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Starts `exe` suspended inside `job`, hidden from activation. Returns the
/// process and its still-suspended main thread (the caller resumes it).
fn spawn(job: &Job, exe: &Path, args: &[String]) -> Result<(Process, HANDLE)> {
    let mut command_line: Vec<u16> = command_line(exe, args).encode_utf16().chain([0]).collect();
    let exe_wide: Vec<u16> = exe.to_string_lossy().encode_utf16().chain([0]).collect();
    let dir: Option<Vec<u16>> = exe
        .parent()
        .map(|d| d.to_string_lossy().encode_utf16().chain([0]).collect());
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        // The app's first ShowWindow(SW_SHOWDEFAULT) shows without activating.
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_SHOWNOACTIVATE.0 as u16,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: all buffers are NUL-terminated and outlive the call; the command
    // line is mutable as `CreateProcessW` requires.
    unsafe {
        CreateProcessW(
            PCWSTR(exe_wide.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            None,
            dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
            &startup,
            &mut info,
        )?;
    }
    let process = Process {
        handle: info.hProcess,
        pid: info.dwProcessId,
    };
    // SAFETY: both handles are valid; on failure the process is killed unstarted.
    unsafe {
        if let Err(e) = AssignProcessToJobObject(job.0, info.hProcess) {
            let _ = TerminateProcess(info.hProcess, 1);
            let _ = CloseHandle(info.hThread);
            return Err(e).context("assigning the app to the job object");
        }
    }
    Ok((process, info.hThread))
}

// --- Finding and embedding the app's window --------------------------------

thread_local! {
    /// Apps still looking for their window. The WinEvent and timer callbacks
    /// carry no context, so they find their app here, by process id / timer id.
    static WAITING: RefCell<Vec<Waiting>> = const { RefCell::new(Vec::new()) };
}

struct Waiting {
    shared: Rc<Shared>,
    hook: HWINEVENTHOOK,
    timer: usize,
}

impl Waiting {
    fn stop(self) {
        // SAFETY: the hook and timer were created by `watch` on this thread.
        unsafe {
            let _ = UnhookWinEvent(self.hook);
            let _ = KillTimer(None, self.timer);
        }
    }
}

fn take_waiting(pid: u32) -> Option<Waiting> {
    WAITING.with_borrow_mut(|all| {
        let index = all.iter().position(|w| w.shared.process.pid == pid)?;
        Some(all.swap_remove(index))
    })
}

/// The app's window was found, or the wait timed out: stop watching and let
/// the process be suspended from now on.
fn finish(pid: u32) {
    if let Some(waiting) = take_waiting(pid) {
        let shared = waiting.shared.clone();
        waiting.stop();
        shared.pending.set(false);
        shared.sync();
    }
}

/// Starts watching the app's windows (hook) and bounds the wait (timer).
fn watch(shared: &Rc<Shared>) -> Result<()> {
    // SAFETY: out-of-context hook with a 'static callback; events arrive
    // through this thread's message loop. Filtered to the app's process.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(on_window_event),
            shared.process.pid,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        bail!("SetWinEventHook failed");
    }
    // SAFETY: thread timer with a 'static callback, dispatched by the message loop.
    let timer = unsafe {
        SetTimer(
            None,
            0,
            ADOPT_TIMEOUT.as_millis() as u32,
            Some(on_adopt_timeout),
        )
    };
    WAITING.with_borrow_mut(|all| {
        all.push(Waiting {
            shared: shared.clone(),
            hook,
            timer,
        });
    });
    Ok(())
}

unsafe extern "system" fn on_window_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW.0
        || id_child != CHILDID_SELF as i32
        || !(event == EVENT_OBJECT_CREATE || event == EVENT_OBJECT_SHOW)
    {
        return;
    }
    let facts = WindowFacts::query(hwnd);
    let Some(shared) = WAITING.with_borrow(|all| {
        all.iter()
            .find(|w| w.shared.process.pid == facts.pid)
            .map(|w| w.shared.clone())
    }) else {
        return;
    };
    match classify(&facts, facts.pid, shared.target) {
        Verdict::Ignore => return,
        Verdict::Embedded => log::info!("app: window {hwnd:?} is already inside the screen"),
        Verdict::Reparent => {
            log::info!("app: embedding top-level window {hwnd:?}");
            embed(hwnd, shared.target);
        }
    }
    fill(hwnd, shared.target);
    shared.window.set(Some(hwnd));
    finish(facts.pid);
}

unsafe extern "system" fn on_adopt_timeout(_hwnd: HWND, _msg: u32, timer: usize, _time: u32) {
    let pid = WAITING.with_borrow(|all| {
        all.iter()
            .find(|w| w.timer == timer)
            .map(|w| w.shared.process.pid)
    });
    if let Some(pid) = pid {
        log::info!(
            "app: no window to embed after {ADOPT_TIMEOUT:?}; assuming it draws into the screen window itself"
        );
        finish(pid);
    }
}

/// What the adoption filter needs to know about a window.
struct WindowFacts {
    pid: u32,
    /// The parent window; null for a top-level window.
    parent: HWND,
    owned: bool,
    style: u32,
    ex_style: u32,
}

impl WindowFacts {
    fn query(hwnd: HWND) -> Self {
        let mut pid = 0;
        // SAFETY: read-only queries on a window handle (which may be gone: they then return zeros).
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let parent = GetAncestor(hwnd, GA_PARENT);
            Self {
                pid,
                parent: if parent == GetDesktopWindow() {
                    HWND::default()
                } else {
                    parent
                },
                owned: GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()),
                style: GetWindowLongPtrW(hwnd, GWL_STYLE) as u32,
                ex_style: GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32,
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// Not the app's main window.
    Ignore,
    /// The app already made its window a child of the screen window
    /// (it honored the parent argument): just size it.
    Embedded,
    /// A top-level window to move into the screen window.
    Reparent,
}

/// Decides whether a window shown by process `pid` is its main window.
/// Top-level windows must be visible, unowned and not tool windows, which
/// skips the hidden helper windows every GUI thread creates (IME, tooltips).
fn classify(window: &WindowFacts, pid: u32, target: HWND) -> Verdict {
    let helper = window.owned
        || window.ex_style & WS_EX_TOOLWINDOW.0 != 0
        || window.style & WS_VISIBLE.0 == 0;
    if window.pid != pid {
        Verdict::Ignore
    } else if window.parent == target {
        Verdict::Embedded
    } else if !window.parent.is_invalid() || helper {
        // A child of one of the app's own windows, or a helper window.
        Verdict::Ignore
    } else {
        Verdict::Reparent
    }
}

/// A top-level window's style as a borderless, visible child.
fn embedded_style(style: u32) -> u32 {
    let remove = WS_POPUP
        | WS_CAPTION
        | WS_BORDER
        | WS_DLGFRAME
        | WS_THICKFRAME
        | WS_SYSMENU
        | WS_MINIMIZEBOX
        | WS_MAXIMIZEBOX
        | WS_MINIMIZE
        | WS_MAXIMIZE;
    (style & !remove.0) | WS_CHILD.0 | WS_VISIBLE.0
}

/// Extended style without frame decorations, and never activating.
fn embedded_ex_style(ex_style: u32) -> u32 {
    let remove = WS_EX_APPWINDOW
        | WS_EX_WINDOWEDGE
        | WS_EX_CLIENTEDGE
        | WS_EX_DLGMODALFRAME
        | WS_EX_STATICEDGE
        | WS_EX_TOOLWINDOW;
    (ex_style & !remove.0) | WS_EX_NOACTIVATE.0
}

/// Turns a top-level window into a borderless child of `target`.
fn embed(window: HWND, target: HWND) {
    // SAFETY: plain window calls. Per SetParent's contract, a window moving
    // under a parent gets WS_CHILD (and loses WS_POPUP) before the move.
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
        SetWindowLongPtrW(window, GWL_STYLE, embedded_style(style) as isize);
        let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(window, GWL_EXSTYLE, embedded_ex_style(ex_style) as isize);
        let _ = SetParent(window, Some(target));
    }
}

/// Sizes `window` to `target`'s client area. Asynchronous, so a busy app
/// cannot block this thread.
fn fill(window: HWND, target: HWND) {
    let mut rect = RECT::default();
    // SAFETY: plain window calls; `rect` outlives the call.
    unsafe {
        let _ = GetClientRect(target, &mut rect);
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
        );
    }
}

// --- Arguments ------------------------------------------------------------

/// Replaces `{hwnd}` in each argument with the decimal window handle.
fn substitute_hwnd(args: &[String], hwnd: usize) -> Vec<String> {
    args.iter()
        .map(|arg| arg.replace("{hwnd}", &hwnd.to_string()))
        .collect()
}

/// A Windows command line for `exe` and `args`, quoted so the app's own
/// argument parser (MSVCRT rules) recovers each argument unchanged.
fn command_line(exe: &Path, args: &[String]) -> String {
    std::iter::once(exe.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|arg| quote_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote are doubled, and the quote escaped.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Trailing backslashes would escape the closing quote: double them.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hwnd(n: usize) -> HWND {
        HWND(n as *mut c_void)
    }

    #[test]
    fn hwnd_is_substituted_as_decimal_everywhere_in_an_argument() {
        let args = ["-parentHWND", "{hwnd}", "--p={hwnd}:{hwnd}", "delayed"].map(String::from);
        assert_eq!(
            substitute_hwnd(&args, 197_122),
            ["-parentHWND", "197122", "--p=197122:197122", "delayed"]
        );
    }

    #[test]
    fn arguments_are_quoted_for_the_msvcrt_parser() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(
            command_line(
                Path::new(r"C:\My Game\game.exe"),
                &["-x".into(), "1 2".into()]
            ),
            r#""C:\My Game\game.exe" -x "1 2""#
        );
    }

    fn facts(pid: u32, parent: usize, owned: bool, style: u32, ex_style: u32) -> WindowFacts {
        WindowFacts {
            pid,
            parent: hwnd(parent),
            owned,
            style,
            ex_style,
        }
    }

    #[test]
    fn main_window_is_a_visible_unowned_top_level_window_of_the_process() {
        let target = hwnd(100);
        let visible = WS_VISIBLE.0;
        let main = facts(7, 0, false, visible, 0);
        assert_eq!(classify(&main, 7, target), Verdict::Reparent);

        // Other processes, hidden helpers, owned popups and tool windows are skipped.
        assert_eq!(
            classify(&facts(8, 0, false, visible, 0), 7, target),
            Verdict::Ignore
        );
        assert_eq!(
            classify(&facts(7, 0, false, 0, 0), 7, target),
            Verdict::Ignore
        );
        assert_eq!(
            classify(&facts(7, 0, true, visible, 0), 7, target),
            Verdict::Ignore
        );
        let tool = WS_EX_TOOLWINDOW.0;
        assert_eq!(
            classify(&facts(7, 0, false, visible, tool), 7, target),
            Verdict::Ignore
        );
    }

    #[test]
    fn windows_already_under_the_screen_are_kept_and_deeper_children_ignored() {
        let target = hwnd(100);
        assert_eq!(
            classify(&facts(7, 100, false, 0, 0), 7, target),
            Verdict::Embedded
        );
        assert_eq!(
            classify(&facts(7, 55, false, WS_VISIBLE.0, 0), 7, target),
            Verdict::Ignore
        );
        assert_eq!(
            classify(&facts(8, 100, false, 0, 0), 7, target),
            Verdict::Ignore
        );
    }

    #[test]
    fn embedding_strips_decorations_and_never_activates() {
        let top = (WS_OVERLAPPEDWINDOW_BITS | WS_VISIBLE.0) & !WS_CHILD.0;
        let child = embedded_style(top);
        assert_eq!(child & WS_CAPTION.0, 0);
        assert_eq!(child & WS_THICKFRAME.0, 0);
        assert_eq!(child & WS_SYSMENU.0, 0);
        assert_ne!(child & WS_CHILD.0, 0);
        assert_ne!(child & WS_VISIBLE.0, 0);

        let ex = embedded_ex_style(WS_EX_APPWINDOW.0 | WS_EX_WINDOWEDGE.0);
        assert_eq!(ex & (WS_EX_APPWINDOW.0 | WS_EX_WINDOWEDGE.0), 0);
        assert_ne!(ex & WS_EX_NOACTIVATE.0, 0);
    }

    /// `WS_OVERLAPPEDWINDOW`: caption, system menu, thick frame, min/max boxes.
    const WS_OVERLAPPEDWINDOW_BITS: u32 =
        WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0;
}
