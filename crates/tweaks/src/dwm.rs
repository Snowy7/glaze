//! Per-window styling with `DwmSetWindowAttribute`: border, title bar,
//! corners, backdrop and dark title bar. Public API only, and no code runs
//! inside other processes.
//!
//! A visible app frame gets full styling; floating surfaces get dark mode only.
//! Overlays and transparent indicators are untouched. A window must pass the
//! process injection allowlist (`glaze_inject::evaluate`, with
//! the signature check). Existing windows are styled when the [`Styler`]
//! starts; WinEvents follow new windows and frame changes. A slow timer checks
//! only tracked windows for style changes that emit no WinEvent. Dropping the
//! `Styler` puts every readable attribute back exactly as it was.
//!
//! WinEvent callbacks arrive through the message loop of the thread that
//! created the `Styler`, which therefore must pump messages (core does).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use glaze_inject::signature::microsoft_signed;
use glaze_inject::{Target, Verdict, evaluate};
use glaze_theme::{Backdrop, Corners, WindowsStyle};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_AUTO, DWMSBT_MAINWINDOW,
    DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWA_BORDER_COLOR,
    DWMWA_CAPTION_COLOR, DWMWA_CLOAKED, DWMWA_COLOR_DEFAULT, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DEFAULT, DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWCP_ROUNDSMALL, DWMWINDOWATTRIBUTE,
    DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, EVENT_OBJECT_STATECHANGE,
    EVENT_OBJECT_UNCLOAKED, EnumWindows, GA_PARENT, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetAncestor,
    GetDesktopWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, KillTimer, OBJID_WINDOW, SetTimer, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};
use windows::core::{BOOL, PWSTR};

use crate::color::colorref;
use crate::palette::Palette;
use crate::plan::{Change, WindowStyle};

thread_local! {
    /// What the WinEvent callback needs. Callbacks run on the thread that
    /// installed the hook, so no lock is required.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static ALLOWLIST: RefCell<ProcessCache> = RefCell::new(ProcessCache::default());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProcessKey {
    pid: u32,
    started: u64,
}

#[derive(Default)]
struct ProcessCache(HashMap<u32, (u64, bool)>);

impl ProcessCache {
    fn verdict(&mut self, key: ProcessKey, evaluate: impl FnOnce() -> bool) -> bool {
        if let Some(&(started, allowed)) = self.0.get(&key.pid)
            && started == key.started
        {
            return allowed;
        }
        let allowed = evaluate();
        self.0.insert(key.pid, (key.started, allowed));
        allowed
    }
}

struct Tracked {
    process: ProcessKey,
    original: Originals,
    applied: WindowStyle,
}

/// Values are captured before each attribute's first write, including failed writes.
/// Every attribute we manage has a four-byte DWM representation.
#[derive(Default)]
struct Originals {
    border: Option<u32>,
    caption: Option<u32>,
    caption_text: Option<u32>,
    corners: Option<u32>,
    backdrop: Option<u32>,
    dark: Option<u32>,
}

struct State {
    style: WindowStyle,
    windows_dir: PathBuf,
    /// Every window we styled, with its original attribute values.
    originals: HashMap<isize, Tracked>,
}

/// Styles allowlisted windows while alive; restores them on drop.
pub struct Styler {
    hook: HWINEVENTHOOK,
    timer: usize,
}

impl Styler {
    /// Styles all existing windows and starts following new ones. Only one
    /// `Styler` can exist per thread.
    pub fn start(raw: &WindowsStyle, palette: Option<&Palette>) -> Result<Self> {
        let state = State {
            style: WindowStyle::resolve(raw, palette),
            windows_dir: windows_dir()?,
            originals: HashMap::new(),
        };
        let installed = STATE.with_borrow_mut(|slot| {
            let free = slot.is_none();
            if free {
                *slot = Some(state);
            }
            free
        });
        if !installed {
            bail!("a window styler is already running on this thread");
        }
        // SAFETY: out-of-context hook with a 'static callback; the hook is
        // removed in `drop`.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_UNCLOAKED,
                None,
                Some(on_window_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            STATE.with_borrow_mut(Option::take);
            bail!("SetWinEventHook failed");
        }
        // SetWindowLongPtr does not guarantee an accessibility event. Check only
        // already-managed handles, without enumerating apps or rewriting attributes.
        // SAFETY: a thread timer with a static callback, removed before state is dropped.
        let timer = unsafe { SetTimer(None, 0, 1000, Some(recheck_tracked)) };
        let styler = Self { hook, timer };
        if timer == 0 {
            bail!("SetTimer failed for window-style revalidation");
        }
        sweep();
        Ok(styler)
    }

