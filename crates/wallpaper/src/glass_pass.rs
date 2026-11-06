//! Liquid Glass plates in the wallpaper frame, drawn by liquid-rust (`crates/liquid-rust`, a
//! vendored copy of al1h3n/liquid-rust, MIT OR Apache-2.0) from the shell's latest
//! shared-memory snapshot. See `docs/glass.md`.
//!
//! Each screen keeps one liquid-rust `Scene` (its plates, in points). Each kept frame glass is
//! drawn from has one `Renderer`, whose blur pyramid of it is reused while the wallpaper holds
//! still; screens showing one kept frame share it. Plates sit
//! exactly where they were published (`Scene::place`): the shell owns their motion. Liquid-rust's
//! springs materialize glass in and out and fade material changes; while one runs, the screen
//! keeps redrawing ([`GlassPass::take_dirty`]).

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use liquid_rust as lr;
use mypaper_desktop::{Rect, monitor_infos};
use mypaper_glass_ipc::{Plate, Reader, Style};
use windows::Win32::Foundation::HANDLE;

use crate::gpu::{Gpu, KeptFrame};

/// How a published material change fades in.
const MATERIAL_FADE: lr::Spring = lr::Spring::SMOOTH;

/// One screen's glass.
struct Screen {
    monitor: Rect,
    /// Physical px per point (the monitor's DPI / 96).
    scale: f32,
    scene: lr::Scene,
    ids: HashMap<u64, lr::GlassId>,
    /// This screen's plates and their styles as last applied.
    plates: Vec<(Plate, Style)>,
    /// The kept frame its glass was last drawn from (`None` while it has no glass).
    source: Option<wgpu::Texture>,
    dirty: bool,
    animating: bool,
    updated: Option<Instant>,
}

/// One renderer per kept frame that glass is drawn from: screens sharing a kept frame (one
/// wallpaper pass for both) share its blur pyramid too, which then holds every screen's part.
struct Shared {
    texture: wgpu::Texture,
    /// Its sRGB view: liquid-rust reads linear colour.
    view: wgpu::TextureView,
    renderer: lr::Renderer,
    /// The kept frame's `drawn` when its pyramid was last built: the wallpaper it blurs.
    drawn: Option<u32>,
}

pub struct GlassPass {
    screens: Vec<Screen>,
    shared: Vec<Shared>,
    reader: Reader,
    /// Compiled once; each kept frame's renderer forks it.
    template: Option<lr::Renderer>,
    pub sequence: u64,
    pub published_qpc: u64,
}

fn overlaps(p: &Plate, m: &Rect) -> bool {
    let [x, y, w, h] = p.rect;
    x < (m.x + m.width) as f32
        && x + w > m.x as f32
        && y < (m.y + m.height) as f32
        && y + h > m.y as f32
}

impl Screen {
    fn new(monitor: Rect, scale: f32) -> Self {
        Self {
            monitor,
            scale,
            scene: lr::Scene::new(),
            ids: HashMap::new(),
            plates: Vec::new(),
            source: None,
            dirty: false,
            animating: false,
            updated: None,
        }
    }

    /// Syncs the scene to this screen's plates: new ones materialize, gone ones dematerialize,
    /// the rest move instantly and fade to a new material. Materials and shapes are resolved
    /// here, only when something changed, never per frame.
    fn apply(&mut self, plates: Vec<(Plate, Style)>) {
        if plates == self.plates {
            return;
        }
        let s = self.scale;
        let (ox, oy) = (self.monitor.x as f32, self.monitor.y as f32);
        for (p, style) in &plates {
            let [x, y, w, h] = p.rect;
            let frame = lr::Rect::new((x - ox) / s, (y - oy) / s, w / s, h / s);
            // Exactly the surface's own corners: quarter circles unless the style smooths them.
            let shape = crate::material::shape(p.radius / s, style.smoothing);
            let (material, appearance) = crate::material::liquid(&p.material, s);
            let mut material = crate::material::styled(
                material,
                style.brightness,
                style.saturation,
                style.sheen,
                style.depth,
            );
            material.inner_shadow = style.inner_shadow;
            match self.ids.get(&p.id) {
                Some(&id) => {
                    self.scene.place(id, frame);
                    self.scene.set_shape(id, shape);
                    self.scene.set_material_with(id, material, MATERIAL_FADE);
                    self.scene.set_appearance(id, Some(appearance));
                }
                None => {
                    let glass = lr::Glass::new(frame).shape(shape).material(material).appearance(appearance);
                    self.ids.insert(p.id, self.scene.add(glass));
                }
            }
        }
        self.ids.retain(|plate, id| {
            let kept = plates.iter().any(|(p, _)| p.id == *plate);
            if !kept {
                self.scene.remove(*id);
            }
            kept
        });
        self.plates = plates;
        self.dirty = true;
    }
}

