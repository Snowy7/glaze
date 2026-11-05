//! One native bar and its reservation, with a cropped software surface and shell-owned motion.
use super::{
    look::Tokens,
    motion::{self, Value},
    services::Model,
    view::{self, Frame, Images},
};
use crate::glass::Plate;
use anyhow::Result;
use mypaper_bars::{
    BarsConfig,
    actions::{BarKind, bar_topics},
    appbar::{self, Edge},
    apps::Item,
    config::{DockPosition, DockStyle, TopPosition},
    events::Reservation,
};
use mypaper_desktop::{Monitor, Rect as DesktopRect};
use mypaper_ui::{Glass, Kind, Point, Rect, Role, Surface, SurfaceId, Ui};
use std::{
    cell::Cell,
    collections::HashMap,
    time::{Duration, Instant},
};

thread_local! { static REPAINTS: Cell<u64> = const { Cell::new(0) }; }

pub(super) fn total_repaints() -> u64 {
    REPAINTS.with(Cell::get)
}

pub struct Bar {
    pub surface: Surface,
    pub kind: BarKind,
    pub config: BarsConfig,
    pub monitor: Monitor,
    pub tokens: Tokens,
    pub frame: Frame,
    pub items: Vec<Item>,
    pub hover: Option<u32>,
    /// The item under a held left button (drawn pressed while the pointer stays on it).
    pub pressed: Option<u32>,
    pub focus: Option<u32>,
    /// Extra islands, each a separate rounded overlay client.
    islands: Vec<Surface>,
    offsets: Vec<Point>,
    windows: Vec<Rect>,
    presentation_dirty: bool,
    pub hidden: bool,
    pub dirty: bool,
    redraws: u64,
    pub hide_at: Option<Instant>,
    pub pointer: Option<Point>,
    pub reservation: Option<Reservation>,
    pub band: DesktopRect,
    pub edge_surface: Option<Surface>,
    slide: Value,
    zoom: Vec<(Value, Value)>,
    pub animate: bool,
    /// Explorer has accepted this bar's current work-area reservation.
    pub reserved: bool,
    magnify_animate: bool,
    magnify_settings: motion::Settings,
    animations: HashMap<String, (Instant, bool)>,
}

pub fn edge(kind: BarKind, config: &BarsConfig) -> Edge {
    match kind {
        BarKind::Top => {
            if config.top.position == TopPosition::Top {
                Edge::Top
            } else {
                Edge::Bottom
            }
        }
        BarKind::Dock => match config.dock.position {
            DockPosition::Top => Edge::Top,
            DockPosition::Bottom => Edge::Bottom,
            DockPosition::Left => Edge::Left,
            DockPosition::Right => Edge::Right,
        },
    }
}

impl Bar {
    pub fn new(
        kind: BarKind,
        config: BarsConfig,
        monitor: Monitor,
        tokens: Tokens,
        animate: bool,
    ) -> Result<Self> {
        let surface = Surface::new_deferred_accessibility(
            Kind::Bar,
            if kind == BarKind::Top {
                "mypaper top bar"
            } else {
                "mypaper dock"
            },
            Role::Toolbar,
        )?;
        let autohide = kind == BarKind::Dock && config.dock.autohide;
        let reservation = None;
        let edge_surface = autohide
            .then(|| Surface::new_deferred_accessibility(Kind::Bar, "Reveal dock", Role::Toolbar))
            .transpose()?;
        let band = monitor.rect;
        let motion = if kind == BarKind::Dock {
            &config.dock.animation
        } else {
            &config.top.animation
        };
        let slide_settings = motion::settings(motion, kind == BarKind::Dock, false, animate);
        let magnify_settings = motion::settings(motion, kind == BarKind::Dock, true, animate);
        let mut slide = Value::new(if autohide { 1.0 } else { 0.0 });
        slide.settings = slide_settings.clone();
        let mut bar = Self {
            surface,
            kind,
            config,
            monitor,
            tokens,
            frame: Frame {
                list: Default::default(),
                targets: vec![],
                shapes: vec![],
                size: (1, 1),
            },
            items: vec![],
            hover: None,
            pressed: None,
            focus: None,
            islands: vec![],
            offsets: vec![],
            windows: vec![],
            presentation_dirty: true,
            hidden: false,
            dirty: true,
            redraws: 0,
            hide_at: None,
            pointer: None,
            reservation,
            band,
            edge_surface,
            slide,
            zoom: vec![],
            animate: slide_settings.enabled,
            reserved: autohide,
            magnify_animate: magnify_settings.enabled,
            magnify_settings,
            animations: HashMap::new(),
        };
        let thickness = ((if kind == BarKind::Top {
            bar.config.top.band()
        } else {
            bar.config.dock.band()
        }) as f32
            * bar.scale())
        .round() as i32;
        bar.band = appbar::settle(
            if autohide {
                bar.monitor.work
            } else {
                bar.monitor.rect
            },
            edge(kind, &bar.config),
            thickness,
        );
        Ok(bar)
    }