    /// Updates attributes in place, retaining each attribute's first original value.
    pub fn update(&self, raw: &WindowsStyle, palette: Option<&Palette>) {
        with_state(|s| {
            s.style = WindowStyle::resolve(raw, palette);
        });
        sweep();
    }
}

impl Drop for Styler {
    fn drop(&mut self) {
        // SAFETY: the hook came from `SetWinEventHook` in `start`.
        let _ = unsafe { UnhookWinEvent(self.hook) };
        if self.timer != 0 {
            // SAFETY: this timer belongs to this thread and this Styler.
            let _ = unsafe { KillTimer(None, self.timer) };
        }
        let Some(mut state) = STATE.with_borrow_mut(Option::take) else {
            return;
        };
        let mut restored = 0;
        for (&key, tracked) in &mut state.originals {
            let hwnd = hwnd_from(key);
            // The handle may have been reused by a window that is not ours
            // to touch, so check again before writing.
            // SAFETY: `IsWindow` accepts any handle value.
            if unsafe { IsWindow(Some(hwnd)) }.as_bool()
                && allowed_process(hwnd, &state.windows_dir) == Some(tracked.process)
            {
                apply_transition(
                    hwnd,
                    tracked.applied,
                    WindowStyle::default(),
                    &mut tracked.original,
                );
                restored += 1;
            }
        }
        log::info!("restored {restored} window(s)");
    }
}

/// Crash recovery: puts the border, title bar, corner and backdrop of every
/// allowlisted window back to its default. A previous run died before it
/// could restore, so the per-window dark-mode values from before are lost:
/// `dark` is `Some(system setting)` if that run forced dark mode (windows of
/// Windows' own apps follow the system theme), or `None` to leave it alone.
pub fn reset_all(dark: Option<bool>) {
    let Ok(windows_dir) = windows_dir() else {
        return;
    };
    let everything = WindowsStyle {
        border: Some(glaze_theme::Paint::Auto),
        caption: Some(glaze_theme::Color([0; 3])),
        caption_text: Some(glaze_theme::Color([0; 3])),
        corners: Some(Corners::Default),
        backdrop: Some(Backdrop::Auto),
        dark_mode: dark,
        ..WindowsStyle::default()
    };
    for_each_window(|hwnd| {
        if window_class(hwnd) == WindowClass::AppFrame && process_allowed(hwnd, &windows_dir) {
            reset(hwnd, &everything, dark.unwrap_or_default());
        }
    });
}

/// Runs `f` on the styler's state, unless there is none or it is busy
/// (a callback fired while we were inside `f`).
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|slot| {
        let mut slot = slot.try_borrow_mut().ok()?;
        slot.as_mut().map(f)
    })
}

impl State {
    /// Styles the window if it is eligible; returns whether it was.
    fn consider(&mut self, hwnd: HWND) -> bool {
        let key = hwnd.0 as isize;
        let class = window_class(hwnd);
        if class == WindowClass::Untouched && !self.originals.contains_key(&key) {
            return false;
        }
        let process = allowed_process(hwnd, &self.windows_dir);
        if self
            .originals
            .get(&key)
            .is_some_and(|t| Some(t.process) != process)
        {
            self.originals.remove(&key); // A dead or reused handle is no longer ours.
        }
        if class == WindowClass::Untouched {
            if let Some(tracked) = self.originals.get_mut(&key) {
                tracked.applied = apply_transition(
                    hwnd,
                    tracked.applied,
                    WindowStyle::default(),
                    &mut tracked.original,
                );
                if tracked.applied == WindowStyle::default() {
                    self.originals.remove(&key);
                }
            }
            return false;
        }
        let Some(process) = process else {
            return false;
        };
        if let std::collections::hash_map::Entry::Vacant(entry) = self.originals.entry(key) {
            let backdrop = read(hwnd, DWMWA_SYSTEMBACKDROP_TYPE);
            if !unmanaged_backdrop(backdrop) {
                return false; // The app owns its material; never mistake ours for its original.
            }
            entry.insert(Tracked {
                process,
                original: Originals {
                    backdrop,
                    ..Originals::default()
                },
                applied: WindowStyle::default(),
            });
        }
        let tracked = self.originals.get_mut(&key).expect("inserted above");
        tracked.applied = apply_transition(
            hwnd,
            tracked.applied,
            class.style(self.style),
            &mut tracked.original,
        );
        true
    }
}

