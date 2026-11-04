//! The shell's single glass boundary: merges every source's plates into the one snapshot the
//! engine draws (`mypaper-glass-ipc`). Callers pass monitor-local physical pixels and publish
//! only when their plates change (and every frame while something moves); nothing at rest.
use std::cell::RefCell;
use std::collections::BTreeMap;

use mypaper_desktop::Monitor;
use mypaper_glass_ipc as ipc;
use mypaper_ui::{Rect, Rgba};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub clarity: f32,
    pub blur: f32,
    pub refraction: f32,
    pub highlight: f32,
    pub tint: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    /// Stable per surface (widgets use their HWND). 0 lets the merger assign one.
    pub id: u64,
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
}

/// Who owns a set of plates; each source replaces only its own per monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Bars,
    Widgets,
}

#[derive(Default)]
struct Merger {
    sets: BTreeMap<(Source, String), Vec<ipc::Plate>>,
    writer: Option<ipc::Writer>,
    failed: bool,
}

thread_local! {
    static MERGER: RefCell<Merger> = RefCell::default();
}

/// Replace `source`'s plates on `monitor` (including `[]` when its surfaces disappear) and
/// republish the merged snapshot.
pub fn publish(source: Source, monitor: &Monitor, plates: &[Plate]) {
    let (ox, oy) = (monitor.rect.x as f32, monitor.rect.y as f32);
    let tag = (source as u64 + 1) << 56;
    let converted = plates
        .iter()
        .enumerate()
        .map(|(i, p)| ipc::Plate {
            id: if p.id != 0 { p.id } else { tag | i as u64 },
            rect: [
                p.rect.left + ox,
                p.rect.top + oy,
                p.rect.right - p.rect.left,
                p.rect.bottom - p.rect.top,
            ],
            radius: p.radius,
            material: ipc::Material {
                clarity: p.material.clarity,
                blur: p.material.blur,
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
                    log::error!("glass transport unavailable, surfaces fall back to tint: {e:#}");
                }
            }
        }
        let Some(writer) = &mut self.writer else {
            return;
        };
        let all: Vec<ipc::Plate> = self.sets.values().flatten().copied().collect();
        if all.len() > ipc::MAX_PLATES {
            log::warn!(
                "{} plates; the engine draws the first {}",
                all.len(),
                ipc::MAX_PLATES
            );
        }
        let n = all.len().min(ipc::MAX_PLATES);
        if let Err(e) = writer.publish(&all[..n]) {
            log::error!("glass publish: {e:#}");
        }
    }
}
