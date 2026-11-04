use std::time::Duration;

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{HWND, LPARAM, MAX_PATH, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GW_CHILD, GW_HWNDNEXT, GW_HWNDPREV, GWL_EXSTYLE,
    GetClassNameW, GetShellWindow, GetWindow, GetWindowLongPtrW, HWND_TOP, SMTO_NORMAL,
    SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageTimeoutW, SetWindowPos,
    SystemParametersInfoW, WS_EX_NOREDIRECTIONBITMAP,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::Rect;

/// Undocumented Progman message that makes Explorer create the WorkerW
/// window wallpapers live in. Every live-wallpaper app relies on it.
const SPAWN_WORKERW: u32 = 0x052C;

/// Explorer may still be building the desktop when we start (autostart at
/// logon, right after an Explorer restart): look again for this long before giving up.
const DESKTOP_WAIT: Duration = Duration::from_millis(1500);
const DESKTOP_POLL: Duration = Duration::from_millis(100);

/// Where wallpaper windows go so they render *behind the desktop icons*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopHost {
    /// Windows 11 24H2+: icons (`SHELLDLL_DefView`) and the static wallpaper
    /// (`WorkerW`) are both children of Progman. We slot in between them.
    Layered { progman: HWND, defview: HWND },
    /// Older builds: a dedicated top-level `WorkerW` sits behind the icons,
    /// which live in another top-level window (`icons`, DefView's parent).
    Legacy { workerw: HWND, icons: HWND },
}

/// What a window in Progman's z-order is, for [`order_is_valid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// The icon view (`SHELLDLL_DefView`).
    Icons,
    /// The static wallpaper (`WorkerW`).
    StaticWallpaper,
    /// The wallpaper window being checked.
    Ours,
    Other,
}

/// Is our window where a wallpaper must be in Progman's children (listed top
/// to bottom): below the icon view and above every static-wallpaper window?
/// A wallpaper above the icons would cover them; below the `WorkerW` it would
/// be hidden behind the static picture.
fn order_is_valid(top_to_bottom: &[Layer]) -> bool {
    let Some(ours) = top_to_bottom.iter().position(|l| *l == Layer::Ours) else {
        return false;
    };
    let below_icons = top_to_bottom
        .iter()
        .position(|l| *l == Layer::Icons)
        .is_none_or(|icons| icons < ours);
    let above_static = top_to_bottom
        .iter()
        .enumerate()
        .all(|(i, l)| *l != Layer::StaticWallpaper || i > ours);
    below_icons && above_static
}

impl DesktopHost {
    /// Finds the desktop. A running Explorer may be a moment late to have built
    /// one (logon, right after it restarted), so this looks again for a short
    /// while; with no shell at all it fails at once.
    pub fn find() -> Result<Self> {
        let deadline = std::time::Instant::now() + DESKTOP_WAIT;
        loop {
            match Self::find_once() {
                Ok(host) => return Ok(host),
                Err(e) if !shell_running() || std::time::Instant::now() >= deadline => {
                    return Err(e);
                }
                Err(_) => std::thread::sleep(DESKTOP_POLL),
            }
        }
    }

    /// Is this the 24H2+ "raised desktop"? Explorer then composes the desktop
    /// itself (Progman has `WS_EX_NOREDIRECTIONBITMAP`) and the icon view,
    /// `SHELLDLL_DefView`, is a layered child of Progman with the static
    /// wallpaper's `WorkerW` below it. Wallpaper windows go between the two,
    /// which is what [`Layered`](Self::Layered) placement does; this says
    /// which variant of it the machine runs (for logs and bug reports).
    pub fn raised(&self) -> bool {
        match *self {
            Self::Layered { progman, .. } => {
                // SAFETY: read-only query of a window handle.
                let ex = unsafe { GetWindowLongPtrW(progman, GWL_EXSTYLE) } as u32;
                ex & WS_EX_NOREDIRECTIONBITMAP.0 != 0
            }
            Self::Legacy { .. } => false,
        }
    }

    /// Is `child` still where a wallpaper belongs (see [`order_is_valid`])?
    /// Always true for the legacy layout, which has its own `WorkerW` to itself.
    pub fn attached_correctly(&self, child: HWND) -> bool {
        let Self::Layered { progman, .. } = *self else {
            return true;
        };
        let mut order = Vec::new();
        // SAFETY: read-only walk over the children of a window handle.
        unsafe {
            let mut sibling = GetWindow(progman, GW_CHILD);
            while let Ok(window) = sibling {
                let mut class = [0u16; 32];
                let len = GetClassNameW(window, &mut class).max(0) as usize;
                let class = String::from_utf16_lossy(&class[..len]);
                order.push(match class.as_str() {
                    _ if window == child => Layer::Ours,
                    "SHELLDLL_DefView" => Layer::Icons,
                    "WorkerW" => Layer::StaticWallpaper,
                    _ => Layer::Other,
                });
                sibling = GetWindow(window, GW_HWNDNEXT);
            }
        }
        order_is_valid(&order)
    }

    /// Re-inserts `child` just below the icon view if Explorer moved it out of
    /// place (raised layout only). Cheap; call it when window events fire.
    pub fn keep_behind_icons(&self, child: HWND) {
        let Self::Layered { defview, .. } = *self else {
            return;
        };
        if self.attached_correctly(child) {
            return;
        }
        log::warn!("the wallpaper window left its place behind the icons; putting it back");
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // SAFETY: a z-order-only move of our own window.
        let _ = unsafe { SetWindowPos(child, Some(defview), 0, 0, 0, 0, flags) };
    }

