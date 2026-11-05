//! Event-driven foreground maximization and a trailing visibility debounce.
use mypaper_desktop::Rect;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::RECT,
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
    cloaked: bool,
    ours: bool,
    desktop: bool,
    fullscreen: bool,
}

fn eligible(f: Facts, same_monitor: bool, fills_work: bool) -> bool {
    f.maximized
        && f.visible
        && !f.minimized
        && !f.tool
        && !f.cloaked
        && !f.ours
        && !f.desktop
        && !f.fullscreen
        && same_monitor
        && fills_work
}

/// Snapshot only after a shared WinEvent or config/display change, never on a periodic timer.
pub struct Foreground {
    facts: Facts,
    monitor: Rect,
    frame: Rect,
    work: Rect,
}

impl Foreground {
    pub fn read(fullscreen: bool) -> Option<Self> {
        // SAFETY: read-only queries; a destroyed HWND simply fails the snapshot.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
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
                cloaked: cloaked != 0,
                ours: pid == std::process::id() || class.starts_with("mypaper."),
                desktop: matches!(
                    class.as_str(),
                    "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
                ),
                fullscreen,
            };
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

    pub fn hides(&self, monitor: Rect) -> bool {
        let f = self.frame;
        let w = self.work;
        // DWM can round an extended frame by a physical pixel at fractional DPI.
        let fills = f.x <= w.x + 2
            && f.y <= w.y + 2
            && f.x + f.width >= w.x + w.width - 2
            && f.y + f.height >= w.y + w.height - 2;
        eligible(self.facts, self.monitor == monitor, fills)
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

    #[test]
    fn maximized_hides_only_on_its_monitor_and_when_filling_work_area() {
        assert!(eligible(maximized(), true, true));
        assert!(!eligible(maximized(), false, true));
        assert!(!eligible(maximized(), true, false));
        assert!(!eligible(
            Facts {
                maximized: false,
                ..maximized()
            },
            true,
            true
        ));
    }

    #[test]
    fn maximized_frame_overshoot_and_negative_monitor_coordinates() {
        let monitor = Rect {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let mut foreground = Foreground {
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
        assert!(foreground.hides(monitor));
        assert!(!foreground.hides(Rect { x: 0, ..monitor }));
        foreground.frame.width = 900;
        assert!(!foreground.hides(monitor));
    }

    #[test]
    fn minimized_tools_cloaked_own_desktop_and_fullscreen_do_not_hide() {
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
                desktop: true,
                ..maximized()
            },
            Facts {
                fullscreen: true,
                ..maximized()
            },
            Facts {
                visible: false,
                ..maximized()
            },
        ] {
            assert!(!eligible(facts, true, true));
        }
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
