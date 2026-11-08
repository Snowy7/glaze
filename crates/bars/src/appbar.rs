//! App-bar registration and work-area reservations, independent of visible surface geometry.
//! A floating bar reserves its complete band; auto-hide reserves nothing.

use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    APPBARDATA, SHAppBarMessage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// The `ABE_*` value.
    pub fn abe(self) -> u32 {
        match self {
            Self::Top => ABE_TOP,
            Self::Bottom => ABE_BOTTOM,
            Self::Left => ABE_LEFT,
            Self::Right => ABE_RIGHT,
        }
    }
}

/// Keeps `thickness` px of `rect` on its `edge` side. Proposes a band from a monitor's rect, and
/// settles what `ABM_QUERYPOS` returns: the system moves the outer side past other app bars (the
/// taskbar, our other bars), and we keep our thickness from there.
pub fn settle(rect: Rect, edge: Edge, thickness: i32) -> Rect {
    let thickness = thickness.max(0);
    match edge {
        Edge::Top => Rect {
            height: thickness,
            ..rect
        },
        Edge::Bottom => Rect {
            y: rect.y + rect.height - thickness,
            height: thickness,
            ..rect
        },
        Edge::Left => Rect {
            width: thickness,
            ..rect
        },
        Edge::Right => Rect {
            x: rect.x + rect.width - thickness,
            width: thickness,
            ..rect
        },
    }
}

/// The window around `band`: `room` px more toward the middle of `monitor`, clipped to it.
pub fn window_rect(band: Rect, edge: Edge, room: i32, monitor: Rect) -> Rect {
    let bottom = monitor.y + monitor.height;
    let right = monitor.x + monitor.width;
    match edge {
        Edge::Top => Rect {
            height: (band.height + room).min(bottom - band.y),
            ..band
        },
        Edge::Bottom => {
            let top = (band.y - room).max(monitor.y);
            Rect {
                y: top,
                height: band.y + band.height - top,
                ..band
            }
        }
        Edge::Left => Rect {
            width: (band.width + room).min(right - band.x),
            ..band
        },
        Edge::Right => {
            let left = (band.x - room).max(monitor.x);
            Rect {
                x: left,
                width: band.x + band.width - left,
                ..band
            }
        }
    }
}

fn to_rect(r: Rect) -> RECT {
    RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.width,
        bottom: r.y + r.height,
    }
}

/// A registered app bar; unregistered on drop.
pub struct AppBar {
    hwnd: HWND,
    edge: Edge,
    callback: u32,
    registered: bool,
    /// The band last granted, so an unchanged band isn't set again (each set notifies every app
    /// bar, ours included).
    band: Option<Rect>,
}

impl AppBar {
    /// Registers `hwnd`; Explorer sends it `callback` with an `ABN_*` code in `wParam`.
    pub fn register(hwnd: HWND, edge: Edge, callback: u32) -> Option<Self> {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: callback,
            ..Default::default()
        };
        // SAFETY: `data` is a valid APPBARDATA for our own window.
        let ok = unsafe { SHAppBarMessage(ABM_NEW, &mut data) } != 0;
        if !ok {
            log::warn!("couldn't register an app bar (is Explorer running?)");
        }
        ok.then_some(Self {
            hwnd,
            edge,
            callback,
            registered: true,
            band: None,
        })
    }

    /// Reserves `thickness` px along the edge of `monitor`, past any other app bars there, and
    /// returns the band the system granted.
    pub fn reserve(&mut self, monitor: Rect, thickness: i32) -> Rect {
        self.try_reserve(monitor, thickness)
            .unwrap_or_else(|| settle(monitor, self.edge, thickness))
    }

    /// As [`reserve`](Self::reserve), but exposes an Explorer refusal so the shell can retry.
    pub fn try_reserve(&mut self, monitor: Rect, thickness: i32) -> Option<Rect> {
        // Explorer can forget a registration without changing the queried band. A cached
        // rectangle alone is not evidence that Windows still excludes it from the work area.
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: read-only monitor queries with initialized storage.
        let lost = unsafe {
            GetMonitorInfoW(
                MonitorFromRect(&to_rect(monitor), MONITOR_DEFAULTTONEAREST),
                &mut info,
            )
        }
        .as_bool()
            && self.needs_repair(info.rcWork.into());
        if lost {
            log::warn!(
                "app bar {:?}: work area overlaps reserved band; registering again",
                self.edge
            );
            let mut data = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                ..Default::default()
            };
            // SAFETY: removes only our own reservation; the HWND remains alive.
            unsafe { SHAppBarMessage(ABM_REMOVE, &mut data) };
            self.registered = false;
            self.band = None;
        }
        if !self.registered {
            let mut data = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                uCallbackMessage: self.callback,
                ..Default::default()
            };
            // SAFETY: re-registers our live window, including retries after Explorer refused it.
            self.registered = unsafe { SHAppBarMessage(ABM_NEW, &mut data) } != 0;
            if !self.registered {
                return None;
            }
        }
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uEdge: self.edge.abe(),
            rc: to_rect(settle(monitor, self.edge, thickness)),
            ..Default::default()
        };
        // SAFETY: `data` is a valid APPBARDATA for our registered window.
        if unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut data) } == 0 {
            self.band = None;
            return None;
        }
        let band = settle(data.rc.into(), self.edge, thickness);
        if self.band == Some(band) {
            return Some(band);
        }
        data.rc = to_rect(band);
        // SAFETY: as above.
        if unsafe { SHAppBarMessage(ABM_SETPOS, &mut data) } == 0 {
            self.band = None;
            return None;
        }
        let granted: Rect = data.rc.into();
        self.band = Some(granted);
        Some(granted)
    }

    /// Forgets the last band (Explorer restarted): the next [`reserve`](Self::reserve) sets it.
    pub fn invalidate(&mut self) {
        self.band = None;
    }

    pub fn needs_repair(&self, work: Rect) -> bool {
        self.band
            .is_some_and(|band| !work_excludes_band(work, band, self.edge))
    }
}

