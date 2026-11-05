//! Broadcasts and app-bar reservations. The visible windows belong to `mypaper-ui`;
//! these hidden windows only receive Windows' notifications and reserve work area.

use anyhow::Result;
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{ABM_WINDOWPOSCHANGED, APPBARDATA, SHAppBarMessage};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::appbar::{AppBar, Edge};

pub const APPBAR: u32 = WM_APP + 421;
pub const DISPLAY: u32 = WM_APP + 422;
pub const EXPLORER: u32 = WM_APP + 423;
pub const POWER: u32 = WM_APP + 424;
pub const TIME: u32 = WM_APP + 425;
pub const SHELL: u32 = WM_APP + 426;
pub const SETTINGS: u32 = WM_APP + 427;
pub const TRASH: u32 = WM_APP + 428;
const CLASS: PCWSTR = w!("mypaper.bars.services");

/// A hidden top-level window (message-only windows miss system broadcasts).
pub struct Broadcast(HWND);

impl Broadcast {
    pub fn new() -> Result<Self> {
        let hwnd = window()?;
        // SAFETY: our own top-level window; callbacks only post to its thread.
        unsafe {
            let _ = RegisterShellHookWindow(hwnd);
        }
        Ok(Self(hwnd))
    }

    pub fn hwnd(&self) -> HWND {
        self.0
    }
}

impl Drop for Broadcast {
    fn drop(&mut self) {
        // SAFETY: our own window on its creating thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

/// The reservation remains full width even when the visible bar floats inside it.
pub struct Reservation {
    appbar: Option<AppBar>,
    hwnd: HWND,
    edge: Edge,
}

impl Reservation {
    pub fn new(edge: Edge) -> Result<Self> {
        let hwnd = window()?;
        Ok(Self {
            appbar: AppBar::register(hwnd, edge, APPBAR),
            hwnd,
            edge,
        })
    }

    pub fn reserve(&mut self, monitor: Rect, thickness: i32) -> Rect {
        self.try_reserve(monitor, thickness)
            .unwrap_or_else(|| crate::appbar::settle(monitor, self.edge, thickness))
    }

    /// Registration can fail while Explorer is restarting or servicing another callback.
    /// Keep the owned HWND and retry; geometry alone does not reserve Windows' work area.
    pub fn try_reserve(&mut self, monitor: Rect, thickness: i32) -> Option<Rect> {
        if self.appbar.is_none() {
            self.appbar = AppBar::register(self.hwnd, self.edge, APPBAR);
        }
        self.appbar.as_mut()?.try_reserve(monitor, thickness)
    }

    pub fn reregister(&mut self) {
        self.appbar = None;
        self.appbar = AppBar::register(self.hwnd, self.edge, APPBAR);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.appbar = None;
        // SAFETY: unregistered before its owned window is destroyed.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn window() -> Result<HWND> {
    // SAFETY: static procedure/class; all windows are created/destroyed on the UI thread.
    unsafe {
        let info = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&info);
        Ok(CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("mypaper bar services"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )?)
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: registered message names are static; thread posts never run app code.
    unsafe {
        let notification = match msg {
            APPBAR => APPBAR,
            WM_DISPLAYCHANGE | WM_DPICHANGED => DISPLAY,
            WM_POWERBROADCAST => POWER,
            WM_TIMECHANGE => TIME,
            WM_SETTINGCHANGE if wp.0 as u32 == SPI_SETWORKAREA.0 => DISPLAY,
            WM_SETTINGCHANGE => SETTINGS,
            crate::trash::WM_TRASH_NOTIFY => TRASH,
            _ if msg == RegisterWindowMessageW(w!("TaskbarCreated")) => EXPLORER,
            // Only what the bars act on: a window flashing (HSHELL_FLASH) and the input language
            // (HSHELL_LANGUAGE). The rest arrives for every window any app creates, moves or
            // retitles; re-posting it would wake the loop a second time for nothing.
            _ if msg == RegisterWindowMessageW(w!("SHELLHOOK")) => match wp.0 {
                0x8006 | 8 => SHELL,
                _ => return LRESULT(0),
            },
            _ => 0,
        };
        if notification != 0 {
            let _ = PostThreadMessageW(GetCurrentThreadId(), notification, wp, lp);
            return LRESULT(0);
        }
        if msg == WM_WINDOWPOSCHANGED {
            let mut data = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                hWnd: hwnd,
                ..Default::default()
            };
            SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut data);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
