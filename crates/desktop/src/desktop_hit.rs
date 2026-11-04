//! "Is this point on the bare desktop?", for the desktop right-click menu and
//! for wallpaper input forwarding.
//!
//! Two layers:
//! - [`under`]: which window is at the point, by class names only (a few
//!   microseconds, allocation-free), so it can run inside a low-level mouse
//!   hook. It cannot tell an icon from the empty space around it.
//! - [`IconMap`]: the icons' rectangles, read ahead of time from the desktop's
//!   folder view (see [`crate::desktop_view`]), which the fast path consults.
//!   A stale map only costs a replayed click, because [`Swallower`]'s callers
//!   confirm the click authoritatively before showing a menu.
//!
//! [`Swallower`] is the pure decision logic of the right-click hook.

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, WindowFromPoint,
};

use crate::Rect;

/// Classes that make up the desktop layer: Explorer's icon view and its
/// container, the shell window itself, and our own wallpaper windows (which
/// show through where the icon view is transparent).
const DESKTOP_WINDOWS: &[&str] = &[
    "SHELLDLL_DefView",
    "SysListView32",
    "Progman",
    "WorkerW",
    "mypaper.wallpaper",
];
/// A desktop-layer window must live under one of these. Folder windows also
/// contain `SHELLDLL_DefView` and `SysListView32`, but their root is `CabinetWClass`.
const SHELL_ROOTS: &[&str] = &["Progman", "WorkerW"];

/// What is at a screen point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Under {
    /// The desktop layer: icons or empty space.
    Desktop,
    /// Anything else: an app window, the taskbar, a widget.
    Other,
}

/// ASCII class names against UTF-16 buffers, without allocating.
fn is_one_of(units: &[u16], names: &[&str]) -> bool {
    names.iter().any(|n| {
        n.len() == units.len() && units.iter().zip(n.bytes()).all(|(u, b)| *u == u16::from(b))
    })
}

/// The class decision of [`under`], on already fetched names.
fn classify(window_class: &[u16], root_class: &[u16]) -> Under {
    if is_one_of(root_class, SHELL_ROOTS) && is_one_of(window_class, DESKTOP_WINDOWS) {
        Under::Desktop
    } else {
        Under::Other
    }
}

/// Which window is at `point`: the desktop layer, or something else. Windows
/// that let clicks through (our widget surface between widgets) are skipped
/// by `WindowFromPoint`, so they count as what is beneath them.
pub fn under(point: POINT) -> Under {
    // SAFETY: read-only queries; the class buffers are valid for their lengths.
    unsafe {
        let window = WindowFromPoint(point);
        if window.is_invalid() {
            return Under::Other;
        }
        let root = GetAncestor(window, GA_ROOT);
        let mut window_class = [0u16; 32];
        let mut root_class = [0u16; 32];
        let window_len = GetClassNameW(window, &mut window_class).max(0) as usize;
        let root_len = GetClassNameW(root, &mut root_class).max(0) as usize;
        classify(&window_class[..window_len], &root_class[..root_len])
    }
}

/// Where the desktop icons are, in screen pixels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconMap {
    rects: Vec<Rect>,
}

impl IconMap {
    pub fn new(rects: Vec<Rect>) -> Self {
        Self { rects }
    }

    /// No icons known (usable in a `static`).
    pub const fn empty() -> Self {
        Self { rects: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Is the point on an icon (its whole cell, label included)?
    pub fn hit(&self, x: i32, y: i32) -> bool {
        self.rects.iter().any(|r| r.contains_point(x, y))
    }
}

/// How long a swallowed right-button press waits for its release before the
/// release is let through (a stuck request must never eat a later click).
const RELEASE_TIMEOUT_MS: u64 = 2_000;

/// What the hook does with a right-button event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Let it through to the desktop.
    Pass,
    /// Swallow the press; a menu request follows at this point.
    SwallowDown,
    /// Swallow the release that belongs to a swallowed press.
    SwallowUp,
}

/// Which edge of the right button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Down,
    Up,
}

/// One right-button event, as the hook sees it.
#[derive(Debug, Clone, Copy)]
pub struct Click {
    pub edge: Edge,
    /// Screen pixels.
    pub at: (i32, i32),
    pub now_ms: u64,
    /// Input we injected ourselves (the native-menu replay).
    pub ours: bool,
    /// The escape hatch: Shift+right-click is always the native menu.
    pub shift: bool,
}

/// The hook's decision logic: swallow a right-click that lands on the bare
/// desktop (press and release together), pass everything else. Pure, so the
/// rules are tested without a hook.
#[derive(Debug, Default)]
pub struct Swallower {
    /// When the swallowed press happened.
    pending_since_ms: Option<u64>,
}

