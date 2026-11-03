use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{HWND, LPARAM, MAX_PATH, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetShellWindow, SMTO_NORMAL, SPI_GETDESKWALLPAPER,
    SPI_SETDESKWALLPAPER, SWP_NOACTIVATE, SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SendMessageTimeoutW, SetWindowPos, SystemParametersInfoW,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::Rect;

/// Undocumented Progman message that makes Explorer create the WorkerW
/// window wallpapers live in. Every live-wallpaper app relies on it.
const SPAWN_WORKERW: u32 = 0x052C;

/// Where wallpaper windows go so they render *behind the desktop icons*.
#[derive(Debug, Clone, Copy)]
pub enum DesktopHost {
    /// Windows 11 24H2+: icons (`SHELLDLL_DefView`) and the static wallpaper
    /// (`WorkerW`) are both children of Progman. We slot in between them.
    Layered { progman: HWND, defview: HWND },
    /// Older builds: a dedicated top-level `WorkerW` sits behind the icons.
    Legacy { workerw: HWND },
}

impl DesktopHost {
    pub fn find() -> Result<Self> {
        // SAFETY: plain Win32 calls with valid arguments.
        unsafe {
            let mut progman = GetShellWindow();
            if progman.is_invalid() {
                progman = FindWindowW(w!("Progman"), PCWSTR::null())?;
            }
            SendMessageTimeoutW(
                progman,
                SPAWN_WORKERW,
                WPARAM(0xD),
                LPARAM(1),
                SMTO_NORMAL,
                1000,
                None,
            );

            if let Ok(defview) =
                FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
            {
                return Ok(Self::Layered { progman, defview });
            }

            let mut workerw = HWND::default();
            let _ = EnumWindows(Some(find_legacy_workerw), LPARAM(&raw mut workerw as isize));
            if workerw.is_invalid() {
                return Err(anyhow!("could not find the desktop WorkerW window"));
            }
            Ok(Self::Legacy { workerw })
        }
    }

    /// The window that wallpaper windows must be created as children of.
    pub fn parent(&self) -> HWND {
        match *self {
            Self::Layered { progman, .. } => progman,
            Self::Legacy { workerw } => workerw,
        }
    }

    /// Moves a child of [`parent`](Self::parent) over the screen rect `rect`,
    /// z-ordered just below the icons.
    pub fn place(&self, child: HWND, rect: Rect) -> Result<()> {
        let mut origin = POINT {
            x: rect.x,
            y: rect.y,
        };
        // SAFETY: valid windows; `origin` outlives the call.
        unsafe {
            let _ = ScreenToClient(self.parent(), &mut origin);
            let below = match *self {
                Self::Layered { defview, .. } => Some(defview),
                Self::Legacy { .. } => None,
            };
            SetWindowPos(
                child,
                below,
                origin.x,
                origin.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        Ok(())
    }

    /// Repaints the static wallpaper after our windows are gone. Only the
    /// legacy layout needs it: its WorkerW keeps our last frame otherwise.
    pub fn restore(&self) {
        if matches!(self, Self::Layered { .. }) {
            return;
        }
        let mut path = [0u16; MAX_PATH as usize];
        let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0);
        // SAFETY: `path` is MAX_PATH wide chars, as SPI_GETDESKWALLPAPER requires.
        unsafe {
            let buf = Some(path.as_mut_ptr().cast());
            if SystemParametersInfoW(SPI_GETDESKWALLPAPER, path.len() as u32, buf, flags).is_ok() {
                let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, buf, flags);
            }
        }
    }
}

/// Legacy layout: the WorkerW we want is the top-level sibling right after
/// the window that hosts `SHELLDLL_DefView`.
unsafe extern "system" fn find_legacy_workerw(hwnd: HWND, out: LPARAM) -> BOOL {
    // SAFETY: `out` points at the HWND owned by `DesktopHost::find`.
    unsafe {
        if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok()
            && let Ok(workerw) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null())
        {
            *(out.0 as *mut HWND) = workerw;
            return false.into();
        }
    }
    true.into()
}
