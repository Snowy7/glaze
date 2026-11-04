//! Liquid glass plates (docs/widgets.md, "Liquid glass"): where the wallpaper engine draws the
//! glass under liquid widgets. Each desktop shell reports the cards of its liquid widgets (DIPs
//! from its monitor's top-left, also while one is dragged); the app turns them into the physical,
//! virtual-screen plates of `mypaper_theme::glass` with the glass knobs, and [`Plates`] publishes
//! them at most every [`THROTTLE`], always ending on the latest. While no widget is liquid nothing
//! is written, so the engine allocates and draws nothing.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use mypaper_theme::glass::{self, Plate};
use mypaper_theme::{Color, Liquid};
use serde::Deserialize;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW,
};
use windows::core::{HSTRING, PCWSTR, w};

/// This host's file in the glass folder.
const SOURCE: &str = "widgets";
/// The fastest the file is rewritten: a drag reports every frame, and the engine's springs
/// smooth the steps between writes.
pub const THROTTLE: Duration = Duration::from_millis(33);

/// A liquid widget's card as its shell reports it: `[x, y, width, height]` in DIPs from the
/// monitor's top-left, and its corner radius.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub instance: String,
    pub rect: [f64; 4],
    pub radius: f64,
}

/// The plate under `card`, on a monitor whose top-left is `origin` (physical px) at `scale`
/// (DPI / 96), with `knobs` (every field set) and the appearance's `tint` color.
pub fn plate(card: &Card, origin: (i32, i32), scale: f64, knobs: &Liquid, tint: Color) -> Plate {
    // Edges are rounded, not sizes, so plates side by side keep their gap.
    let px = |v: f64| (v * scale).round() as i32;
    let [x, y, w, h] = card.rect;
    let (x0, y0) = (px(x), px(y));
    let unit = |v: Option<f32>| v.unwrap_or(0.0).clamp(0.0, 1.0);
    let Color([r, g, b]) = tint;
    Plate {
        id: card.instance.clone(),
        rect: [
            origin.0 + x0,
            origin.1 + y0,
            (px(x + w) - x0).max(1),
            (px(y + h) - y0).max(1),
        ],
        radius: (card.radius.max(0.0) * scale) as f32,
        intensity: unit(knobs.intensity),
        refraction: unit(knobs.refraction),
        highlight: unit(knobs.highlight),
        tint: [r, g, b, (unit(knobs.tint) * 255.0).round() as u8],
        blur: (f64::from(knobs.blur.unwrap_or(0.0).clamp(0.0, 32.0)) * scale) as f32,
    }
}

/// The cards each monitor's shell reported, and what is published of them.
#[derive(Default)]
pub struct Plates {
    /// Monitor device → its liquid cards.
    cards: BTreeMap<String, Vec<Card>>,
    /// What the file holds; `None` until the first write (so a host that never shows a liquid
    /// widget never touches the folder).
    written: Option<Vec<Plate>>,
    /// What should be published, when it differs from `written`.
    next: Vec<Plate>,
    last_write: Option<Instant>,
    due: Option<Instant>,
}

impl Plates {
    /// Replaces the cards `device`'s shell shows.
    pub fn set_cards(&mut self, device: &str, cards: Vec<Card>) {
        if cards.is_empty() {
            self.cards.remove(device);
        } else {
            self.cards.insert(device.to_owned(), cards);
        }
    }

    /// Forgets the monitors whose desktop layer went away.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.cards.retain(|device, _| keep(device));
    }

    /// Every monitor's cards.
    pub fn cards(&self) -> impl Iterator<Item = (&str, &[Card])> {
        self.cards.iter().map(|(d, c)| (d.as_str(), c.as_slice()))
    }

    /// Makes `plates` what should be published: due at once, or [`THROTTLE`] after the last
    /// write.
    pub fn offer(&mut self, plates: Vec<Plate>, now: Instant) {
        let same = match &self.written {
            Some(written) => *written == plates,
            None => plates.is_empty(),
        };
        self.next = plates;
        if same {
            self.due = None;
        } else if self.due.is_none() {
            self.due = Some(self.last_write.map_or(now, |t| (t + THROTTLE).max(now)));
        }
    }

    /// When a write is due.
    pub fn deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Takes what to publish, if a write is due at `now`.
    pub fn take_due(&mut self, now: Instant) -> Option<Vec<Plate>> {
        self.due.take_if(|at| *at <= now)?;
        self.last_write = Some(now);
        self.written = Some(self.next.clone());
        Some(self.next.clone())
    }

    /// Whether the file holds plates (so the engine must hear that they are gone).
    pub fn published(&self) -> bool {
        self.written.as_ref().is_some_and(|w| !w.is_empty())
    }
}

