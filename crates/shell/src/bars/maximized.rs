//! Event-driven per-monitor z-order maximization and a trailing visibility debounce.
use mypaper_desktop::Rect;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    },
    UI::WindowsAndMessaging::*,
};

const DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Default)]
struct Facts {
    maximized: bool,
    minimized: bool,
    visible: bool,
    tool: bool,
    owned: bool,
    cloaked: bool,
    ours: bool,
    system_ui: bool,
    overlay: bool,
}

fn normal_app(f: Facts) -> bool {
    f.visible
        && !f.minimized
        && !f.tool
        && !f.owned
        && !f.cloaked
        && !f.ours
        && !f.system_ui
        && !f.overlay
}

fn system_ui(class: &str) -> bool {
    matches!(
        class,
        "XamlExplorerHostIslandWindow"
            | "MultitaskingViewFrame"
            | "ForegroundStaging"
            | "Windows.UI.Core.CoreWindow"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Progman"
            | "WorkerW"
            | "IME"
            | "Default IME"
            | "MSCTFIME UI"
            | "CiceroUIWndFrame"
            | "CEF-OSC-WIDGET"
    )
}

fn topmost_overlay(ex: u32, style: u32) -> bool {
    // A regular always-on-top app still participates; captionless utility overlays do not.
    ex & WS_EX_TOPMOST.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 && style & WS_CAPTION.0 != WS_CAPTION.0
}

/// Snapshot only after a shared WinEvent or config/display change, never on a periodic timer.
pub struct ZOrder(Vec<AppWindow>);

impl ZOrder {
    pub fn read() -> Self {
        let mut windows = Vec::new();
        let mut seen = HashSet::new();
        // SAFETY: read-only queries. Track visited handles because windows can be reordered or
        // destroyed during traversal; never loop indefinitely if the desktop changes under us.
        unsafe {
            let mut next = GetTopWindow(None).ok();
            while let Some(hwnd) = next.filter(|hwnd| !hwnd.is_invalid()) {
                if !seen.insert(hwnd.0 as usize) {
                    break;
                }
                next = GetWindow(hwnd, GW_HWNDNEXT).ok();
                if let Some(window) = AppWindow::read(hwnd) {
                    windows.push(window);
                }
            }
        }
        Self(windows)
    }

    pub fn hides(&self, monitor: Rect) -> bool {
        // Stop at the first normal app on this monitor, even if it is restored. Neither shell
        // UI above it nor a foreground app on another monitor changes this monitor's decision.
        self.0
            .iter()
            .find(|window| window.monitor == monitor && normal_app(window.facts))
            .is_some_and(AppWindow::hides)
    }
}

struct AppWindow {
    facts: Facts,
    monitor: Rect,
    frame: Rect,
    work: Rect,
}

impl AppWindow {
    fn read(hwnd: HWND) -> Option<Self> {
        // SAFETY: read-only queries; a destroyed HWND simply fails the snapshot.
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let class = mypaper_bars::apps::class_name(hwnd);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                size_of_val(&cloaked) as u32,
            );
            let facts = Facts {
                maximized: IsZoomed(hwnd).as_bool(),
                minimized: IsIconic(hwnd).as_bool(),
                visible: IsWindowVisible(hwnd).as_bool(),
                tool: ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0,
                owned: GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()),
                cloaked: cloaked != 0,
                ours: pid == std::process::id() || class.starts_with("mypaper."),
                system_ui: system_ui(&class),
                overlay: topmost_overlay(ex, GetWindowLongPtrW(hwnd, GWL_STYLE) as u32),
            };
            if !normal_app(facts) {
                return None;
            }
            let mut frame = RECT::default();
            // DWM's painted frame can still be animating when the foreground/maximize
            // event arrives. The native rect already has the maximized target bounds.
            if GetWindowRect(hwnd, &mut frame).is_err() {
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    (&raw mut frame).cast(),
                    size_of_val(&frame) as u32,
                )
                .ok()?;
            }
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info)
                .as_bool()
            {
                return None;
            }
            Some(Self {
                facts,
                monitor: info.rcMonitor.into(),
                work: info.rcWork.into(),
                frame: frame.into(),
            })
        }
    }

    fn hides(&self) -> bool {
        let f = self.frame;
        let w = self.work;
        // DWM can round an extended frame by a physical pixel at fractional DPI.
        let fills = f.x <= w.x + 2
            && f.y <= w.y + 2
            && f.x + f.width >= w.x + w.width - 2
            && f.y + f.height >= w.y + w.height - 2;
        self.facts.maximized && fills
    }
}

#[derive(Default)]
pub struct Debounce {
    pub active: bool,
    pending: Option<(bool, Instant)>,
}