impl Swallower {
    /// `probe` runs only when the answer depends on it, and only on a press.
    pub fn decide(
        &mut self,
        click: Click,
        icons: &IconMap,
        probe: impl FnOnce() -> Under,
    ) -> Verdict {
        if click.ours {
            return Verdict::Pass;
        }
        match click.edge {
            Edge::Down => {
                self.pending_since_ms = None;
                if click.shift || icons.hit(click.at.0, click.at.1) || probe() != Under::Desktop {
                    return Verdict::Pass;
                }
                self.pending_since_ms = Some(click.now_ms);
                Verdict::SwallowDown
            }
            Edge::Up => match self.pending_since_ms.take() {
                Some(since) if click.now_ms.saturating_sub(since) <= RELEASE_TIMEOUT_MS => {
                    Verdict::SwallowUp
                }
                _ => Verdict::Pass,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn class(window: &str, root: &str) -> Under {
        classify(&units(window), &units(root))
    }

    fn icons() -> IconMap {
        IconMap::new(vec![
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 100,
            },
            Rect {
                x: 0,
                y: 100,
                width: 80,
                height: 100,
            },
        ])
    }

    #[test]
    fn the_desktop_layer_is_the_icon_view_under_the_shell_not_a_folder_window() {
        assert_eq!(class("SysListView32", "Progman"), Under::Desktop);
        assert_eq!(class("SHELLDLL_DefView", "Progman"), Under::Desktop);
        assert_eq!(
            class("SysListView32", "WorkerW"),
            Under::Desktop,
            "legacy layout"
        );
        assert_eq!(class("mypaper.wallpaper", "Progman"), Under::Desktop);
        assert_eq!(class("Progman", "Progman"), Under::Desktop);
        // An Explorer folder window has the same inner classes.
        assert_eq!(class("SysListView32", "CabinetWClass"), Under::Other);
        assert_eq!(
            class("Chrome_RenderWidgetHostHWND", "Progman"),
            Under::Other,
            "a widget"
        );
        assert_eq!(class("Shell_TrayWnd", "Shell_TrayWnd"), Under::Other);
        assert_eq!(class("", ""), Under::Other);
    }

    #[test]
    fn icons_are_hit_by_their_cell() {
        let map = icons();
        assert!(map.hit(10, 10) && map.hit(79, 199) && map.hit(0, 100));
        assert!(!map.hit(80, 10) && !map.hit(10, 200) && !map.hit(-1, 0));
        assert!(!IconMap::default().hit(0, 0) && IconMap::default().is_empty());
        assert_eq!(map.len(), 2);
    }

    fn click(edge: Edge, at: (i32, i32), now_ms: u64) -> Click {
        Click {
            edge,
            at,
            now_ms,
            ours: false,
            shift: false,
        }
    }

    #[test]
    fn a_right_click_on_empty_desktop_is_swallowed_as_a_pair() {
        let mut s = Swallower::default();
        let map = icons();
        let down = s.decide(click(Edge::Down, (500, 500), 1_000), &map, || {
            Under::Desktop
        });
        assert_eq!(down, Verdict::SwallowDown);
        let up = s.decide(click(Edge::Up, (500, 500), 1_090), &map, || unreachable!());
        assert_eq!(up, Verdict::SwallowUp);
        // The pair is consumed: a stray release passes.
        let stray = s.decide(click(Edge::Up, (500, 500), 1_100), &map, || unreachable!());
        assert_eq!(stray, Verdict::Pass);
    }

    #[test]
    fn icons_other_windows_shift_and_our_own_replay_always_pass() {
        let mut s = Swallower::default();
        let map = icons();
        // On an icon the probe is not even asked.
        let on_icon = click(Edge::Down, (10, 10), 0);
        assert_eq!(s.decide(on_icon, &map, || unreachable!()), Verdict::Pass);
        let release = click(Edge::Up, (10, 10), 5);
        assert_eq!(s.decide(release, &map, || unreachable!()), Verdict::Pass);
        // An app window.
        let on_app = click(Edge::Down, (500, 500), 0);
        assert_eq!(s.decide(on_app, &map, || Under::Other), Verdict::Pass);
        let release = click(Edge::Up, (500, 500), 5);
        assert_eq!(s.decide(release, &map, || unreachable!()), Verdict::Pass);
        // Shift: native menu, probe not asked.
        let shifted = Click {
            shift: true,
            ..click(Edge::Down, (500, 500), 0)
        };
        assert_eq!(s.decide(shifted, &map, || unreachable!()), Verdict::Pass);
        // The replayed click.
        for edge in [Edge::Down, Edge::Up] {
            let ours = Click {
                ours: true,
                ..click(edge, (500, 500), 0)
            };
            assert_eq!(s.decide(ours, &map, || unreachable!()), Verdict::Pass);
        }
    }

    #[test]
    fn a_swallowed_press_does_not_swallow_a_much_later_release() {
        let mut s = Swallower::default();
        let map = IconMap::default();
        let down = s.decide(click(Edge::Down, (5, 5), 0), &map, || Under::Desktop);
        assert_eq!(down, Verdict::SwallowDown);
        let late = click(Edge::Up, (5, 5), RELEASE_TIMEOUT_MS + 1);
        assert_eq!(s.decide(late, &map, || unreachable!()), Verdict::Pass);
    }

    #[test]
    fn a_new_press_replaces_an_unreleased_one() {
        let mut s = Swallower::default();
        let map = IconMap::default();
        s.decide(click(Edge::Down, (5, 5), 0), &map, || Under::Desktop);
        // The release never came (hook was off for a moment); the next press is on an app.
        let on_app = click(Edge::Down, (5, 5), 100);
        assert_eq!(s.decide(on_app, &map, || Under::Other), Verdict::Pass);
        let release = click(Edge::Up, (5, 5), 150);
        assert_eq!(s.decide(release, &map, || unreachable!()), Verdict::Pass);
    }
}
