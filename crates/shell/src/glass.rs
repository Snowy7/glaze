//! The shell's single glass boundary: merges every source's plates into the one snapshot the
//! engine draws (`mypaper-glass-ipc`). Callers pass monitor-local physical pixels and publish
//! only when their plates change (and every frame while something moves); nothing at rest.
use std::cell::RefCell;
use std::collections::BTreeMap;

use mypaper_desktop::Monitor;
use mypaper_glass_ipc as ipc;
use mypaper_ui::Rect;
use mypaper_ui::theme::Knobs;

#[path = "glass_tone.rs"]
pub mod tone;

/// A plate's material: the kit's, so a surface's glass (`theme::material` tuned by the user's
/// [`Knobs`]) is published as it is. Blur here is physical px.
pub type Material = mypaper_ui::GlassMaterial;

/// The same quiet Liquid Glass for widgets, menus and flyouts. Theme/instance controls are
/// applied by the caller; the widget look owns the built-in optical defaults.
pub fn liquid_material(tint: mypaper_theme::Color) -> Material {
    let defaults = mypaper_widgets::look::liquid_defaults();
    let alpha = defaults.tint.expect("built-in tint");
    Material {
        clarity: 1.0 - alpha,
        blur: defaults.blur.expect("built-in blur"),
        refraction: defaults.refraction.expect("built-in refraction"),
        highlight: defaults.highlight.expect("built-in highlight"),
        tint: mypaper_ui::Rgba::rgb8(tint.0, alpha),
    }
}

/// Which of the look's families a surface belongs to (`config.json` › `look.bars`, `widgets`,
/// `menus`: "Different for").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Bars,
    Widgets,
    /// Menus and panels: flyouts too.
    Menus,
}

/// The user's material controls for `family`: its own over the look's (`config.json` ›
/// `look`), which Studio's Look sliders edit.
pub fn knobs(look: &mypaper_config::Look, family: Family) -> Knobs {
    let own = match family {
        Family::Bars => look.bars.as_ref(),
        Family::Widgets => look.widgets.as_ref(),
        Family::Menus => look.menus.as_ref(),
    };
    let m = own.map_or(look.material, |f| f.material.over(&look.material));
    Knobs {
        clarity: m.clarity,
        blur: m.blur,
        refraction: m.refraction,
        highlight: m.highlight,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    /// Stable per surface (widgets use their HWND). 0 lets the merger assign one.
    pub id: u64,
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
    pub style: ipc::Style,
}

/// Who owns a set of plates; each source replaces only its own per monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    #[allow(
        dead_code,
        reason = "the standalone widget fixture publishes only Widgets"
    )]
    Bars,
    Widgets,
}

#[derive(Default)]
struct Merger {
    sets: BTreeMap<(Source, String), Vec<ipc::Plate>>,
    styles: BTreeMap<(Source, String), Vec<ipc::Style>>,
    writer: Option<ipc::Writer>,
    failed: bool,
    published: Vec<ipc::Plate>,
    published_styles: Vec<ipc::Style>,
}

thread_local! {
    static MERGER: RefCell<Merger> = RefCell::default();
}

/// Replace `source`'s plates on `monitor` (including `[]` when its surfaces disappear) and
/// republish the merged snapshot.
pub fn publish(source: Source, monitor: &Monitor, plates: &[Plate]) {
    let (ox, oy) = (monitor.rect.x as f32, monitor.rect.y as f32);
    let converted = plates
        .iter()
        .map(|p| ipc::Plate {
            id: p.id,
            rect: [
                p.rect.left + ox,
                p.rect.top + oy,
                p.rect.right - p.rect.left,
                p.rect.bottom - p.rect.top,
            ],
            radius: p.radius,
            material: ipc::Material {
                clarity: p.material.clarity,
                blur: p.material.blur.clamp(0.0, 256.0),
                refraction: p.material.refraction,
                highlight: p.material.highlight,
                tint: [
                    p.material.tint.r,
                    p.material.tint.g,
                    p.material.tint.b,
                    p.material.tint.a,
                ],
            },
        })
        .collect();
    MERGER.with_borrow_mut(|m| {
        m.styles.insert(
            (source, monitor.device.clone()),
            plates.iter().map(|p| p.style).collect(),
        );
        m.sets.insert((source, monitor.device.clone()), converted);
        m.flush();
    });
}

impl Merger {
    fn flush(&mut self) {
        if self.writer.is_none() && !self.failed {
            match ipc::Writer::open() {
                Ok(writer) => self.writer = Some(writer),
                Err(e) => {
                    self.failed = true;
                    log::error!("glass transport unavailable: {e:#}");
                }
            }
        }
        let Some(writer) = &mut self.writer else {
            return;
        };
        let all = merged(&self.sets);
        let styles: Vec<_> = self.styles.values().flatten().copied().collect();
        if all.len() > ipc::MAX_PLATES {
            log::error!(
                "{} plates exceeds the whole-desktop limit of {}",
                all.len(),
                ipc::MAX_PLATES
            );
            return;
        }
        if all == self.published && styles == self.published_styles {
            return;
        }
        if let Err(e) = writer.publish_styled(&all, &styles) {
            log::error!("glass publish: {e:#}");
        } else {
            self.published = all;
            self.published_styles = styles;
        }
    }
}

/// Auto ids include the monitor group as well as the source and local shape index.
/// Explicit ids (widget HWNDs) stay intact for overlay exclusion.
fn merged(sets: &BTreeMap<(Source, String), Vec<ipc::Plate>>) -> Vec<ipc::Plate> {
    sets.iter()
        .enumerate()
        .flat_map(|(group, ((source, _), plates))| {
            plates.iter().enumerate().map(move |(i, p)| ipc::Plate {
                id: if p.id == 0 {
                    ((*source as u64 + 1) << 56) | ((group as u64) << 32) | i as u64
                } else {
                    p.id
                },
                ..*p
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_snapshot_preserves_both_monitors_and_widgets_with_unique_ids() {
        let plate = ipc::Plate {
            id: 0,
            rect: [-2560.0, 8.0, 2400.0, 32.0],
            radius: 12.0,
            material: ipc::Material::default(),
        };
        let mut sets = BTreeMap::from([
            ((Source::Bars, "DISPLAY1".into()), vec![plate, plate]),
            ((Source::Bars, "DISPLAY2".into()), vec![plate, plate]),
            (
                (Source::Widgets, "DISPLAY1".into()),
                vec![ipc::Plate { id: 12345, ..plate }],
            ),
        ]);
        let all = merged(&sets);
        assert_eq!(all.len(), 5);
        let ids: std::collections::BTreeSet<_> = all.iter().map(|p| p.id).collect();
        assert_eq!(
            ids.len(),
            all.len(),
            "auto ids must not repeat on the second monitor"
        );
        assert_eq!(all[4].id, 12345, "widget HWND identity is preserved");
        sets.insert((Source::Bars, "DISPLAY1".into()), vec![]);
        let remaining = merged(&sets);
        assert_eq!(remaining.len(), 3);
        assert!(remaining.iter().any(|p| p.id == 12345));
        assert_eq!(
            remaining[0].rect, plate.rect,
            "geometry is used without interpolation"
        );
    }
}