/// Styles every existing window.
fn sweep() {
    let mut styled = 0;
    for_each_window(|hwnd| styled += usize::from(with_state(|s| s.consider(hwnd)) == Some(true)));
    log::info!("styled {styled} window(s)");
}

unsafe extern "system" fn on_window_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // Child windows and sub-objects fire this too; only top-level windows matter.
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || hwnd.is_invalid() {
        return;
    }
    with_state(|s| {
        if event == EVENT_OBJECT_DESTROY {
            s.originals.remove(&(hwnd.0 as isize));
            return;
        }
        if matches!(
            event,
            EVENT_OBJECT_SHOW
                | EVENT_OBJECT_HIDE
                | EVENT_OBJECT_STATECHANGE
                | EVENT_OBJECT_LOCATIONCHANGE
                | EVENT_OBJECT_CLOAKED
                | EVENT_OBJECT_UNCLOAKED
        ) {
            s.consider(hwnd);
        }
    });
}

unsafe extern "system" fn recheck_tracked(_: HWND, _: u32, _: usize, _: u32) {
    with_state(|s| {
        let handles: Vec<_> = s.originals.keys().copied().collect();
        for key in handles {
            s.consider(hwnd_from(key));
        }
    });
}

fn hwnd_from(key: isize) -> HWND {
    HWND(key as *mut c_void)
}

fn for_each_window(mut f: impl FnMut(HWND)) {
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `&mut &mut dyn FnMut` passed below, which
        // outlives the whole `EnumWindows` call.
        let f = unsafe { &mut *(lparam.0 as *mut &mut dyn FnMut(HWND)) };
        f(hwnd);
        true.into()
    }
    let mut f: &mut dyn FnMut(HWND) = &mut f;
    // SAFETY: see `visit`; the pointer is to a local that lives until return.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut f as isize)) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowClass {
    AppFrame,
    Floating,
    Untouched,
}

impl WindowClass {
    fn style(self, requested: WindowStyle) -> WindowStyle {
        match self {
            Self::AppFrame => requested,
            Self::Floating => WindowStyle {
                dark_mode: requested.dark_mode,
                ..Default::default()
            },
            Self::Untouched => WindowStyle::default(),
        }
    }
}

/// Pure classification: transparent indicators and full-monitor overlays must
/// never receive writes. Floating shapes may follow the scheme, but DWM must
/// not paint their enclosing rectangle. NOREDIRECTIONBITMAP alone is fine.
fn classify_window(
    style: u32,
    ex: u32,
    owned: bool,
    cloaked: bool,
    covers_monitor: bool,
) -> WindowClass {
    if style & WS_CHILD.0 != 0
        || cloaked
        || ex & (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) != 0
        || (covers_monitor && ex & WS_EX_TOPMOST.0 != 0)
    {
        return WindowClass::Untouched;
    }
    let app_frame = ex & WS_EX_APPWINDOW.0 != 0
        || (style & (WS_THICKFRAME.0 | WS_SYSMENU.0) == WS_THICKFRAME.0 | WS_SYSMENU.0
            && style & (WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) != 0);
    if style & WS_CAPTION.0 == WS_CAPTION.0
        && ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) == 0
        && (!(owned || style & WS_POPUP.0 != 0) || app_frame)
    {
        WindowClass::AppFrame
    } else {
        WindowClass::Floating
    }
}

fn unmanaged_backdrop(original: Option<u32>) -> bool {
    original.is_none_or(|v| v == DWMSBT_AUTO.0 as u32)
}

fn window_class(hwnd: HWND) -> WindowClass {
    // SAFETY: plain queries on a window handle; stale handles just fail.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetAncestor(hwnd, GA_PARENT) != GetDesktopWindow() {
            return WindowClass::Untouched;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mut covers_monitor = false;
        if ex & WS_EX_TOPMOST.0 != 0 {
            let mut rect = RECT::default();
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            covers_monitor = GetWindowRect(hwnd, &mut rect).is_ok()
                && GetMonitorInfoW(
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                    &mut monitor,
                )
                .as_bool()
                && rect.left <= monitor.rcMonitor.left
                && rect.top <= monitor.rcMonitor.top
                && rect.right >= monitor.rcMonitor.right
                && rect.bottom >= monitor.rcMonitor.bottom;
        }
        classify_window(
            GetWindowLongPtrW(hwnd, GWL_STYLE) as u32,
            ex,
            GetWindow(hwnd, GW_OWNER).is_ok_and(|h| !h.is_invalid()),
            read(hwnd, DWMWA_CLOAKED).is_some_and(|v| v != 0),
            covers_monitor,
        )
    }
}
/// Does the window's process pass the injection allowlist?
fn process_allowed(hwnd: HWND, windows_dir: &Path) -> bool {
    allowed_process(hwnd, windows_dir).is_some()
}