    pub fn scale(&self) -> f32 {
        self.monitor.dpi as f32 / 96.0
    }
    pub fn owns(&self, id: SurfaceId) -> bool {
        self.surface.id() == id || self.islands.iter().any(|s| s.id() == id)
    }
    pub fn input_offset(&self, id: SurfaceId) -> Point {
        let index = if self.surface.id() == id {
            0
        } else {
            self.islands
                .iter()
                .position(|s| s.id() == id)
                .map_or(0, |i| i + 1)
        };
        self.offsets.get(index).copied().unwrap_or_default()
    }
    pub fn windows(&mut self, windows: &[Rect]) {
        if self.windows != windows {
            self.windows = windows.to_vec();
            self.presentation_dirty = true;
        }
    }
    fn covered(&self, body: Rect) -> bool {
        let origin = self.origin();
        let body = body.offset(origin.0 as f32, origin.1 as f32);
        self.windows.iter().any(|w| w.intersects(&body))
    }
    /// The plates' material in physical px (`tokens.material` is in DIPs; none when transparent).
    fn material(&self) -> Option<crate::glass::Material> {
        self.tokens.material.map(|m| crate::glass::Material {
            blur: m.blur * self.scale(),
            ..m
        })
    }

    /// What decides this bar's window and work-area reservation: its monitor, edge, band and
    /// auto-hide. A config change that keeps it updates the bar in place ([`Bar::restyle`]);
    /// only a change of it builds a new bar and reserves again.
    pub fn geometry(kind: BarKind, config: &BarsConfig, monitor: &Monitor) -> impl PartialEq + use<> {
        let band = if kind == BarKind::Top {
            config.top.band()
        } else {
            config.dock.band()
        };
        (
            kind,
            monitor.device.clone(),
            monitor.rect,
            monitor.dpi,
            edge(kind, config),
            band,
            kind == BarKind::Dock && config.dock.autohide,
        )
    }