    /// Resolves the current desktop once. Recovery callers wait for shell events instead of polling.
    pub fn find_once() -> Result<Self> {
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

            let mut found = (HWND::default(), HWND::default());
            let _ = EnumWindows(Some(find_legacy_workerw), LPARAM(&raw mut found as isize));
            let (icons, workerw) = found;
            if workerw.is_invalid() {
                return Err(anyhow!("could not find the desktop WorkerW window"));
            }
            Ok(Self::Legacy { workerw, icons })
        }
    }

    /// The window that wallpaper windows must be created as children of.
    pub fn parent(&self) -> HWND {
        match *self {
            Self::Layered { progman, .. } => progman,
            Self::Legacy { workerw, .. } => workerw,
        }
    }

    /// Whether a shell window belongs to this desktop generation.
    pub(crate) fn contains_window(&self, window: HWND) -> bool {
        match *self {
            Self::Layered { progman, defview } => window == progman || window == defview,
            Self::Legacy { workerw, icons } => window == workerw || window == icons,
        }
    }

    /// The window that holds the icons (`SHELLDLL_DefView`): widget windows,
    /// which draw over the icons, are created as its children.
    pub fn icons_parent(&self) -> HWND {
        match *self {
            Self::Layered { progman, .. } => progman,
            Self::Legacy { icons, .. } => icons,
        }
    }

    /// Moves a child of [`icons_parent`](Self::icons_parent) over the screen
    /// rect `rect`, z-ordered above the icons.
    pub fn place_above_icons(&self, child: HWND, rect: Rect) -> Result<()> {
        let mut origin = POINT {
            x: rect.x,
            y: rect.y,
        };
        // SAFETY: valid windows; `origin` outlives the call.
        unsafe {
            let _ = ScreenToClient(self.icons_parent(), &mut origin);
            SetWindowPos(
                child,
                Some(HWND_TOP),
                origin.x,
                origin.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        Ok(())
    }

    /// Puts `child` (placed with [`place_above_icons`](Self::place_above_icons))
    /// back above the icons if Explorer raised them over it. Cheap; call it
    /// when window events fire.
    pub fn keep_above_icons(&self, child: HWND) {
        // SAFETY: read-only z-order walk over sibling windows, then a z-order-only move.
        unsafe {
            let Ok(defview) = FindWindowExW(
                Some(self.icons_parent()),
                None,
                w!("SHELLDLL_DefView"),
                PCWSTR::null(),
            ) else {
                return;
            };
            let mut above = GetWindow(child, GW_HWNDPREV);
            while let Ok(window) = above {
                if window == defview {
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                    let _ = SetWindowPos(child, Some(HWND_TOP), 0, 0, 0, 0, flags);
                    return;
                }
                above = GetWindow(window, GW_HWNDPREV);
            }
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
        // Placed; if the layout is not what we expect, fix it now and say so.
        self.keep_behind_icons(child);
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

/// Is there a shell window (Progman) at all?
fn shell_running() -> bool {
    // SAFETY: plain lookups.
    unsafe { !GetShellWindow().is_invalid() || FindWindowW(w!("Progman"), PCWSTR::null()).is_ok() }
}

/// Legacy layout: the WorkerW we want is the top-level sibling right after
/// the window that hosts `SHELLDLL_DefView`. Finds both, as `(icons, workerw)`.
unsafe extern "system" fn find_legacy_workerw(hwnd: HWND, out: LPARAM) -> BOOL {
    // SAFETY: `out` points at the (HWND, HWND) owned by `DesktopHost::find`.
    unsafe {
        if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok()
            && let Ok(workerw) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null())
        {
            *(out.0 as *mut (HWND, HWND)) = (hwnd, workerw);
            return false.into();
        }
    }
    true.into()
}

#[cfg(test)]
mod tests {
    use super::Layer::{Icons, Other, Ours, StaticWallpaper};
    use super::*;

    #[test]
    fn a_wallpaper_belongs_between_the_icons_and_the_static_wallpaper() {
        // The layout seen on a 24H2 raised desktop: widgets, icons, ours, static wallpaper.
        assert!(order_is_valid(&[Other, Icons, Ours, StaticWallpaper]));
        assert!(order_is_valid(&[
            Icons,
            Ours,
            Other,
            Other,
            StaticWallpaper
        ]));
        // One window per monitor: only the one being checked matters.
        assert!(order_is_valid(&[Icons, Other, Ours, StaticWallpaper]));
        // No static wallpaper, or no icon view (yet): nothing to be on the wrong side of.
        assert!(order_is_valid(&[Icons, Ours]));
        assert!(order_is_valid(&[Ours, StaticWallpaper]));
    }

    #[test]
    fn covering_the_icons_or_hiding_behind_the_static_wallpaper_is_caught() {
        assert!(
            !order_is_valid(&[Ours, Icons, StaticWallpaper]),
            "over the icons"
        );
        assert!(
            !order_is_valid(&[Icons, StaticWallpaper, Ours]),
            "behind the picture"
        );
        assert!(
            !order_is_valid(&[StaticWallpaper, Icons, Ours]),
            "behind everything"
        );
        assert!(
            !order_is_valid(&[Icons, StaticWallpaper]),
            "not a child at all"
        );
        assert!(!order_is_valid(&[]));
    }
}