fn allowed_process(hwnd: HWND, windows_dir: &Path) -> Option<ProcessKey> {
    let mut pid = 0;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: opens a query-only handle, closed by `Process`'s drop.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return None;
    };
    let process = Process(handle);
    let key = ProcessKey {
        pid,
        started: process.started()?,
    };
    ALLOWLIST
        .with_borrow_mut(|cache| cache.verdict(key, || evaluate_process(&process, windows_dir)))
        .then_some(key)
}

fn evaluate_process(process: &Process, windows_dir: &Path) -> bool {
    let Some(image) = process.image() else {
        return false;
    };
    let family = process.package_family();
    let mut target = Target {
        image: &image,
        package_family: family.as_deref(),
        // Optimistic first: if the allowlist says no even then, skip the
        // signature check, which hashes the whole executable.
        microsoft_signed: true,
    };
    if evaluate(&target, windows_dir) != Verdict::Allow {
        return false;
    }
    target.microsoft_signed = microsoft_signed(&image);
    evaluate(&target, windows_dir) == Verdict::Allow
}

/// A process handle, closed on drop.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by us and is closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Process {
    fn started(&self) -> Option<u64> {
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: query-only handle and four valid output FILETIME pointers.
        unsafe { GetProcessTimes(self.0, &mut created, &mut exit, &mut kernel, &mut user) }.ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    fn image(&self) -> Option<PathBuf> {
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer and its capacity in `len` describe the same memory.
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        }
        .ok()?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
    }

    /// The package family name, or `None` for an unpackaged process.
    fn package_family(&self) -> Option<String> {
        let mut buffer = [0u16; 128];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer and its capacity in `len` describe the same memory.
        let status =
            unsafe { GetPackageFamilyName(self.0, &mut len, Some(PWSTR(buffer.as_mut_ptr()))) };
        status
            .is_ok()
            .then(|| String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)]))
    }
}

fn windows_dir() -> Result<PathBuf> {
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is passed with its length.
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len > buffer.len() {
        bail!("GetWindowsDirectoryW failed");
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
}

/// Sets one DWM attribute. Failures are expected (older Windows builds lack
/// the newer attributes) and not worth more than a trace line.
fn set<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: T) -> bool {
    // SAFETY: `value` is a plain value and its size is passed with it.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast::<c_void>(),
            size_of::<T>() as u32,
        )
    };
    if let Err(e) = &result {
        if attribute == DWMWA_WINDOW_CORNER_PREFERENCE {
            log::warn!("DWM corner preference on {hwnd:?} was rejected: {e}");
        } else {
            log::trace!("DWM attribute {} on {hwnd:?}: {e}", attribute.0);
        }
    }
    result.is_ok()
}

/// DWM uses four-byte values for every attribute managed here. Some color
/// attributes are write-only on Windows builds; only those use a default.
fn read(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE) -> Option<u32> {
    let mut value = 0u32;
    // SAFETY: a correctly sized, initialized four-byte output buffer.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            attribute,
            (&raw mut value).cast(),
            size_of::<u32>() as u32,
        )
    }
    .ok()
    .map(|()| value)
}

/// Capture once, write only a changed field, and remember only successful writes.
fn update_attribute<T: Copy>(
    field: &mut Option<T>,
    next: Option<T>,
    change: Change<T>,
    original: &mut Option<u32>,
    fallback: u32,
    encode: impl FnOnce(T) -> u32,
    mut io: impl FnMut(Option<u32>) -> Option<u32>,
) {
    if matches!(change, Change::Keep) {
        return;
    }
    let saved = *original.get_or_insert_with(|| io(None).unwrap_or(fallback));
    let value = next.map_or(saved, encode);
    if io(Some(value)).is_some() {
        *field = next;
    }
}