impl Debounce {
    pub fn observe(&mut self, hidden: bool, now: Instant) {
        if hidden == self.active {
            self.pending = None;
        } else if self.pending.is_none_or(|(value, _)| value != hidden) {
            self.pending = Some((hidden, now + DELAY));
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.pending.map(|(_, at)| at)
    }

    pub fn take_due(&mut self, now: Instant) -> Option<bool> {
        let (hidden, at) = self.pending?;
        if at > now {
            return None;
        }
        self.pending = None;
        self.active = hidden;
        Some(hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maximized() -> Facts {
        Facts {
            maximized: true,
            visible: true,
            ..Default::default()
        }
    }

    const MONITOR: Rect = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    const OTHER: Rect = Rect {
        x: -2560,
        ..MONITOR
    };

    fn window(facts: Facts, monitor: Rect) -> AppWindow {
        AppWindow {
            facts,
            monitor,
            frame: monitor,
            work: monitor,
        }
    }

    #[test]
    fn switcher_above_maximized_app_keeps_dock_hidden() {
        for class in [
            "XamlExplorerHostIslandWindow",
            "MultitaskingViewFrame",
            "ForegroundStaging",
            "Windows.UI.Core.CoreWindow",
        ] {
            let windows = ZOrder(vec![
                window(
                    Facts {
                        maximized: false,
                        system_ui: system_ui(class),
                        ..maximized()
                    },
                    MONITOR,
                ),
                window(maximized(), MONITOR),
            ]);
            assert!(windows.hides(MONITOR), "{class}");
        }
    }

    #[test]
    fn restored_app_above_maximized_app_shows_dock() {
        let windows = ZOrder(vec![
            window(
                Facts {
                    maximized: false,
                    ..maximized()
                },
                MONITOR,
            ),
            window(maximized(), MONITOR),
        ]);
        assert!(!windows.hides(MONITOR));
    }

    #[test]
    fn maximized_app_on_another_monitor_has_no_effect() {
        let windows = ZOrder(vec![window(maximized(), OTHER)]);
        assert!(!windows.hides(MONITOR));
        assert!(windows.hides(OTHER));
    }

    #[test]
    fn focusing_another_monitor_keeps_this_monitors_dock_hidden() {
        let mut windows = ZOrder(vec![
            window(maximized(), MONITOR),
            window(
                Facts {
                    maximized: false,
                    ..maximized()
                },
                OTHER,
            ),
        ]);
        assert!(windows.hides(MONITOR));
        assert!(!windows.hides(OTHER));
        // Focusing the restored app on the other monitor moves it to the front of z-order.
        windows.0.swap(0, 1);
        assert!(windows.hides(MONITOR));
        assert!(!windows.hides(OTHER));
    }

    #[test]
    fn maximized_frame_overshoot_and_negative_monitor_coordinates() {
        let monitor = Rect {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let mut app = AppWindow {
            facts: maximized(),
            monitor,
            work: Rect {
                x: -2560,
                y: 24,
                width: 2560,
                height: 1342,
            },
            frame: Rect {
                x: -2568,
                y: 16,
                width: 2576,
                height: 1358,
            },
        };
        assert!(app.hides());
        app.frame.width = 900;
        assert!(!app.hides());
    }

    #[test]
    fn non_app_windows_are_skipped_above_a_maximized_app() {
        for facts in [
            Facts {
                minimized: true,
                ..maximized()
            },
            Facts {
                tool: true,
                ..maximized()
            },
            Facts {
                cloaked: true,
                ..maximized()
            },
            Facts {
                ours: true,
                ..maximized()
            },
            Facts {
                system_ui: true,
                ..maximized()
            },
            Facts {
                owned: true,
                ..maximized()
            },
            Facts {
                overlay: true,
                ..maximized()
            },
            Facts {
                visible: false,
                ..maximized()
            },
        ] {
            assert!(!normal_app(facts));
            let windows = ZOrder(vec![window(facts, MONITOR), window(maximized(), MONITOR)]);
            assert!(windows.hides(MONITOR));
        }
    }

    #[test]
    fn topmost_overlays_are_skipped_but_normal_topmost_apps_participate() {
        assert!(topmost_overlay(WS_EX_TOPMOST.0, WS_POPUP.0));
        assert!(!topmost_overlay(WS_EX_TOPMOST.0, WS_CAPTION.0));
        assert!(!topmost_overlay(
            WS_EX_TOPMOST.0 | WS_EX_APPWINDOW.0,
            WS_POPUP.0
        ));
    }

    #[test]
    fn rapid_focus_changes_cancel_hide_and_show_and_repeated_events_do_not_starve() {
        let now = Instant::now();
        let mut d = Debounce::default();
        d.observe(true, now);
        assert_eq!(d.take_due(now + Duration::from_millis(149)), None);
        d.observe(false, now + Duration::from_millis(100));
        assert_eq!(d.deadline(), None);
        assert!(!d.active);
        d.observe(true, now + Duration::from_millis(200));
        d.observe(true, now + Duration::from_millis(300));
        assert_eq!(d.take_due(now + Duration::from_millis(350)), Some(true));
        d.observe(false, now + Duration::from_millis(400));
        d.observe(true, now + Duration::from_millis(450));
        assert_eq!(d.deadline(), None);
        assert!(d.active);
        d.observe(false, now + Duration::from_millis(500));
        assert_eq!(d.take_due(now + Duration::from_millis(650)), Some(false));
    }
}
