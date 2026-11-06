//! Temporary panel avoidance. The home comes from Studio; no placement is persisted here.
use mypaper_desktop::Monitor;
use mypaper_ui::{Point, Rect};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    },
};

const DURATION: Duration = Duration::from_millis(170);

#[derive(Default)]
pub(super) struct Position {
    home: Option<Point>,
    destination: Option<Point>,
    motion: Option<Motion>,
}

struct Motion {
    from: Point,
    to: Point,
    start: Instant,
}

impl Motion {
    fn at(&self, now: Instant) -> Point {
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / DURATION.as_secs_f32())
            .min(1.0);
        let ease = t * t * (3.0 - 2.0 * t);
        Point::new(
            self.from.x + (self.to.x - self.from.x) * ease,
            self.from.y + (self.to.y - self.from.y) * ease,
        )
    }
}

impl Position {
    pub fn home(&mut self, x: i32, y: i32) {
        if x.abs_diff(0) <= 100_000 && y.abs_diff(0) <= 100_000 {
            self.home = Some(Point::new(x as f32, y as f32));
        }
    }

    pub fn animating(&self) -> bool {
        self.motion.is_some()
    }
    pub fn away(&self) -> bool {
        self.destination.is_some() && self.destination != self.home
    }

    pub fn tick(&mut self, panel: HWND, monitors: &[Monitor], avoid: &[Rect]) {
        let Some(home) = self.home else {
            return;
        };
        let (mut outer, mut client, mut origin) =
            (RECT::default(), RECT::default(), POINT::default());
        // SAFETY: panel was authenticated against the session owner; queries only its geometry.
        let queried = unsafe {
            GetWindowRect(panel, &mut outer).is_ok()
                && GetClientRect(panel, &mut client).is_ok()
                && ClientToScreen(panel, &mut origin).as_bool()
        };
        if !queried {
            return;
        }
        let size = (
            (client.right - client.left) as f32,
            (client.bottom - client.top) as f32,
        );
        let areas: Vec<_> = monitors
            .iter()
            .map(|m| {
                let s = m.dpi as f32 / 96.0;
                Rect::new(
                    m.work.x as f32 + 8.0 * s,
                    (m.work.y as f32).max(m.rect.y as f32 + 40.0 * s),
                    m.work.width as f32 - 16.0 * s,
                    (m.work.y + m.work.height) as f32
                        - 96.0 * s
                        - (m.work.y as f32).max(m.rect.y as f32 + 40.0 * s),
                )
            })
            .collect();
        let desired = destination(home, size, &areas, avoid);
        let now = Instant::now();
        let current = Point::new(origin.x as f32, origin.y as f32);
        // Studio can refit a newly measured page while the selection remains unchanged.
        // Reconcile that native move too, instead of losing avoidance until the next pick.
        if self.destination != Some(desired) || self.motion.is_none() && current != desired {
            self.destination = Some(desired);
            self.motion = Some(Motion {
                from: current,
                to: desired,
                start: now,
            });
        }
        let Some(motion) = &self.motion else {
            return;
        };
        let at = motion.at(now);
        let finished = now.saturating_duration_since(motion.start) >= DURATION;
        // SAFETY: only the authenticated Studio panel moves. Focus, size and Z-order stay intact.
        unsafe {
            let _ = SetWindowPos(
                panel,
                None,
                at.x.round() as i32 - (origin.x - outer.left),
                at.y.round() as i32 - (origin.y - outer.top),
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        if finished {
            self.motion = None;
        }
    }
}

/// Prefer the other edge of the inspected display; another display's near edge is a fallback
/// when the widget spans both edges. Always evaluate against home so moving never oscillates.
fn destination(home: Point, size: (f32, f32), areas: &[Rect], avoid: &[Rect]) -> Point {
    let panel = |p: Point| Rect::new(p.x, p.y, size.0, size.1);
    let overlap = |p| {
        avoid
            .iter()
            .map(|r| {
                let r = panel(p).intersect(r.inflate(6.0));
                r.width().max(0.0) * r.height().max(0.0)
            })
            .sum::<f32>()
    };
    let home_overlap = overlap(home);
    if home_overlap == 0.0 {
        return home;
    }
    let home_display = areas.iter().position(|r| r.contains(panel(home).center()));
    let mut candidates = Vec::new();
    for (i, area) in areas.iter().enumerate() {
        if size.0 > area.width() || size.1 > area.height() + 1.0 {
            continue;
        }
        let y = home.y.clamp(area.top, (area.bottom - size.1).max(area.top));
        for x in [area.left, area.right - size.0] {
            let point = Point::new(x, y);
            let distance = (point.x - home.x).abs() + (point.y - home.y).abs();
            candidates.push((point, overlap(point), Some(i) != home_display, distance));
        }
    }
    candidates.sort_by(|a, b| {
        a.1.total_cmp(&b.1)
            .then(a.2.cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
    });
    candidates
        .first()
        .filter(|c| c.1 < home_overlap)
        .map_or(home, |c| c.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_to_the_other_edge_then_returns_to_the_remembered_side() {
        let area = Rect::new(8., 40., 1904., 944.);
        let home = Point::new(1432., 40.);
        let widget = Rect::new(1500., 90., 300., 250.);
        let aside = destination(home, (480., 800.), &[area], &[widget]);
        assert_eq!(aside, Point::new(8., 40.));
        assert_eq!(destination(home, (480., 800.), &[area], &[widget]), aside);
        assert_eq!(destination(home, (480., 800.), &[area], &[]), home);
    }
    #[test]
    fn uses_the_other_displays_near_edge_when_both_edges_are_covered() {
        let areas = [
            Rect::new(-1912., 40., 1904., 944.),
            Rect::new(8., 40., 1904., 944.),
        ];
        let home = Point::new(1432., 40.);
        assert_eq!(
            destination(
                home,
                (480., 800.),
                &areas,
                &[Rect::new(0., 60., 1920., 900.)]
            ),
            Point::new(-488., 40.)
        );
        assert_eq!(
            destination(
                home,
                (480., 800.),
                &areas,
                &[Rect::new(-1800., 60., 200., 200.)]
            ),
            home
        );
    }
    #[test]
    fn animation_reaches_both_endpoints_and_has_a_short_smooth_middle() {
        let now = Instant::now();
        let motion = Motion {
            from: Point::new(0., 40.),
            to: Point::new(1000., 40.),
            start: now,
        };
        assert_eq!(motion.at(now), motion.from);
        assert_eq!(motion.at(now + DURATION / 2), Point::new(500., 40.));
        assert_eq!(motion.at(now + DURATION), motion.to);
    }
}