fn apply_transition(
    hwnd: HWND,
    mut before: WindowStyle,
    next: WindowStyle,
    original: &mut Originals,
) -> WindowStyle {
    let delta = before.transition_to(next);
    macro_rules! attribute {
        ($field:ident, $saved:ident, $attribute:ident, $fallback:expr, $encode:expr) => {
            update_attribute(
                &mut before.$field,
                next.$field,
                delta.$field,
                &mut original.$saved,
                $fallback,
                $encode,
                |value| match value {
                    None => read(hwnd, $attribute),
                    Some(value) => set(hwnd, $attribute, value).then_some(value),
                },
            );
        };
    }
    attribute!(
        border,
        border,
        DWMWA_BORDER_COLOR,
        DWMWA_COLOR_DEFAULT,
        colorref
    );
    attribute!(
        caption,
        caption,
        DWMWA_CAPTION_COLOR,
        DWMWA_COLOR_DEFAULT,
        colorref
    );
    attribute!(
        caption_text,
        caption_text,
        DWMWA_TEXT_COLOR,
        DWMWA_COLOR_DEFAULT,
        colorref
    );
    attribute!(
        corners,
        corners,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DEFAULT.0 as u32,
        |v| corner_preference(v).0 as u32
    );
    attribute!(
        backdrop,
        backdrop,
        DWMWA_SYSTEMBACKDROP_TYPE,
        DWMSBT_AUTO.0 as u32,
        |v| backdrop_type(v).0 as u32
    );
    attribute!(dark_mode, dark, DWMWA_USE_IMMERSIVE_DARK_MODE, 0, u32::from);
    before
}
/// Crash recovery only: live transitions use the recorded originals instead.
fn reset(hwnd: HWND, raw: &WindowsStyle, dark: bool) {
    if raw.border.is_some() {
        set(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT);
    }
    if raw.caption.is_some() {
        set(hwnd, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT);
    }
    if raw.caption_text.is_some() {
        set(hwnd, DWMWA_TEXT_COLOR, DWMWA_COLOR_DEFAULT);
    }
    if raw.corners.is_some() {
        set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT);
    }
    if raw.backdrop.is_some() {
        set(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_AUTO);
    }
    if raw.dark_mode.is_some() {
        set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, BOOL::from(dark));
    }
}

fn corner_preference(corners: Corners) -> DWM_WINDOW_CORNER_PREFERENCE {
    match corners {
        Corners::Default => DWMWCP_DEFAULT,
        Corners::Round => DWMWCP_ROUND,
        Corners::Small => DWMWCP_ROUNDSMALL,
        Corners::Square => DWMWCP_DONOTROUND,
    }
}