    /// Takes a new config and look that keep [`Bar::geometry`]: the same windows and
    /// reservation, repainted with the new content and material (the engine fades a plate's
    /// material change, since its id, the window, stays the same).
    pub fn restyle(&mut self, config: BarsConfig, tokens: Tokens, animate: bool) {
        let motion = if self.kind == BarKind::Dock {
            &config.dock.animation
        } else {
            &config.top.animation
        };
        let dock = self.kind == BarKind::Dock;
        let slide = motion::settings(motion, dock, false, animate);
        self.magnify_settings = motion::settings(motion, dock, true, animate);
        self.magnify_animate = self.magnify_settings.enabled;
        self.animate = slide.enabled;
        self.slide.settings = slide;
        self.config = config;
        self.tokens = tokens;
        self.dirty = true;
        self.presentation_dirty = true;
    }
    pub fn horizontal(&self) -> bool {
        matches!(edge(self.kind, &self.config), Edge::Top | Edge::Bottom)
    }
    pub fn shows_content(&self) -> bool {
        !self.hidden && self.slide.value < 0.999
    }
    pub fn topics(&self) -> std::collections::BTreeSet<mypaper_bars::actions::Topic> {
        bar_topics(self.kind, &self.config)
    }
    pub fn reserve(&mut self) {
        if self.reservation.is_none() && !(self.kind == BarKind::Dock && self.config.dock.autohide)
        {
            self.reservation = Reservation::new(edge(self.kind, &self.config))
                .inspect_err(|e| log::error!("app-bar reservation: {e:#}"))
                .ok();
        }
        let thickness = ((if self.kind == BarKind::Top {
            self.config.top.band()
        } else {
            self.config.dock.band()
        }) as f32
            * self.scale())
        .round() as i32;
        let granted = self
            .reservation
            .as_mut()
            .and_then(|r| r.try_reserve(self.monitor.rect, thickness));
        self.reserved =
            granted.is_some() || (self.kind == BarKind::Dock && self.config.dock.autohide);
        self.band = if let Some(band) = granted {
            band
        } else {
            appbar::settle(self.monitor.work, edge(self.kind, &self.config), thickness)
        };
        self.dirty = true;
    }
    pub fn reveal(&mut self, now: Instant) {
        self.slide.target(0.0, self.animate, now);
        self.hide_at = Some(now + Duration::from_secs(1));
        self.dirty = true;
    }
    pub fn leave(&mut self, now: Instant) {
        self.pointer = None;
        self.hover = None;
        self.pressed = None;
        self.dirty = true;
        if self.config.dock.autohide && self.kind == BarKind::Dock {
            self.hide_at = Some(now + Duration::from_secs(1));
        }
        self.zoom_to(None, now);
    }
    pub fn zoom_to(&mut self, pointer: Option<Point>, now: Instant) {
        if self.kind != BarKind::Dock {
            return;
        }
        let enabled = self.config.dock.style == DockStyle::Floating
            && self.config.dock.magnify.enabled
            && self.magnify_animate;
        let scale = self.scale();
        let size = self.config.dock.size() as f32 * scale;
        // Base centers are anchored in screen-independent envelope coordinates.
        let centers: Vec<_> = self
            .frame
            .targets
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t, view::Target::Dock(_)))
            .filter_map(|(i, _)| self.frame.list.hits.iter().find(|h| h.id == i as u32 + 1))
            .enumerate()
            .map(|(i, h)| {
                (if self.horizontal() {
                    (h.rect.left + h.rect.right) / 2.0
                } else {
                    (h.rect.top + h.rect.bottom) / 2.0
                }) - self.zoom.get(i).map_or(0.0, |v| v.1.value)
            })
            .collect();
        let coordinates = pointer
            .filter(|_| enabled)
            .map(|p| if self.horizontal() { p.x } else { p.y });
        let target = coordinates.map(|p| {
            motion::magnify(
                &centers,
                p,
                size,
                self.config.dock.magnify.scale as f32,
                self.config.dock.magnify.range as f32 * size,
            )
        });
        self.zoom
            .resize_with(self.items.len(), || (Value::new(1.0), Value::new(0.0)));
        for (i, (factor, shift)) in self.zoom.iter_mut().enumerate() {
            let (s, x) = target
                .as_ref()
                .and_then(|t| t.get(i))
                .copied()
                .unwrap_or((1.0, 0.0));
            factor.settings = self.magnify_settings.clone();
            shift.settings = self.magnify_settings.clone();
            factor.target(s, self.magnify_animate, now);
            shift.target(x, self.magnify_animate, now);
        }
    }

    pub fn animate_item(&mut self, key: String, launch: bool, now: Instant) {
        if self.animate
            && (if launch {
                self.config.dock.bounce
            } else {
                self.config.dock.attention != mypaper_bars::config::AttentionStyle::None
            })
        {
            self.animations.insert(key, (now, launch));
            self.dirty = true;
        }
    }
    pub fn paint(
        &mut self,
        ui: &mut Ui,
        model: &Model,
        images: &mut Images,
        now: Instant,
        keep_open: bool,
    ) -> Result<bool> {
        if self.hidden {
            self.surface.hide();
            for island in &self.islands {
                island.hide();
            }
            if let Some(edge) = &self.edge_surface {
                edge.hide();
            }
            return Ok(false);
        }
        let first_paint = self.redraws == 0;
        if self.hide_at.is_some_and(|t| t <= now) {
            self.hide_at = if keep_open {
                Some(now + Duration::from_secs(1))
            } else {
                None
            };
            if self.pointer.is_none() && !keep_open {
                self.slide.target(1.0, self.animate, now);
            }
        }
        let previous_slide = self.slide.value;
        let mut moving = self.slide.step(now);
        let mut zoom_changed = false;
        for (scale, shift) in &mut self.zoom {
            let before = (scale.value, shift.value);
            moving |= scale.step(now) | shift.step(now);
            zoom_changed |= before != (scale.value, shift.value);
        }
        let had_animation = !self.animations.is_empty();
        let attention = self.config.dock.attention;
        self.animations.retain(|key, (start, launch)| {
            now.duration_since(*start).as_secs_f32()
                < if *launch {
                    4.0
                } else if attention == mypaper_bars::config::AttentionStyle::Pulse {
                    2.4
                } else {
                    1.8
                }
                && (!*launch
                    || self
                        .items
                        .iter()
                        .find(|i| i.key == *key)
                        .is_none_or(|i| i.windows.is_empty()))
        });
        let hops: Vec<_> = self
            .items
            .iter()
            .map(|i| {
                self.animations.get(&i.key).map_or(0.0, |(start, launch)| {
                    let t = now.duration_since(*start).as_secs_f32();
                    if !*launch && attention == mypaper_bars::config::AttentionStyle::Pulse {
                        -(0.7 + 0.3 * (t * std::f32::consts::TAU / 0.4).cos())
                    } else {
                        (t * std::f32::consts::PI / 0.6).sin().abs() * 8.0 * self.scale()
                    }
                })
            })
            .collect();
        moving |= !self.animations.is_empty();
        let rebuilt = self.dirty || zoom_changed || had_animation;
        if rebuilt {
            let scale = self.scale();
            let hot = mypaper_ui::Interaction {
                hover: self.hover,
                pressed: self.pressed,
                focus: self.focus,
                focus_visible: self.focus.is_some(),
            };
            self.frame = if self.kind == BarKind::Top {
                view::top(
                    self.band.width as u32,
                    &self.config,
                    model,
                    &self.tokens,
                    scale,
                    ui,
                    images,
                    Some(&hot),
                )
            } else {
                let zoom: Vec<_> = self.zoom.iter().map(|(s, x)| (s.value, x.value)).collect();
                view::dock(
                    &self.config,
                    model,
                    &self.items,
                    &self.tokens,
                    scale,
                    (self.band.width as u32, self.band.height as u32),
                    &zoom,
                    &hops,
                    ui,
                    images,
                    Some(&hot),
                )
            };
            self.focus = self
                .focus
                .filter(|id| self.frame.list.hits.iter().any(|h| h.id == *id));
            // The view drew the kit's focus ring for it (keyboard focus only).
            self.frame.list.focus = self.focus;
            self.dirty = false;
        }
        if rebuilt || previous_slide != self.slide.value || self.presentation_dirty {
            self.present(ui)?;
            self.presentation_dirty = false;
        }
        if self.slide.value >= 0.999 && self.config.dock.autohide && self.kind == BarKind::Dock {
            if self.surface.is_visible() {
                self.surface.hide();
            }
        } else if !self.surface.is_visible() {
            self.surface.show();
        }
        if first_paint && self.surface.is_visible() {
            mypaper_desktop::startup::ready("shell", "first_paint_ms");
        }
        if let Some(edge) = &mut self.edge_surface {
            let e = appbar::settle(
                self.monitor.work,
                super::bar::edge(self.kind, &self.config),
                1,
            );
            let list = mypaper_ui::DisplayList {
                ops: vec![mypaper_ui::Op::Fill {
                    rect: Rect::new(0.0, 0.0, e.width as f32, e.height as f32),
                    radius: 0.0,
                    color: mypaper_ui::Rgba::HIT,
                }],
                ..Default::default()
            };
            edge.present(ui, (e.x, e.y), (e.width as u32, e.height as u32), list)?;
            if !edge.is_visible() {
                edge.show();
            }
        }
        Ok(moving)
    }

    /// One HWND per island; dock keeps its envelope so magnified icons stay clickable.
    fn present(&mut self, ui: &mut Ui) -> Result<()> {
        let count = if self.kind == BarKind::Top {
            self.frame.shapes.len().max(1)
        } else {
            1
        };
        while self.islands.len() + 1 < count {
            self.islands.push(Surface::new_deferred_accessibility(
                Kind::Bar,
                "mypaper bar island",
                Role::Toolbar,
            )?);
        }
        self.islands.truncate(count - 1);
        self.offsets.clear();
        let origin = self.origin();
        let visible = self.shows_content();
        for i in 0..count {
            let (body, radius) = self.frame.shapes.get(i).copied().unwrap_or((
                Rect::new(0.0, 0.0, self.frame.size.0 as f32, self.frame.size.1 as f32),
                0.0,
            ));
            let crop = if self.kind == BarKind::Top {
                body.inflate(1.0)
            } else {
                Rect::new(0.0, 0.0, self.frame.size.0 as f32, self.frame.size.1 as f32)
            };
            let (list, offset, size) = if self.kind == BarKind::Top {
                self.frame.crop(crop)
            } else {
                (self.frame.list.clone(), Point::default(), self.frame.size)
            };
            self.offsets.push(offset);
            let glass = self
                .material()
                .filter(|_| self.covered(body))
                .map(|material| Glass {
                    body: body.offset(-offset.x, -offset.y),
                    radius,
                    material,
                });
            let surface = if i == 0 {
                &mut self.surface
            } else {
                &mut self.islands[i - 1]
            };
            surface.set_glass(glass.filter(|_| visible));
            surface.set_click_activates(
                self.frame
                    .list
                    .hits
                    .iter()
                    .filter(|h| {
                        self.frame
                            .targets
                            .get(h.id as usize - 1)
                            .is_some_and(view::claims_foreground)
                    })
                    .map(|h| h.rect.offset(-offset.x, -offset.y))
                    .collect(),
            );
            if surface.present_sparse(
                ui,
                (origin.0 + offset.x as i32, origin.1 + offset.y as i32),
                size,
                list,
            )? {
                self.redraws += 1;
                REPAINTS.with(|count| count.set(count.get() + 1));
                log::debug!(
                    "bar {:?} {} redraw={} island={i}",
                    self.kind,
                    self.monitor.device,
                    self.redraws
                );
            }
            if visible {
                if !surface.is_visible() {
                    surface.show();
                }
            } else {
                surface.hide();
            }
        }
        Ok(())
    }

    pub fn origin(&self) -> (i32, i32) {
        let (w, h) = (self.frame.size.0 as i32, self.frame.size.1 as i32);
        let e = edge(self.kind, &self.config);
        let margin = if self.kind == BarKind::Dock {
            (self.config.dock.margin.unwrap_or(6.0) as f32 * self.scale()).round() as i32
        } else {
            0
        };
        let (mut x, mut y) = match e {
            Edge::Top => (
                self.band.x + (self.band.width - w) / 2,
                self.band.y + margin,
            ),
            Edge::Bottom => (
                self.band.x + (self.band.width - w) / 2,
                self.band.y + self.band.height - h - margin,
            ),
            Edge::Left => (
                self.band.x + margin,
                self.band.y + (self.band.height - h) / 2,
            ),
            Edge::Right => (
                self.band.x + self.band.width - w - margin,
                self.band.y + (self.band.height - h) / 2,
            ),
        };
        let slide = self.slide.value;
        match e {
            Edge::Top => y -= ((h + margin) as f32 * slide).round() as i32,
            Edge::Bottom => y += ((h + margin) as f32 * slide).round() as i32,
            Edge::Left => x -= ((w + margin) as f32 * slide).round() as i32,
            Edge::Right => x += ((w + margin) as f32 * slide).round() as i32,
        }
        (x, y)
    }

    pub fn plates(&self) -> Vec<Plate> {
        if self.hidden || self.slide.value >= 0.999 {
            return vec![];
        }
        let Some(material) = self.material() else {
            return vec![];
        };
        let (x, y) = self.origin();
        self.frame
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, (rect, _))| !self.covered(*rect) || !mypaper_ui::glass::available())
            .map(|(i, (rect, radius))| Plate {
                id: if i == 0 {
                    self.surface.hwnd().0 as u64
                } else {
                    self.islands[i - 1].hwnd().0 as u64
                },
                rect: rect.offset(
                    (x - self.monitor.rect.x) as f32,
                    (y - self.monitor.rect.y) as f32,
                ),
                radius: *radius,
                material,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_ui::GlassMaterial;
    use mypaper_ui::glass::{Backdrop, Shape};
    use windows::Win32::Foundation::HWND;
    struct Available;
    impl Backdrop for Available {
        fn available(&self) -> bool {
            true
        }
        fn open(&self, _: HWND, _: Shape, _: f32, _: &GlassMaterial) -> bool {
            true
        }
        fn place(&self, _: HWND, _: Shape, _: f32) {}
        fn material(&self, _: HWND, _: &GlassMaterial) {}
        fn close(&self, _: HWND) {}
    }
    #[test]
    fn only_the_covered_island_loses_its_plate_and_uncover_restores_it() {
        mypaper_ui::glass::install(std::rc::Rc::new(Available));
        let config = BarsConfig::default();
        let tokens = Tokens::resolve(
            &config.top.look,
            &Default::default(),
            &mypaper_bars::accent::read(),
        );
        let monitor = Monitor {
            device: "test".into(),
            rect: DesktopRect {
                x: -1280,
                y: 0,
                width: 1280,
                height: 720,
            },
            work: DesktopRect {
                x: -1280,
                y: 32,
                width: 1280,
                height: 688,
            },
            dpi: 96,
            primary: true,
        };
        let mut bar = Bar::new(BarKind::Top, config, monitor, tokens, false).unwrap();
        bar.frame.size = (1280, 32);
        bar.frame.shapes = [0.0, 500.0, 1000.0]
            .map(|x| (Rect::new(x, 0.0, 100.0, 32.0), 10.0))
            .to_vec();
        for _ in 0..2 {
            bar.islands.push(
                Surface::new_deferred_accessibility(Kind::Bar, "Test island", Role::Toolbar)
                    .unwrap(),
            );
        }
        assert_eq!(bar.plates().len(), 3);
        bar.windows(&[Rect::new(-760.0, 0.0, 40.0, 200.0)]);
        let plates = bar.plates();
        assert_eq!(plates.len(), 2);
        assert_eq!(plates[0].rect.left, 0.0);
        assert_eq!(plates[1].rect.left, 1000.0);
        bar.windows(&[]);
        assert_eq!(bar.plates().len(), 3);
    }
}
