//! Native windows the wallpaper renders into, and the frame-pacing waiter.

use std::time::Instant;

use anyhow::{Context, Result};
use glaze_desktop::{DesktopHost, EXIT_RESTART, Rect};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer,
    TIMER_ALL_ACCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetWindowLongPtrW, IDC_ARROW, IsIconic, LoadCursorW, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PostQuitMessage, QS_ALLINPUT, RegisterClassExW, SW_HIDE,
    SW_SHOWNA, SetWindowLongPtrW, ShowWindow, WM_APP, WM_DESTROY, WM_ERASEBKGND, WNDCLASSEXW,
    WS_CHILD, WS_EX_NOACTIVATE,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Posted to the main thread when theme files change.
pub const WM_RELOAD: u32 = WM_APP + 1;
/// Posted to the main thread by the [`glaze_desktop::ControlWindow`] when a
/// system condition changes.
pub use glaze_desktop::WM_FACT;
/// Posted to the main thread when `config.json` changes.
pub const WM_CONFIG: u32 = WM_APP + 3;

const CLASS: PCWSTR = w!("glaze.wallpaper");
/// The window class of the wallpaper's [`glaze_desktop::ControlWindow`].
pub const CONTROL_CLASS: PCWSTR = w!("glaze.wallpaper.control");
const DEV_WINDOW: isize = 1;

/// Makes the calling thread a COM single-threaded apartment for the rest of
/// the process. WebView2 requires one on the thread that pumps messages (its
/// completion handlers are delivered through that pump). Everything else that
/// uses COM here (WIC) tolerates an apartment chosen earlier.
pub fn init_com_apartment() -> Result<()> {
    // SAFETY: plain per-thread COM init, intentionally never balanced: the
    // apartment lives until the process exits.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .context("CoInitializeEx")
}

pub fn register_class() -> Result<()> {
    register(CLASS, wndproc)
}

fn register(
    name: PCWSTR,
    proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static wndproc and class name.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: name,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

/// An owned window, destroyed on drop.
pub struct Window(pub HWND);

impl Window {
    /// A borderless child of the desktop, covering `monitor`, behind the icons.
    pub fn desktop(host: &DesktopHost, monitor: Rect) -> Result<Self> {
        // SAFETY: class registered by `register_class`; parent is a live shell window.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                CLASS,
                w!("Glaze"),
                WS_CHILD,
                0,
                0,
                monitor.width,
                monitor.height,
                Some(host.parent()),
                None,
                None,
                None,
            )?
        };
        let window = Self(hwnd);
        host.place(hwnd, monitor)?;
        Ok(window)
    }

    /// A normal resizable window, for developing without touching the desktop.
    pub fn dev(title: &str) -> Result<Self> {
        // SAFETY: class registered by `register_class`.
        let hwnd = unsafe {
            let hwnd = CreateWindowExW(
                Default::default(),
                CLASS,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                720,
                None,
                None,
                None,
                None,
            )?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, DEV_WINDOW);
            hwnd
        };
        Ok(Self(hwnd))
    }

    pub fn client_size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        // SAFETY: valid window; `rect` outlives the call.
        let _ = unsafe { GetClientRect(self.0, &mut rect) };
        (
            (rect.right - rect.left).max(0) as u32,
            (rect.bottom - rect.top).max(0) as u32,
        )
    }

    pub fn minimized(&self) -> bool {
        // SAFETY: read-only query.
        unsafe { IsIconic(self.0) }.as_bool()
    }

    /// Shows or hides the window without activating it. Hidden, the desktop shows what is under
    /// it: Windows' own wallpaper (the theme's still, see `snapshot`).
    pub fn show(&self, visible: bool) {
        let how = if visible { SW_SHOWNA } else { SW_HIDE };
        // SAFETY: our own window, on its thread.
        let _ = unsafe { ShowWindow(self.0, how) };
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: we own this window and drop it on its creating thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: standard window procedure, called by the system on our thread.
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1), // the GPU paints everything; avoid flicker
            WM_DESTROY => {
                // Closing the dev window quits. A desktop window dying means
                // Explorer took its parent down: ask to be restarted.
                let dev = GetWindowLongPtrW(hwnd, GWLP_USERDATA) == DEV_WINDOW;
                PostQuitMessage(if dev { 0 } else { EXIT_RESTART.into() });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Sleeps the main thread until a deadline or a window message, whichever
/// comes first. Uses a high-resolution waitable timer, so frame pacing is
/// precise without spinning (plain `Sleep` rounds to ~15.6ms).
pub struct Waiter {
    timer: HANDLE,
}

impl Waiter {
    pub fn new() -> Result<Self> {
        // SAFETY: creates an unnamed timer we own.
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                PCWSTR::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )?
        };
        Ok(Self { timer })
    }

    /// `None` sleeps until the next message: zero CPU while fully paused.
    pub fn wait(&self, deadline: Option<Instant>, extra: &[HANDLE]) {
        // SAFETY: the timer handle is valid for our lifetime.
        unsafe {
            let mut handles = extra.to_vec();
            if let Some(deadline) = deadline {
                let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                    return;
                };
                let due = -((left.as_nanos() / 100) as i64).max(1); // relative, 100ns units
                let _ = SetWaitableTimer(self.timer, &due, 0, None, None, false);
                handles.push(self.timer);
            }
            MsgWaitForMultipleObjectsEx(Some(&handles), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        let _ = unsafe { CloseHandle(self.timer) };
    }
}