/// Writes `plates` (no plates: removes the file) and tells the wallpaper engine, if it runs.
pub fn publish(plates: Vec<Plate>) {
    let written = if plates.is_empty() {
        glass::remove(SOURCE)
    } else {
        glass::write(
            SOURCE,
            &glass::Plates {
                pid: std::process::id(),
                plates,
            },
        )
    };
    if let Err(e) = written {
        return log::warn!("glass plates: {e:#}");
    }
    let name = HSTRING::from(glass::MESSAGE);
    // SAFETY: plain window lookups and a post to a message-only window; a missing window (no
    // wallpaper running) is fine, it reads the file when it starts.
    unsafe {
        let Ok(window) = FindWindowExW(
            Some(HWND_MESSAGE),
            None,
            w!("mypaper.wallpaper.control"),
            PCWSTR::null(),
        ) else {
            return;
        };
        let message = RegisterWindowMessageW(&name);
        let _ = PostMessageW(Some(window), message, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knobs() -> Liquid {
        Liquid {
            intensity: Some(1.0),
            refraction: Some(0.6),
            highlight: Some(0.5),
            tint: Some(0.1),
            blur: Some(2.0),
        }
    }

    #[test]
    fn a_card_becomes_a_physical_plate_on_its_monitor() {
        let card = Card {
            instance: "clock-1".into(),
            rect: [10.0, 20.5, 100.0, 50.0],
            radius: 18.0,
        };
        let got = plate(&card, (-2560, 0), 1.5, &knobs(), Color([16, 16, 16]));
        assert_eq!(got.id, "clock-1");
        assert_eq!(got.rect, [-2560 + 15, 31, 150, 75]);
        assert_eq!(got.radius, 27.0);
        assert_eq!(got.tint, [16, 16, 16, 26]);
        assert_eq!(got.blur, 3.0, "blur is DIPs too");
        assert_eq!(
            (got.intensity, got.refraction, got.highlight),
            (1.0, 0.6, 0.5)
        );
    }

    #[test]
    fn publishes_only_changes_at_most_every_throttle_and_always_the_latest() {
        let start = Instant::now();
        let mut plates = Plates::default();
        let one = |x: i32| {
            vec![plate(
                &Card {
                    instance: "a".into(),
                    rect: [f64::from(x), 0.0, 10.0, 10.0],
                    radius: 2.0,
                },
                (0, 0),
                1.0,
                &knobs(),
                Color([0, 0, 0]),
            )]
        };

        plates.offer(Vec::new(), start);
        assert_eq!(
            plates.deadline(),
            None,
            "nothing liquid: nothing is ever written"
        );

        plates.offer(one(0), start);
        assert_eq!(
            plates.take_due(start),
            Some(one(0)),
            "the first change goes out at once"
        );
        assert!(plates.published());

        // A drag: three moves within one throttle window publish once, the last of them.
        let soon = start + Duration::from_millis(5);
        plates.offer(one(1), soon);
        plates.offer(one(2), soon);
        assert_eq!(plates.take_due(soon), None, "throttled");
        assert_eq!(plates.deadline(), Some(start + THROTTLE));
        plates.offer(one(3), start + Duration::from_millis(20));
        assert_eq!(plates.take_due(start + THROTTLE), Some(one(3)));

        // Back where it was written: nothing to do. Gone: an empty write tells the engine.
        plates.offer(one(3), start + THROTTLE * 3);
        assert_eq!(plates.deadline(), None);
        plates.offer(Vec::new(), start + THROTTLE * 3);
        assert_eq!(plates.take_due(start + THROTTLE * 3), Some(Vec::new()));
        assert!(!plates.published());
    }
}