/// Other bars may shrink the work area further. Only intrusion into our band is a loss.
fn work_excludes_band(work: Rect, band: Rect, edge: Edge) -> bool {
    match edge {
        Edge::Top => work.y >= band.y + band.height,
        Edge::Bottom => work.y + work.height <= band.y,
        Edge::Left => work.x >= band.x + band.width,
        Edge::Right => work.x + work.width <= band.x,
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            ..Default::default()
        };
        // SAFETY: unregisters our own window.
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut data) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const SCREEN: Rect = rect(0, 0, 2560, 1440);
    const LEFT_SCREEN: Rect = rect(-2560, 9, 2560, 1440);

    #[test]
    fn work_area_reset_invalidates_an_unchanged_band_on_every_edge() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let band = settle(LEFT_SCREEN, edge, 32);
            assert!(!work_excludes_band(LEFT_SCREEN, band, edge), "{edge:?}");
            let inset = rect(-2520, 49, 2480, 1360);
            assert!(work_excludes_band(inset, band, edge), "{edge:?}");
        }
    }

    #[test]
    #[ignore = "temporarily reserves 3 px of the live desktop; run under the desktop lock"]
    fn repairs_a_registration_removed_without_taskbar_created() {
        let window = crate::events::Broadcast::new().unwrap();
        let monitor = mypaper_desktop::monitor_infos()
            .into_iter()
            .find(|m| m.primary)
            .unwrap();
        let mut bar = AppBar::register(window.hwnd(), Edge::Right, crate::events::APPBAR).unwrap();
        let band = bar.try_reserve(monitor.rect, 3).unwrap();
        let work = || {
            mypaper_desktop::monitor_infos()
                .into_iter()
                .find(|m| m.primary)
                .unwrap()
                .work
        };
        assert!(!bar.needs_repair(work()));
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: window.hwnd(),
            ..Default::default()
        };
        // SAFETY: fault injection removes only the test's own appbar, never Explorer's.
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut data) };
        assert!(
            bar.needs_repair(work()),
            "removal must expose the stale cached band"
        );
        assert_eq!(bar.try_reserve(monitor.rect, 3), Some(band));
        assert!(
            !bar.needs_repair(work()),
            "the unchanged rectangle must be reserved again"
        );
    }

    #[test]
    fn proposes_a_band_on_each_edge() {
        assert_eq!(settle(SCREEN, Edge::Top, 32), rect(0, 0, 2560, 32));
        assert_eq!(settle(SCREEN, Edge::Bottom, 64), rect(0, 1376, 2560, 64));
        assert_eq!(
            settle(LEFT_SCREEN, Edge::Left, 64),
            rect(-2560, 9, 64, 1440)
        );
        assert_eq!(settle(LEFT_SCREEN, Edge::Right, 64), rect(-64, 9, 64, 1440));
    }

    #[test]
    fn settles_past_other_app_bars() {
        // The taskbar (48 px) is at the bottom: QUERYPOS moves our band's bottom above it.
        let queried = rect(0, 1328, 2560, 64);
        assert_eq!(settle(queried, Edge::Bottom, 64), rect(0, 1328, 2560, 64));
        let queried = rect(0, 0, 2560, 1392);
        assert_eq!(settle(queried, Edge::Bottom, 64), rect(0, 1328, 2560, 64));
        // A top bar already took 32 px: a left dock starts below it.
        let queried = rect(0, 32, 2560, 1408);
        assert_eq!(settle(queried, Edge::Left, 64), rect(0, 32, 64, 1408));
        assert_eq!(settle(queried, Edge::Top, -5), rect(0, 32, 2560, 0));
    }

    #[test]
    fn windows_extend_toward_the_middle_and_stay_on_their_monitor() {
        let top = rect(0, 0, 2560, 32);
        assert_eq!(
            window_rect(top, Edge::Top, 360, SCREEN),
            rect(0, 0, 2560, 392)
        );
        let bottom = rect(0, 1376, 2560, 64);
        assert_eq!(
            window_rect(bottom, Edge::Bottom, 360, SCREEN),
            rect(0, 1016, 2560, 424)
        );
        let right = rect(-64, 9, 64, 1440);
        assert_eq!(
            window_rect(right, Edge::Right, 360, LEFT_SCREEN),
            rect(-424, 9, 424, 1440)
        );
        // A small screen clips the room.
        let tiny = rect(0, 0, 800, 300);
        assert_eq!(
            window_rect(rect(0, 0, 800, 32), Edge::Top, 360, tiny),
            rect(0, 0, 800, 300)
        );
        assert_eq!(
            window_rect(rect(0, 0, 64, 300), Edge::Left, 1000, tiny),
            rect(0, 0, 800, 300)
        );
    }
}
