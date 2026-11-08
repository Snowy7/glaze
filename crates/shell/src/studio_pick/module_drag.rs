//! Desktop module transfers share one validated operation with Studio's release handler.
use super::wire::PreviewOp;
use glaze_bars::BarsConfig;
use glaze_ui::{AnimationFrames, Surface, SurfaceId};
use glaze_ui::{Point, Rect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::Win32::Foundation::HWND;

pub(super) struct Gesture {
    pub selection: super::wire::Selection,
    pub from: Slot,
    pub destination: Option<Slot>,
    pub marker: Option<Rect>,
    pub preview: Option<Preview>,
    pub base_revision: u64,
    pub capture: SurfaceId,
    window: HWND,
    start: Point,
    zones: Vec<Zone>,
    raw: Value,
    primary: bool,
    scoped: bool,
    _frames: AnimationFrames,
}

impl Gesture {
    pub fn new(
        selection: super::wire::Selection,
        surface: &Surface,
        start: Point,
        zones: Vec<Zone>,
        raw: Value,
        primary: bool,
        scoped: bool,
    ) -> Option<Self> {
        let from = Slot::parse(&selection.id)?;
        let base_revision = super::wire::revision(&raw);
        // SAFETY: capture belongs to this shell-owned no-activate name chip, on the UI thread.
        unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(surface.hwnd());
        }
        Some(Self {
            selection,
            from,
            destination: None,
            marker: None,
            preview: None,
            base_revision,
            capture: surface.id(),
            window: surface.hwnd(),
            start,
            zones,
            raw,
            primary,
            scoped,
            _frames: surface.animation_frames(std::time::Duration::from_secs(3600)),
        })
    }
    pub fn update(&mut self, point: Point) {
        if (point.x - self.start.x)
            .abs()
            .max((point.y - self.start.y).abs())
            < 5.0
        {
            return;
        }
        let target = self
            .zones
            .iter()
            .find(|z| z.display == self.selection.display && z.rect.inflate(10.0).contains(point));
        let Some((slot, marker)) = target.map(|z| z.destination(point, &self.from)) else {
            self.destination = None;
            self.marker = None;
            self.preview = None;
            return;
        };
        if self.destination.as_ref() == Some(&slot) {
            return;
        }
        let ops = move_ops(
            &self.raw,
            &self.selection.display,
            self.primary,
            self.scoped,
            &self.from,
            &slot,
        );
        self.destination = Some(slot);
        self.marker = Some(marker);
        self.preview = ops.map(|ops| Preview {
            base_revision: self.base_revision,
            saved_revision: None,
            ops,
        });
    }
}
impl Drop for Gesture {
    fn drop(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
        // SAFETY: never release capture belonging to a different shell interaction.
        unsafe {
            if GetCapture() == self.window {
                let _ = ReleaseCapture();
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub bar: String,
    pub zone: String,
    pub index: usize,
}

impl Slot {
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.split('/');
        let slot = Self {
            bar: parts.next()?.into(),
            zone: parts.next()?.into(),
            index: parts.next()?.parse().ok()?,
        };
        (parts.next().is_none() && slot.valid()).then_some(slot)
    }
    pub fn valid(&self) -> bool {
        matches!(
            (self.bar.as_str(), self.zone.as_str()),
            ("top", "left" | "center" | "right") | ("dock", "left" | "right")
        )
    }
    pub fn id(&self) -> String {
        format!("{}/{}/{}", self.bar, self.zone, self.index)
    }
    fn path(&self) -> String {
        format!("/{}/{}", self.bar, self.zone)
    }
}

#[derive(Clone)]
pub struct Zone {
    pub display: String,
    pub bar: String,
    pub zone: String,
    pub rect: Rect,
    pub horizontal: bool,
    pub items: Vec<(usize, Rect)>,
    pub len: usize,
}

impl Zone {
    pub fn destination(&self, point: Point, from: &Slot) -> (Slot, Rect) {
        let coordinate = if self.horizontal { point.x } else { point.y };
        let before_hit = self
            .items
            .iter()
            .filter(|(_, r)| {
                coordinate
                    > if self.horizontal {
                        (r.left + r.right) / 2.0
                    } else {
                        (r.top + r.bottom) / 2.0
                    }
            })
            .count();
        let mut before = self
            .items
            .get(before_hit)
            .map_or(self.len, |(index, _)| *index);
        let edge = self
            .items
            .get(before_hit)
            .map(|(_, r)| if self.horizontal { r.left } else { r.top })
            .or_else(|| {
                self.items
                    .last()
                    .map(|(_, r)| if self.horizontal { r.right } else { r.bottom })
            })
            .unwrap_or(if self.horizontal {
                self.rect.left + self.rect.width() / 2.0
            } else {
                self.rect.top + self.rect.height() / 2.0
            });
        if self.bar == from.bar && self.zone == from.zone && from.index < before {
            before -= 1;
        }
        let marker = if self.horizontal {
            Rect::new(
                edge - 1.5,
                self.rect.top + 3.0,
                3.0,
                (self.rect.height() - 6.0).max(6.0),
            )
        } else {
            Rect::new(
                self.rect.left + 3.0,
                edge - 1.5,
                (self.rect.width() - 6.0).max(6.0),
                3.0,
            )
        };
        (
            Slot {
                bar: self.bar.clone(),
                zone: self.zone.clone(),
                index: before,
            },
            marker,
        )
    }
}

#[derive(Clone)]
pub struct Preview {
    pub base_revision: u64,
    pub saved_revision: Option<u64>,
    pub ops: Vec<PreviewOp>,
}
impl Preview {
    pub fn current(&self, revision: u64) -> bool {
        self.base_revision == revision && self.saved_revision != Some(revision)
    }
}

/// Materialize only the affected display's lists, preserving all unrelated raw fields.
/// `to.index` is the insertion position after removal from the source list.
pub fn move_ops(
    raw: &Value,
    display: &str,
    primary: bool,
    scoped: bool,
    from: &Slot,
    to: &Slot,
) -> Option<Vec<PreviewOp>> {
    if !from.valid() || !to.valid() || from == to {
        return None;
    }
    let config = BarsConfig::parse(&raw.to_string())
        .ok()?
        .for_monitor(display, primary)
        .ok()?;
    let floating = config.dock.style == glaze_bars::config::DockStyle::Floating;
    let explicit = ["left", "right"].map(|zone| {
        config
            .raw()
            .get("dock")
            .is_some_and(|dock| dock.get(zone).is_some())
    });
    let mut resolved = serde_json::to_value(config).ok()?;
    // A floating dock's implicit taskbar defaults are hidden. The first added module
    // starts an empty visible list rather than unexpectedly adding Start and Search.
    if floating {
        for (zone, explicit) in ["left", "right"].into_iter().zip(explicit) {
            if !explicit {
                resolved["dock"][zone] = serde_json::json!([]);
            }
        }
    }
    let mut source = resolved.pointer(&from.path())?.as_array()?.clone();
    if from.index >= source.len() {
        return None;
    }
    let item = source.remove(from.index);
    let path = |slot: &Slot| {
        if scoped {
            format!(
                "/overrides/{}{}",
                display
                    .trim_start_matches(r"\\.\")
                    .replace('~', "~0")
                    .replace('/', "~1"),
                slot.path()
            )
        } else {
            slot.path()
        }
    };
    let validated = |mut ops: Vec<PreviewOp>| {
        if floating && (from.bar == "dock" || to.bar == "dock") {
            for (zone, explicit) in ["left", "right"].into_iter().zip(explicit) {
                let path = path(&Slot {
                    bar: "dock".into(),
                    zone: zone.into(),
                    index: 0,
                });
                if !explicit && !ops.iter().any(|op| op.path == path) {
                    ops.push(PreviewOp {
                        path,
                        value: serde_json::json!([]),
                    });
                }
            }
        }
        let value = super::wire::patched(raw, &ops)?;
        BarsConfig::parse(&value.to_string())
            .ok()?
            .for_monitor(display, primary)
            .ok()?;
        Some(ops)
    };
    if from.bar == to.bar && from.zone == to.zone {
        if to.index > source.len() {
            return None;
        }
        source.insert(to.index, item);
        return validated(vec![PreviewOp {
            path: path(from),
            value: Value::Array(source),
        }]);
    }
    let mut destination = resolved.pointer(&to.path())?.as_array()?.clone();
    if to.index > destination.len() {
        return None;
    }
    destination.insert(to.index, item);
    validated(vec![
        PreviewOp {
            path: path(from),
            value: Value::Array(source),
        },
        PreviewOp {
            path: path(to),
            value: Value::Array(destination),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn transfers_preserve_duplicates_and_display_overrides() {
        let raw = json!({"top":{"left":["|","|","clock"],"center":[],"right":[]},"dock":{"left":[],"right":[]},"overrides":{"DISPLAY2":{"top":{"left":["media","clock"]}}}});
        let from = Slot::parse("top/left/0").unwrap();
        let to = Slot::parse("dock/left/0").unwrap();
        let ops = move_ops(&raw, r"\\.\DISPLAY2", false, true, &from, &to).unwrap();
        let result = super::super::wire::patched(&raw, &ops).unwrap();
        assert_eq!(result["top"], raw["top"]);
        assert_eq!(
            result["overrides"]["DISPLAY2"]["top"]["left"],
            json!(["clock"])
        );
        assert_eq!(
            result["overrides"]["DISPLAY2"]["dock"]["left"],
            json!(["media"])
        );
        assert!(BarsConfig::parse(&result.to_string()).is_ok());
        assert!(
            move_ops(
                &raw,
                "DISPLAY2",
                false,
                true,
                &from,
                &Slot::parse("dock/left/99").unwrap()
            )
            .is_none()
        );
    }
    #[test]
    fn insertion_uses_post_removal_indices_and_supports_vertical_bars() {
        let zone = Zone {
            display: "D".into(),
            bar: "top".into(),
            zone: "left".into(),
            rect: Rect::new(0., 0., 200., 30.),
            horizontal: true,
            items: vec![
                (0, Rect::new(0., 0., 30., 30.)),
                (1, Rect::new(40., 0., 30., 30.)),
            ],
            len: 2,
        };
        let from = Slot::parse("top/left/0").unwrap();
        assert_eq!(zone.destination(Point::new(100., 10.), &from).0.index, 1);
        let hidden = Zone {
            items: vec![
                (2, Rect::new(0., 0., 30., 30.)),
                (5, Rect::new(40., 0., 30., 30.)),
            ],
            len: 8,
            ..zone.clone()
        };
        assert_eq!(
            hidden
                .destination(Point::new(38., 10.), &Slot::parse("top/left/2").unwrap())
                .0
                .index,
            4,
            "insertion preserves separators and modules with no visible output"
        );
        assert_eq!(hidden.destination(Point::new(100., 10.), &from).0.index, 7);
        let vertical = Zone {
            horizontal: false,
            items: vec![],
            ..zone
        };
        let (_, marker) = vertical.destination(Point::new(5., 5.), &from);
        assert_eq!(marker.height(), 3.);
    }
}