fn backdrop_type(backdrop: Backdrop) -> DWM_SYSTEMBACKDROP_TYPE {
    match backdrop {
        Backdrop::Auto => DWMSBT_AUTO,
        Backdrop::None => DWMSBT_NONE,
        Backdrop::Mica => DWMSBT_MAINWINDOW,
        Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
        Backdrop::Tabbed => DWMSBT_TABBEDWINDOW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_overlays_and_see_through_indicators_are_untouched() {
        let frame = WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0;
        for (style, ex, cloaked, fullscreen) in [
            (frame, 0x801a8, false, false),
            (frame, 0x200188, false, true),
            (frame, WS_EX_LAYERED.0, false, false),
            (frame, WS_EX_TRANSPARENT.0, false, false),
            (frame | WS_CHILD.0, 0, false, false),
            (frame, 0, true, false),
        ] {
            assert_eq!(
                classify_window(style, ex, false, cloaked, fullscreen),
                WindowClass::Untouched
            );
        }
    }

    #[test]
    fn floating_toolbars_popups_and_captionless_notifications_only_follow_dark_mode() {
        for (style, ex, owned) in [
            (WS_CAPTION.0 | WS_POPUP.0, 0x200108, true),
            (WS_CAPTION.0, WS_EX_TOOLWINDOW.0, false),
            (0, 0x200188, false),
            (0, 0x8200008, false),
            (WS_CAPTION.0, 0, true),
        ] {
            let class = classify_window(style, ex, owned, false, false);
            assert_eq!(class, WindowClass::Floating);
            let requested = WindowStyle {
                border: Some(glaze_theme::Color([2; 3])),
                caption: Some(glaze_theme::Color([3; 3])),
                caption_text: Some(glaze_theme::Color([4; 3])),
                corners: Some(Corners::Round),
                backdrop: Some(Backdrop::Mica),
                dark_mode: Some(true),
            };
            let safe = class.style(requested);
            assert_eq!(
                safe,
                WindowStyle {
                    dark_mode: Some(true),
                    ..Default::default()
                }
            );
            let released = requested.transition_to(safe);
            assert_eq!(released.backdrop, Change::Reset);
            assert_eq!(released.corners, Change::Reset);
            assert_eq!(released.border, Change::Reset);
            assert_eq!(released.caption, Change::Reset);
            assert_eq!(released.caption_text, Change::Reset);
            assert_eq!(released.dark_mode, Change::Keep);
        }
    }

    #[test]
    fn normal_frames_keep_full_styling_including_winui_no_redirection_frames() {
        let frame = WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0;
        for ex in [0, WS_EX_APPWINDOW.0, 0x200100] {
            for popup in [0, WS_POPUP.0] {
                assert_eq!(
                    classify_window(frame | popup, ex, false, false, false),
                    WindowClass::AppFrame
                );
            }
        }
        assert_eq!(
            classify_window(frame, 0, true, false, false),
            WindowClass::AppFrame
        );
        let requested = WindowStyle {
            corners: Some(Corners::Round),
            backdrop: Some(Backdrop::Mica),
            ..Default::default()
        };
        assert_eq!(WindowClass::AppFrame.style(requested), requested);
        assert_eq!(
            WindowClass::Untouched.style(requested),
            WindowStyle::default()
        );
    }
    #[test]
    fn explicit_app_materials_are_preserved_including_none_and_unknown_values() {
        assert!(unmanaged_backdrop(None));
        assert!(unmanaged_backdrop(Some(0)));
        for explicit in [1, 2, 3, 4, 99] {
            assert!(!unmanaged_backdrop(Some(explicit)));
        }
    }

    #[test]
    fn releasing_attributes_restores_the_first_readable_value_exactly() {
        // Exercise each DWM representation, including non-default corner/dark values
        // and the color sentinel. Repeated themes must not replace the original.
        for initial in [0x123456, 0xfffffffe, 1, 3, 4] {
            let mut field = None;
            let mut original = None;
            let mut actual = initial;
            let mut reads = 0;
            for next in [Some(7), Some(9), None] {
                let change = match next {
                    Some(v) => Change::Set(v),
                    None => Change::Reset,
                };
                update_attribute(
                    &mut field,
                    next,
                    change,
                    &mut original,
                    0,
                    |v| v,
                    |value| match value {
                        None => {
                            reads += 1;
                            Some(actual)
                        }
                        Some(value) => {
                            actual = value;
                            Some(value)
                        }
                    },
                );
                assert_eq!(field, next);
            }
            assert_eq!(actual, initial);
            assert_eq!(reads, 1);
        }
    }

    #[test]
    fn unreadable_attributes_use_defaults_and_failed_restores_remain_pending() {
        let mut field = None;
        let mut original = None;
        let mut writes = Vec::new();
        update_attribute(
            &mut field,
            Some(2),
            Change::Set(2),
            &mut original,
            17,
            |v| v,
            |value| {
                if let Some(v) = value {
                    writes.push(v);
                    value
                } else {
                    None
                }
            },
        );
        assert_eq!(original, Some(17));
        update_attribute(
            &mut field,
            None,
            Change::Reset,
            &mut original,
            17,
            |v| v,
            |_| None,
        );
        assert_eq!(field, Some(2), "a failed restore must be retried");
        update_attribute(
            &mut field,
            None,
            Change::Reset,
            &mut original,
            17,
            |v| v,
            |value| {
                writes.push(value.unwrap());
                value
            },
        );
        assert_eq!(field, None);
        assert_eq!(writes, [2, 17]);
        update_attribute(
            &mut field,
            None,
            Change::Keep,
            &mut original,
            17,
            |v| v,
            |_| panic!("unchanged attributes perform no IO"),
        );
    }

    #[test]
    fn process_verdict_is_cached_once_and_pid_reuse_is_rechecked() {
        let mut cache = ProcessCache::default();
        let process = ProcessKey {
            pid: 7,
            started: 100,
        };
        assert!(cache.verdict(process, || true));
        assert!(cache.verdict(process, || panic!("must not hash this executable twice")));
        let reused = ProcessKey {
            started: 200,
            ..process
        };
        assert!(!cache.verdict(reused, || false));
        assert!(!cache.verdict(reused, || panic!("negative verdicts are cached too")));
    }
}