impl GlassPass {
    pub fn new(monitors: Option<&[Rect]>) -> Result<Self> {
        let infos = monitor_infos();
        let screens = monitors
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let dpi = infos.iter().find(|i| i.rect == *m).map_or(96, |i| i.dpi);
                Screen::new(*m, dpi as f32 / 96.0)
            })
            .collect();
        Ok(Self {
            screens,
            shared: Vec::new(),
            reader: Reader::open()?,
            template: None,
            sequence: 0,
            published_qpc: 0,
        })
    }

    pub fn wait_handles(&self) -> Vec<HANDLE> {
        std::iter::once(self.reader.wake_event())
            .chain(self.reader.writer_process())
            .collect()
    }

    pub fn reload(&mut self) {
        match self.reader.read_changed() {
            Ok(Some(snapshot)) => {
                self.sequence = snapshot.sequence;
                self.published_qpc = snapshot.published_qpc;
                let styles = snapshot.styles.into_iter().chain(std::iter::repeat(Style::default()));
                self.set_styled(snapshot.plates.into_iter().zip(styles).collect());
            }
            Ok(None) => {}
            Err(e) => log::error!("glass IPC: {e:#}"),
        }
    }

    /// Plates at the default style (the frame lab's fixtures).
    #[cfg(test)]
    pub(crate) fn set_plates(&mut self, plates: Vec<Plate>) {
        self.set_styled(plates.into_iter().map(|p| (p, Style::default())).collect());
    }

    pub(crate) fn set_styled(&mut self, plates: Vec<(Plate, Style)>) {
        for screen in &mut self.screens {
            // Only this monitor's plates: identical publications cause zero drawing.
            let mine = plates.iter().filter(|(p, _)| overlaps(p, &screen.monitor)).copied().collect();
            screen.apply(mine);
        }
    }

    /// Whether screen `index` must redraw its glass: its plates changed, or a spring runs.
    pub fn take_dirty(&mut self, index: usize) -> bool {
        self.screens.get_mut(index).is_some_and(|s| std::mem::take(&mut s.dirty) || s.animating)
    }

    /// A renderer of its own (buffers, blur pyramid) on the pipelines every plate's renderer
    /// shares, compiled on first use: Studio's previews draw with one. Compiling them again
    /// took 3-4 ms of every preview answer; a fork takes 0.02 ms (RTX, 2026-11-06).
    pub fn fork(&mut self, gpu: &Gpu) -> lr::Renderer {
        let srgb = gpu.format.add_srgb_suffix();
        self.template
            .get_or_insert_with(|| lr::Renderer::new(&gpu.device, srgb))
            .fork(&gpu.device)
    }

    /// Whether a spring runs on screen `index` (the engine then keeps drawing it). Springs only
    /// advance as the screen is drawn, so a covered screen's spring waits, mid-flight, for it.
    pub fn animating(&self, index: usize) -> bool {
        self.screens.get(index).is_some_and(|s| s.animating)
    }

    /// Draws screen `index`'s glass into the swapchain image `target` (which already holds the
    /// restored wallpaper), refracting the kept wallpaper `kept`. It can be another size than
    /// `target`: liquid-rust samples it by uv. The blur pyramid is rebuilt where this screen's
    /// glass samples once the kept frame holds a wallpaper it hasn't blurred (`kept.drawn`), so a
    /// second screen showing the same frame only adds its own part.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        index: usize,
        kept: &KeptFrame,
        target: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let behind = &kept.texture;
        let Some(screen) = self.screens.get_mut(index) else {
            return false;
        };
        let now = Instant::now();
        let dt = screen.updated.map_or(0.0, |t| (now - t).as_secs_f32());
        screen.animating = screen.scene.update(dt);
        screen.updated = screen.animating.then_some(now);
        let drawn = !screen.ids.is_empty() || screen.animating;
        // Nothing left, not even glass fading out: this screen no longer holds a pyramid.
        screen.source = drawn.then(|| behind.clone());
        let screens = &self.screens;
        self.shared.retain(|s| {
            screens
                .iter()
                .any(|screen| screen.source.as_ref() == Some(&s.texture))
        });
        if !drawn {
            return false;
        }
        let srgb = gpu.format.add_srgb_suffix();
        let template = self.template.get_or_insert_with(|| lr::Renderer::new(&gpu.device, srgb));
        let shared = match self.shared.iter().position(|s| s.texture == *behind) {
            Some(i) => &mut self.shared[i],
            None => {
                let view = behind.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(srgb),
                    ..Default::default()
                });
                self.shared.push(Shared {
                    texture: behind.clone(),
                    view,
                    renderer: template.fork(&gpu.device),
                    drawn: None,
                });
                self.shared.last_mut().expect("just pushed")
            }
        };
        // A wallpaper this pyramid hasn't blurred yet. A screen sharing the kept frame (drawn
        // after another in the same wallpaper frame, or newly showing it) finds it blurred, and
        // liquid-rust adds only the texels its own glass reads.
        let content_changed = shared.drawn != kept.drawn.get();
        shared.drawn = kept.drawn.get();
        let screen = &self.screens[index];
        let target_view = target.create_view(&wgpu::TextureViewDescriptor { format: Some(srgb), ..Default::default() });
        shared.renderer.render(
            &gpu.device,
            &gpu.queue,
            encoder,
            &lr::Frame {
                content: &shared.view,
                target: &target_view,
                size: (target.width(), target.height()),
                scale: screen.scale,
                content_changed,
                copy_content: false,
            },
            &screen.scene,
        );
        true
    }

    /// Lands every spring now (the lab's fixed frames show settled glass).
    #[cfg(test)]
    pub fn settle(&mut self) {
        for screen in &mut self.screens {
            screen.scene.physics = false;
            screen.scene.update(0.0);
            screen.scene.physics = true;
        }
    }

    /// The renderer screen `index` last drew with (the frame lab profiles its passes).
    #[cfg(test)]
    pub fn renderer(&mut self, index: usize) -> Option<&mut lr::Renderer> {
        let source = self.screens.get(index)?.source.clone()?;
        self.shared.iter_mut().find(|s| s.texture == source).map(|s| &mut s.renderer)
    }

    /// Releases old kept frames on theme switches, even while a screen stays hidden.
    pub fn reset(&mut self) {
        self.shared.clear();
        for screen in &mut self.screens {
            screen.source = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(monitors: &[Rect]) -> GlassPass {
        GlassPass {
            screens: monitors.iter().map(|m| Screen::new(*m, 1.5)).collect(),
            shared: Vec::new(),
            reader: Reader::open().unwrap(),
            template: None,
            sequence: 0,
            published_qpc: 0,
        }
    }

    #[test]
    fn only_affected_screens_redraw_and_identical_publications_do_nothing() {
        let monitors = [
            Rect {
                x: -2560,
                y: 9,
                width: 2560,
                height: 1440,
            },
            Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ];
        let mut pass = pass(&monitors);
        let mut p = Plate {
            id: 7,
            rect: [-2544.25, 77.0, 309.0, 307.0],
            radius: 22.0,
            material: mypaper_glass_ipc::Material::default(),
        };
        pass.set_plates(vec![p]);
        assert!(pass.take_dirty(0));
        assert!(!pass.take_dirty(1));
        let id = pass.screens[0].ids[&7];
        let frame = pass.screens[0].scene.glass(id).unwrap().frame;
        assert_eq!((frame.x, frame.y, frame.width), ((-2544.25 + 2560.0) / 1.5, (77.0 - 9.0) / 1.5, 309.0 / 1.5), "points, monitor-local");
        pass.screens[0].scene.physics = false;
        pass.screens[0].scene.update(0.0);
        pass.set_plates(vec![p]);
        assert!(!pass.take_dirty(0), "nothing changed");
        p.rect[0] = 100.0;
        pass.set_plates(vec![p]);
        assert!(pass.take_dirty(0), "left screen 0");
        assert!(pass.take_dirty(1), "arrived on screen 1");
        assert!(pass.screens[0].ids.is_empty());
    }
}
