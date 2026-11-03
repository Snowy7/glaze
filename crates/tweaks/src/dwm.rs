//! Per-window styling with `DwmSetWindowAttribute`: border, title bar,
//! corners, backdrop and dark title bar. Public API only, and no code runs
//! inside other processes.
//!
//! A window is touched only if it is top-level, visible, captioned and its
//! process passes the injection allowlist (`mypaper_inject::evaluate`, with
//! the signature check). Existing windows are styled when the [`Styler`]
//! starts; new ones through an out-of-context `EVENT_OBJECT_SHOW` hook, so
//! nothing polls. Dropping the `Styler` puts every touched window back.
//!
//! WinEvent callbacks arrive through the message loop of the thread that
//! created the `Styler`, which therefore must pump messages (core does).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use mypaper_inject::signature::microsoft_signed;
use mypaper_inject::{Target, Verdict, evaluate};
use mypaper_theme::{Backdrop, Corners, WindowsStyle};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_AUTO, DWMSBT_MAINWINDOW,
    DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWA_BORDER_COLOR,
    DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWCP_ROUNDSMALL, DWMWINDOWATTRIBUTE, DwmGetWindowAttribute,
    DwmSetWindowAttribute,
};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_SHOW, EnumWindows, GA_PARENT, GWL_STYLE, GetAncestor,
    GetDesktopWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_CAPTION,
};
use windows::core::{BOOL, PWSTR};

use crate::color::colorref;
use crate::palette::Palette;
use crate::plan::WindowStyle;

thread_local! {
    /// What the WinEvent callback needs. Callbacks run on the thread that
    /// installed the hook, so no lock is required.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct State {
    /// The theme's section as written: says which attributes are ours to restore.
    raw: WindowsStyle,
    style: WindowStyle,
    windows_dir: PathBuf,
    /// Every window we styled, with the dark-mode value it had before.
    originals: HashMap<isize, bool>,
}

/// Styles allowlisted windows while alive; restores them on drop.
pub struct Styler {
    hook: HWINEVENTHOOK,
}

impl Styler {
    /// Styles all existing windows and starts following new ones. Only one
    /// `Styler` can exist per thread.
    pub fn start(raw: &WindowsStyle, palette: Option<&Palette>) -> Result<Self> {
        let state = State {
            raw: raw.clone(),
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
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(on_show),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            STATE.with_borrow_mut(Option::take);
            bail!("SetWinEventHook failed");
        }
        let styler = Self { hook };
        sweep();
        Ok(styler)
    }

    /// Re-applies the style with a new palette (an `"auto"` color changed).
    pub fn restyle(&self, palette: Option<&Palette>) {
        with_state(|s| s.style = WindowStyle::resolve(&s.raw, palette));
        sweep();
    }
}

impl Drop for Styler {
    fn drop(&mut self) {
        // SAFETY: the hook came from `SetWinEventHook` in `start`.
        let _ = unsafe { UnhookWinEvent(self.hook) };
        let Some(state) = STATE.with_borrow_mut(Option::take) else {
            return;
        };
        let mut restored = 0;
        for (&key, &dark) in &state.originals {
            let hwnd = hwnd_from(key);
            // The handle may have been reused by a window that is not ours
            // to touch, so check again before writing.
            // SAFETY: `IsWindow` accepts any handle value.
            if unsafe { IsWindow(Some(hwnd)) }.as_bool()
                && process_allowed(hwnd, &state.windows_dir)
            {
                reset(hwnd, &state.raw, dark);
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
        border: Some(mypaper_theme::Paint::Auto),
        caption: Some(mypaper_theme::Color([0; 3])),
        caption_text: Some(mypaper_theme::Color([0; 3])),
        corners: Some(Corners::Default),
        backdrop: Some(Backdrop::Auto),
        dark_mode: dark,
        ..WindowsStyle::default()
    };
    for_each_window(|hwnd| {
        if is_candidate(hwnd) && process_allowed(hwnd, &windows_dir) {
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
        if !is_candidate(hwnd) || !process_allowed(hwnd, &self.windows_dir) {
            return false;
        }
        // Remember the dark-mode value from before our first change only.
        self.originals
            .entry(hwnd.0 as isize)
            .or_insert_with(|| read_dark(hwnd));
        apply(hwnd, &self.style);
        true
    }
}

/// Styles every existing window.
fn sweep() {
    let mut styled = 0;
    for_each_window(|hwnd| styled += usize::from(with_state(|s| s.consider(hwnd)) == Some(true)));
    log::info!("styled {styled} window(s)");
}

unsafe extern "system" fn on_show(
    _: HWINEVENTHOOK,
    _: u32,
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
        s.consider(hwnd);
        // Forget windows that are gone, so the map does not grow forever.
        // SAFETY: `IsWindow` accepts any handle value.
        s.originals
            .retain(|&key, _| unsafe { IsWindow(Some(hwnd_from(key))) }.as_bool());
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

/// Top-level, visible and captioned: the windows that have a title bar to style.
fn is_candidate(hwnd: HWND) -> bool {
    // SAFETY: plain queries on a window handle; stale handles just fail.
    unsafe {
        let caption = WS_CAPTION.0;
        IsWindowVisible(hwnd).as_bool()
            && GetAncestor(hwnd, GA_PARENT) == GetDesktopWindow()
            && GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & caption == caption
    }
}

/// Does the window's process pass the injection allowlist?
fn process_allowed(hwnd: HWND, windows_dir: &Path) -> bool {
    let mut pid = 0;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: opens a query-only handle, closed by `Process`'s drop.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let process = Process(handle);
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
fn set<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: T) {
    // SAFETY: `value` is a plain value and its size is passed with it.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast::<c_void>(),
            size_of::<T>() as u32,
        )
    };
    if let Err(e) = result {
        log::trace!("DWM attribute {} on {hwnd:?}: {e}", attribute.0);
    }
}

fn read_dark(hwnd: HWND) -> bool {
    let mut dark = BOOL::default();
    // SAFETY: `dark` is a BOOL, the size this attribute uses.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw mut dark).cast::<c_void>(),
            size_of::<BOOL>() as u32,
        )
    };
    read.is_ok() && dark.as_bool()
}

fn apply(hwnd: HWND, style: &WindowStyle) {
    if let Some(color) = style.border {
        set(hwnd, DWMWA_BORDER_COLOR, colorref(color));
    }
    if let Some(color) = style.caption {
        set(hwnd, DWMWA_CAPTION_COLOR, colorref(color));
    }
    if let Some(color) = style.caption_text {
        set(hwnd, DWMWA_TEXT_COLOR, colorref(color));
    }
    if let Some(corners) = style.corners {
        set(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            corner_preference(corners),
        );
    }
    if let Some(backdrop) = style.backdrop {
        set(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, backdrop_type(backdrop));
    }
    if let Some(dark) = style.dark_mode {
        set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, BOOL::from(dark));
    }
}

/// Puts back the defaults for the attributes `raw` sets, and the dark-mode
/// value the window had before.
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
