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
use mypaper_ui::{Kind, Point, Rect, Role, Surface, Ui};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub struct Bar {
    pub surface: Surface,
    pub kind: BarKind,
    pub config: BarsConfig,
    pub monitor: Monitor,
    pub tokens: Tokens,
    pub frame: Frame,
    pub items: Vec<Item>,
    pub hover: Option<u32>,
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
        let surface = Surface::new(
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
            .then(|| Surface::new(Kind::Bar, "Reveal dock", Role::Toolbar))
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
        self.band = if let Some(r) = &mut self.reservation {
            r.reserve(self.monitor.rect, thickness)
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
            if let Some(edge) = &self.edge_surface {
                edge.hide();
            }
            return Ok(false);
        }
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
        let mut moving = self.slide.step(now);
        let mut zoom_changed = false;
        for (scale, shift) in &mut self.zoom {
            let before = (scale.value, shift.value);
            moving |= scale.step(now) | shift.step(now);
            zoom_changed |= before != (scale.value, shift.value);
        }
        let had_animation = !self.animations.is_empty();
        self.animations.retain(|key, (start, launch)| {
            now.duration_since(*start) < Duration::from_secs(if *launch { 4 } else { 2 })
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
                    (t * std::f32::consts::PI * 3.0).sin().abs()
                        * 8.0
                        * self.scale()
                        * if *launch {
                            1.0
                        } else {
                            (1.0 - t / 2.0).max(0.0)
                        }
                })
            })
            .collect();
        moving |= !self.animations.is_empty();
        if self.dirty || zoom_changed || had_animation {
            let scale = self.scale();
            self.frame = if self.kind == BarKind::Top {
                view::top(
                    self.band.width as u32,
                    &self.config,
                    model,
                    &self.tokens,
                    scale,
                    ui,
                    images,
                    self.hover,
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
                    self.hover,
                )
            };
            let origin = self.origin();
            if self
                .surface
                .present(ui, origin, self.frame.size, self.frame.list.clone())?
            {
                self.redraws += 1;
                log::debug!(
                    "bar {:?} {} redraw={} size={:?}",
                    self.kind,
                    self.monitor.device,
                    self.redraws,
                    self.frame.size
                );
            }
            self.dirty = false;
        }
        self.surface.move_to(self.origin())?;
        if self.slide.value >= 0.999 && self.config.dock.autohide && self.kind == BarKind::Dock {
            if self.surface.is_visible() {
                self.surface.hide();
            }
        } else if !self.surface.is_visible() {
            self.surface.show();
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
        let Some(material) = self.tokens.material else {
            return vec![];
        };
        let (x, y) = self.origin();
        self.frame
            .shapes
            .iter()
            .map(|(rect, radius)| Plate {
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
