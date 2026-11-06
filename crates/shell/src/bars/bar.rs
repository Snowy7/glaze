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
use mypaper_ui::reveal::{Reveal, Snapshot, Transform};
use mypaper_ui::{Glass, Kind, Point, Rect, Role, Surface, SurfaceId, Ui};
use std::{
    cell::Cell,
    collections::HashMap,
    time::{Duration, Instant},
};

thread_local! { static REPAINTS: Cell<u64> = const { Cell::new(0) }; }

/// Logs (debug) the dock's mean rebuild and present time every 240
/// rebuilt frames (a magnification sweep's split between the view and the renderer).
fn dock_timing(build: Duration, present: Duration) {
    thread_local!(static SUM: Cell<(u32, Duration, Duration)> = const { Cell::new((0, Duration::ZERO, Duration::ZERO)) });
    SUM.with(|sum| {
        let (n, b, p) = sum.get();
        let (n, b, p) = (n + 1, b + build, p + present);
        if n == 240 {
            log::debug!(
                "dock timing: build {:?} present {:?} per frame",
                b / n,
                p / n
            );
            sum.set((0, Duration::ZERO, Duration::ZERO));
        } else {
            sum.set((n, b, p));
        }
    });
}

pub(super) fn total_repaints() -> u64 {
    REPAINTS.with(Cell::get)
}

pub struct Bar {
    studio_active: bool,
    studio_base: Option<BarsConfig>,
    studio_band: Option<DesktopRect>,
    studio_reveal: Option<(Value, Option<Instant>)>,
    /// A preview may show a normally disabled bar; it never owns a reservation.
    pub(super) studio_only: bool,
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
    pub maximized: super::maximized::Debounce,
    pub pointer: Option<Point>,
    pub reservation: Option<Reservation>,
    pub band: DesktopRect,
    pub edge_surface: Option<Surface>,
    slide: Value,
    presentation: Reveal,
    reveal_snapshots: Vec<Option<Snapshot>>,
    presentation_transforms: Vec<Transform>,
    /// The dock's magnification, as macOS's: how far in it is (one spring, 0 to 1, on enter and
    /// leave) and the pointer's place along the dock; each slot's (scale, shift) follows the
    /// pointer directly, so icons never trail it.
    presence: Value,
    along: Option<f32>,
    zoom: Vec<(f32, f32)>,
    zoom_due: bool,
    zoomed_at: Instant,
    /// Most of one display refresh, and at least 8 ms (at most 125 frames a second: a
    /// magnification frame costs ~0.8 ms of software rendering, and past 120 Hz that buys no
    /// visible smoothness): the zoom is laid out at most this often.
    frame_interval: Duration,
    pub animate: bool,
    /// Explorer has accepted this bar's current work-area reservation.
    pub reserved: bool,
    magnify_animate: bool,
    magnify_settings: motion::Settings,
    bounce_settings: motion::Settings,
    attention_settings: motion::Settings,
    hover_settings: motion::Settings,
    press_settings: motion::Settings,
    interactions: HashMap<u32, (Value, Value)>,
    animations: HashMap<String, (Instant, bool)>,
    /// The floating dock's sprites (DWM places and scales its icons); none for other bars and
    /// styles, or when DirectComposition is unavailable.
    sprites: Option<super::sprites::Sprites>,
    /// DirectComposition failed once: draw everything in the window from then on.
    sprites_failed: bool,
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
        let performance = config.performance.active(mypaper_desktop::on_battery());
        let dock = kind == BarKind::Dock;
        let slide_settings =
            motion::resolve(motion, dock, motion::Leaf::Show, animate, performance);
        let magnify_settings =
            motion::resolve(motion, dock, motion::Leaf::Magnify, animate, performance);
        let bounce_settings =
            motion::resolve(motion, dock, motion::Leaf::Bounce, animate, performance);
        let attention_settings =
            motion::resolve(motion, dock, motion::Leaf::Attention, animate, performance);
        let hover_settings =
            motion::resolve(motion, dock, motion::Leaf::Hover, animate, performance);
        let press_settings =
            motion::resolve(motion, dock, motion::Leaf::Press, animate, performance);
        let presentation = motion::reveal(motion::presentation(motion, dock, motion::Leaf::Show));
        let mut slide = Value::new(if autohide { 1.0 } else { 0.0 });
        slide.settings = slide_settings;
        let mut bar = Self {
            studio_active: false,
            studio_base: None,
            studio_band: None,
            studio_reveal: None,
            studio_only: false,
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
                rest: vec![],
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
            maximized: Default::default(),
            pointer: None,
            reservation,
            band,
            edge_surface,
            slide,
            presentation,
            reveal_snapshots: Vec::new(),
            presentation_transforms: Vec::new(),
            presence: {
                let mut v = Value::new(0.0);
                v.settings = magnify_settings;
                v
            },
            along: None,
            zoom: vec![],
            zoom_due: false,
            zoomed_at: Instant::now(),
            frame_interval: mypaper_ui::display_frame()
                .mul_f32(0.85)
                .max(Duration::from_millis(8)),
            animate: slide_settings.enabled,
            reserved: autohide,
            magnify_animate: magnify_settings.enabled,
            magnify_settings,
            bounce_settings,
            attention_settings,
            hover_settings,
            press_settings,
            interactions: HashMap::new(),
            animations: HashMap::new(),
            sprites: None,
            sprites_failed: false,
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
        self.monitor.dpi as f32 / 96.0 * self.tokens.scale
    }

    pub fn studio_session(&mut self, active: bool) {
        self.studio_active = active;
        self.surface.set_no_activate(active);
        for island in &self.islands { island.set_no_activate(active); }
        if let Some(edge) = &self.edge_surface { edge.set_no_activate(active); }
    }
    pub fn owns(&self, id: SurfaceId) -> bool {
        self.surface.id() == id || self.islands.iter().any(|s| s.id() == id)
    }
    /// Whether `hwnd` is this bar's surface, one of its islands or its edge strip.
    pub fn owns_hwnd(&self, hwnd: windows::Win32::Foundation::HWND) -> bool {
        self.surface.hwnd() == hwnd
            || self.islands.iter().any(|s| s.hwnd() == hwnd)
            || self.edge_surface.as_ref().is_some_and(|s| s.hwnd() == hwnd)
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
    pub fn input_point(&self, id: SurfaceId, point: Point) -> Point {
        let index = if self.surface.id() == id {
            0
        } else {
            self.islands
                .iter()
                .position(|s| s.id() == id)
                .map_or(0, |i| i + 1)
        };
        let point = self
            .presentation_transforms
            .get(index)
            .map_or(point, |t| t.inverse(point));
        let offset = self.input_offset(id);
        Point::new(point.x + offset.x, point.y + offset.y)
    }

    fn presentation_transform(&self, full: Rect) -> Transform {
        let edge = edge(self.kind, &self.config);
        let middle = Point::new(
            (full.left + full.right) / 2.0,
            (full.top + full.bottom) / 2.0,
        );
        let anchor = match edge {
            Edge::Top => Rect::new(middle.x, full.top, 1.0, 1.0),
            Edge::Bottom => Rect::new(middle.x, full.bottom, 1.0, 1.0),
            Edge::Left => Rect::new(full.left, middle.y, 1.0, 1.0),
            Edge::Right => Rect::new(full.right, middle.y, 1.0, 1.0),
        };
        Transform {
            from: full,
            to: self.presentation.body(full, anchor, 1.0 - self.slide.value),
        }
    }

    pub fn windows(&mut self, windows: &[Rect]) {
        if self.windows != windows {
            self.windows = windows.to_vec();
            self.presentation_dirty = true;
        }
    }
    fn covered(&self, body: Rect) -> bool {
        if self.tokens.wallpaper_only {
            return false;
        }
        if self.tokens.live_blur {
            return true;
        }
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
    pub fn geometry(
        kind: BarKind,
        config: &BarsConfig,
        monitor: &Monitor,
    ) -> impl PartialEq + use<> {
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
        self.studio_base = None;
        if let Some(band) = self.studio_band.take() { self.band = band; }
        if let Some((slide, hide_at)) = self.studio_reveal.take() { self.slide = slide; self.hide_at = hide_at; }
        let motion = if self.kind == BarKind::Dock {
            &config.dock.animation
        } else {
            &config.top.animation
        };
        let dock = self.kind == BarKind::Dock;
        let performance = config.performance.active(mypaper_desktop::on_battery());
        let slide = motion::resolve(motion, dock, motion::Leaf::Show, animate, performance);
        self.magnify_settings =
            motion::resolve(motion, dock, motion::Leaf::Magnify, animate, performance);
        self.bounce_settings =
            motion::resolve(motion, dock, motion::Leaf::Bounce, animate, performance);
        self.attention_settings =
            motion::resolve(motion, dock, motion::Leaf::Attention, animate, performance);
        self.hover_settings =
            motion::resolve(motion, dock, motion::Leaf::Hover, animate, performance);
        self.press_settings =
            motion::resolve(motion, dock, motion::Leaf::Press, animate, performance);
        self.interactions.clear();
        self.presentation = motion::reveal(motion::presentation(motion, dock, motion::Leaf::Show));
        self.reveal_snapshots.clear();
        self.animations.clear();
        self.magnify_animate = self.magnify_settings.enabled;
        self.presence.configure(self.magnify_settings);
        self.zoom_due = true;
        self.animate = slide.enabled;
        self.slide.configure(slide);
        self.config = config;
        self.tokens = tokens;
        self.dirty = true;
        self.presentation_dirty = true;
    }
    pub fn horizontal(&self) -> bool {
        matches!(edge(self.kind, &self.config), Edge::Top | Edge::Bottom)
    }

    /// Repaint transient geometry without changing the appbar reservation or saved config.
    pub(super) fn durable_config(&self) -> &BarsConfig { self.studio_base.as_ref().unwrap_or(&self.config) }

    pub(super) fn studio_appearance(&mut self, config: BarsConfig, tokens: Tokens, transient: bool) {
        if transient {
            self.studio_base.get_or_insert_with(|| self.config.clone());
            self.studio_band.get_or_insert(self.band);
            if self.studio_reveal.is_none() {
                let mut shown = Value::new(0.0);
                shown.settings = self.slide.settings;
                self.studio_reveal = Some((std::mem::replace(&mut self.slide, shown), self.hide_at.take()));
            }
            let thickness = if self.kind == BarKind::Top { config.top.band() } else { config.dock.band() };
            self.band = appbar::settle(self.monitor.rect, edge(self.kind, &config),
                (thickness as f32 * self.monitor.dpi as f32 / 96.0 * tokens.scale).round() as i32);
        } else {
            self.studio_base = None;
            if let Some(band) = self.studio_band.take() { self.band = band; }
            if let Some((slide, hide_at)) = self.studio_reveal.take() { self.slide = slide; self.hide_at = hide_at; }
        }
        self.config = config;
        self.tokens = tokens;
        self.dirty = true;
        self.presentation_dirty = true;
        self.zoom_due = true;
    }
    pub fn shows_content(&self) -> bool {
        !self.hidden && self.enabled() && self.slide.value < 0.999
    }
    fn enabled(&self) -> bool {
        self.studio_base.is_none() && !self.studio_only
            || if self.kind == BarKind::Top { self.config.top.enabled } else { self.config.dock.enabled }
    }
    pub fn topics(&self) -> std::collections::BTreeSet<mypaper_bars::actions::Topic> {
        bar_topics(self.kind, &self.config)
    }
    pub fn hide_when_maximized(&self) -> bool {
        match self.kind {
            BarKind::Top => self.config.top.hide_when_maximized,
            BarKind::Dock => self.config.dock.hide_when_maximized,
        }
    }
    fn auto_hides(&self) -> bool {
        (self.kind == BarKind::Dock && self.config.dock.autohide) || self.maximized.active
    }
    /// A visibility transition changes only this reservation. Explorer resizes maximized apps;
    /// restored windows keep their positions (no global work-area setter or window enumeration).
    pub fn tick_maximized(&mut self, now: Instant) -> Result<()> {
        if self.studio_only || self.studio_base.is_some() { return Ok(()); }
        if let Some(hidden) = self.maximized.take_due(now) {
            if hidden && self.edge_surface.is_none() {
                self.edge_surface = Some(Surface::new_deferred_accessibility(
                    Kind::Bar,
                    "Reveal dock",
                    Role::Toolbar,
                )?);
            }
            if hidden {
                self.reservation = None;
            }
            self.reserve();
            self.hide_at = None;
            self.slide
                .target(if self.auto_hides() { 1.0 } else { 0.0 }, self.animate, now);
            self.dirty = true;
        }
        Ok(())
    }
    pub fn reserve(&mut self) {
        if self.studio_only || self.studio_base.is_some() { return; }
        if self.reservation.is_none() && !self.auto_hides() {
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
        self.reserved = granted.is_some() || self.auto_hides();
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
        if self.auto_hides() {
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
        match pointer.filter(|_| enabled) {
            Some(p) => {
                self.along = Some(if self.horizontal() { p.x } else { p.y });
                self.presence.target(1.0, true, now);
            }
            // The pointer's last place holds while the dock settles back, as the Dock's does.
            None => self.presence.target(0.0, true, now),
        }
        self.zoom_due = true;
    }

    /// Steps the enter/leave spring and lays the slots out under the pointer, at most once per
    /// display frame (mouse moves come faster). Returns whether the zoom changed.
    fn step_zoom(&mut self, now: Instant, moving: &mut bool) -> bool {
        if self.kind != BarKind::Dock {
            return false;
        }
        let before = self.presence.value;
        let springing = self.presence.step(now);
        *moving |= springing;
        if !self.zoom_due && before == self.presence.value {
            return false;
        }
        if now.saturating_duration_since(self.zoomed_at) < self.frame_interval {
            // A tick is due within the frame; the pointer's latest place is drawn then.
            *moving = true;
            return false;
        }
        self.zoom_due = false;
        self.zoomed_at = now;
        let size = self.config.dock.size() as f32 * self.scale();
        let presence = self.presence.value.clamp(0.0, 1.2);
        let max = 1.0 + (self.config.dock.magnify.scale as f32 - 1.0) * presence;
        let rest = &self.frame.rest;
        let next = match self.along {
            Some(p) if presence > 0.001 => motion::magnify(
                rest,
                p,
                size,
                max,
                self.config.dock.magnify.range as f32 * size,
            ),
            _ => vec![(1.0, 0.0); rest.len()],
        };
        let changed = next != self.zoom;
        self.zoom = next;
        changed
    }

    /// Tracks only entered items. Settlement removes resting entries and disarms the ticker.
    fn step_interactions(&mut self, now: Instant, moving: &mut bool) -> bool {
        for id in [self.hover, self.pressed].into_iter().flatten() {
            self.interactions.entry(id).or_insert_with(|| {
                let (mut hover, mut press) = (Value::new(0.0), Value::new(0.0));
                hover.configure(self.hover_settings);
                press.configure(self.press_settings);
                (hover, press)
            });
        }
        let mut changed = false;
        for (&id, (hover, press)) in &mut self.interactions {
            let before = (hover.value, press.value);
            let hot = self.hover == Some(id);
            hover.target(f32::from(hot && self.pressed.is_none()), true, now);
            press.target(f32::from(hot && self.pressed == Some(id)), true, now);
            *moving |= hover.step(now) | press.step(now);
            changed |= before != (hover.value, press.value);
            self.tokens
                .interaction
                .insert(id, (hover.value, press.value));
        }
        self.interactions.retain(|id, (hover, press)| {
            let keep = self.hover == Some(*id)
                || self.pressed == Some(*id)
                || hover.value != 0.0
                || press.value != 0.0;
            if !keep {
                self.tokens.interaction.remove(id);
            }
            keep
        });
        changed
    }

    pub fn animate_item(&mut self, key: String, launch: bool, now: Instant) {
        if (if launch {
            self.bounce_settings.enabled
        } else {
            self.attention_settings.enabled
        }) && (if launch {
            self.config.dock.bounce
        } else {
            self.config.dock.attention != mypaper_bars::config::AttentionStyle::None
        }) {
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
        if self.hidden || !self.enabled() {
            if self.reveal_snapshots.iter().any(Option::is_some) {
                self.surface.release();
                for island in &mut self.islands {
                    island.release();
                }
                self.reveal_snapshots.clear();
                self.presentation_dirty = true;
            }
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
        if self.studio_base.is_some() {
            self.slide.target(0.0, false, now);
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
        let previous_slide = self.slide.value;
        let mut moving = self.slide.step(now);
        let zoom_changed = self.step_zoom(now, &mut moving);
        let interaction_changed = self.step_interactions(now, &mut moving);
        let had_animation = !self.animations.is_empty();
        let attention = self.config.dock.attention;
        self.animations.retain(|key, (start, launch)| {
            now.duration_since(*start).as_secs_f32()
                < if *launch {
                    self.bounce_settings.duration
                } else {
                    self.attention_settings.duration
                }
                .as_secs_f32()
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
                    let timing = if *launch {
                        self.bounce_settings
                    } else {
                        self.attention_settings
                    };
                    let t = timing
                        .start(0.0, timing.duration.as_secs_f32(), *start)
                        .value(now);
                    if !*launch && attention == mypaper_bars::config::AttentionStyle::Pulse {
                        -(0.7 + 0.3 * (t * std::f32::consts::TAU / 0.4).cos())
                    } else {
                        (t * std::f32::consts::PI / 0.6).sin().abs() * 8.0 * self.scale()
                    }
                })
            })
            .collect();
        moving |= !self.animations.is_empty();
        let rebuilt = self.dirty || zoom_changed || interaction_changed || had_animation;
        let timing = Instant::now();
        if rebuilt {
            self.reveal_snapshots.clear();
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
                let zoom = self.zoom.clone();
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
        let built = timing.elapsed();
        if rebuilt || previous_slide != self.slide.value || self.presentation_dirty {
            self.present(ui)?;
            self.presentation_dirty = false;
        }
        if rebuilt && self.kind == BarKind::Dock {
            dock_timing(built, timing.elapsed() - built);
        }
        if self.slide.value >= 0.999 && self.auto_hides() {
            if self.surface.is_visible() {
                self.surface.hide();
            }
        } else if !self.surface.is_visible() {
            self.surface.show();
        }
        if first_paint && self.surface.is_visible() {
            mypaper_desktop::startup::ready("shell", "first_paint_ms");
        }
        let auto_hides = self.auto_hides();
        if let Some(edge) = &mut self.edge_surface {
            if !auto_hides {
                edge.hide();
                return Ok(moving);
            }
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
        self.presentation_transforms.clear();
        self.reveal_snapshots.resize_with(count, || None);
        let origin = self.origin();
        let visible = self.shows_content();
        for i in 0..count {
            let (body, radius) = self.frame.shapes.get(i).copied().unwrap_or((
                Rect::new(0.0, 0.0, self.frame.size.0 as f32, self.frame.size.1 as f32),
                0.0,
            ));
            let crop = if self.kind == BarKind::Top {
                body.inflate(self.tokens.shadow_outset(self.scale()).max(1.0))
            } else {
                Rect::new(0.0, 0.0, self.frame.size.0 as f32, self.frame.size.1 as f32)
            };
            let (mut list, offset, size) = if self.kind == BarKind::Top {
                self.frame.crop(crop)
            } else {
                (self.dock_list(ui), Point::default(), self.frame.size)
            };
            let full = if self.kind == BarKind::Top {
                body.offset(-offset.x, -offset.y)
            } else {
                Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32)
            };
            let transform = self.presentation_transform(full);
            self.presentation_transforms.push(transform);
            if self.presentation.scales() && self.slide.value > 0.0 && self.slide.value < 1.0 {
                if self.reveal_snapshots[i].is_none() {
                    self.reveal_snapshots[i] = Snapshot::new(ui, list.clone(), size).ok();
                }
                if let Some(snapshot) = &self.reveal_snapshots[i] {
                    list = snapshot.frame(transform.from, transform.to);
                }
            } else {
                self.reveal_snapshots[i] = None;
            }
            self.offsets.push(offset);
            let glass = self
                .material()
                .filter(|_| self.covered(body))
                .map(|material| Glass {
                    body: transform.rect(body.offset(-offset.x, -offset.y)),
                    radius: radius * transform.scale(),
                    material,
                });
            let surface = if i == 0 {
                &mut self.surface
            } else {
                &mut self.islands[i - 1]
            };
            surface.set_no_activate(self.studio_active);
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
                    .map(|h| transform.rect(h.rect.offset(-offset.x, -offset.y)))
                    .collect(),
            );
            let alpha = if self.presentation == Reveal::Fade {
                ((1.0 - self.slide.value).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                255
            };
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
            surface.set_alpha(alpha)?;
            // After the move: the surface opens or places its glass at the origin it has now.
            // Before it, glass opened at the previous frame's origin, ~a magnified dock's headroom
            // too high, and the engine's asynchronous open could miss the correcting place.
            surface.set_glass(glass.filter(|_| visible));
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

    /// What the dock's window draws: only its hit mask when its plate is glass (or bare) and
    /// the rest goes to sprites, else the whole frame.
    fn dock_list(&mut self, ui: &mut Ui) -> mypaper_ui::DisplayList {
        let t = &self.tokens;
        let glass_plate =
            t.bare || (t.fill.is_none() && t.material.is_some() && t.theme.glass.is_some());
        // Off by default: with the exact hit mask still re-uploaded each frame, a sweep cost more
        // CPU than drawing it all (27.6 % vs 20.1 %, 2026-11-05). `MYPAPER_DOCK_SPRITES=1` turns
        // it on; `=full` also keeps the whole window hit while composing (measures the floor).
        static MODE: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
        let mode = *MODE.get_or_init(|| match std::env::var("MYPAPER_DOCK_SPRITES").as_deref() {
            Ok("1") => Some(false),
            Ok("full") => Some(true),
            _ => None,
        });
        let Some(full_mask) = mode else {
            self.sprites = None;
            return self.frame.list.clone();
        };
        if self.sprites_failed
            || self.config.dock.style != DockStyle::Floating
            || !glass_plate
            || (self.presentation.scales() && self.slide.value > 0.0)
        {
            self.sprites = None;
            return self.frame.list.clone();
        }
        if self.sprites.is_none() {
            match super::sprites::Sprites::new(self.surface.hwnd()) {
                Ok(s) => self.sprites = Some(s),
                Err(e) => {
                    log::warn!("dock sprites: {e:#}");
                    self.sprites_failed = true;
                    return self.frame.list.clone();
                }
            }
        }
        let sprites = self.sprites.as_mut().expect("made above");
        match sprites.update(ui, &self.frame.list) {
            Ok(Some(mut mask)) if full_mask => {
                let (w, h) = self.frame.size;
                mask.ops = vec![mypaper_ui::Op::Fill {
                    rect: Rect::new(0.0, 0.0, w as f32, h as f32),
                    radius: 0.0,
                    color: mypaper_ui::Rgba::HIT,
                }];
                mask
            }
            Ok(Some(mask)) => mask,
            Ok(None) => self.frame.list.clone(),
            Err(e) => {
                log::warn!("dock sprites: {e:#}");
                self.sprites = None;
                self.sprites_failed = true;
                self.frame.list.clone()
            }
        }
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
        if self.kind == BarKind::Dock {
            let outset = self.tokens.shadow_outset(self.scale()) as i32;
            match e {
                Edge::Top => y -= outset,
                Edge::Bottom => y += outset,
                Edge::Left => x -= outset,
                Edge::Right => x += outset,
            }
        }
        let slide = if self.presentation == Reveal::Slide {
            self.slide.value
        } else {
            0.0
        };
        match e {
            Edge::Top => y -= ((h + margin) as f32 * slide).round() as i32,
            Edge::Bottom => y += ((h + margin) as f32 * slide).round() as i32,
            Edge::Left => x -= ((w + margin) as f32 * slide).round() as i32,
            Edge::Right => x += ((w + margin) as f32 * slide).round() as i32,
        }
        (x, y)
    }

    pub fn plates(&self) -> Vec<Plate> {
        if !self.shows_content() {
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
            .map(|(i, (rect, radius))| {
                let window = if self.kind == BarKind::Top { i } else { 0 };
                let offset = self.offsets.get(window).copied().unwrap_or_default();
                let transform =
                    self.presentation_transforms
                        .get(window)
                        .copied()
                        .unwrap_or(Transform {
                            from: *rect,
                            to: *rect,
                        });
                let rect = transform
                    .rect(rect.offset(-offset.x, -offset.y))
                    .offset(offset.x, offset.y);
                Plate {
                    id: if self.kind == BarKind::Top && i > 0 {
                        self.islands[i - 1].hwnd().0 as u64
                    } else {
                        (self.surface.hwnd().0 as u64).wrapping_add((i as u64) << 32)
                    },
                    rect: rect.offset(
                        (x - self.monitor.rect.x) as f32,
                        (y - self.monitor.rect.y) as f32,
                    ),
                    radius: *radius * transform.scale(),
                    material,
                }
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

    #[test]
    fn studio_preview_restores_auto_hide_and_keeps_reservations_untouched() {
        let config = BarsConfig::parse(r#"{"preset":"macos","dock":{"enabled":true,"autohide":true}}"#).unwrap();
        let tokens = Tokens::resolve(&config.dock.look, &Default::default(), &mypaper_bars::accent::read());
        let monitor = Monitor {
            device: "test".into(), dpi: 96, primary: true,
            rect: DesktopRect { x: 0, y: 0, width: 1280, height: 720 },
            work: DesktopRect { x: 0, y: 32, width: 1280, height: 688 },
        };
        let mut bar = Bar::new(BarKind::Dock, config.clone(), monitor, tokens.clone(), false).unwrap();
        let original_band = bar.band;
        assert_eq!(bar.slide.value, 1.0);
        let preview = BarsConfig::parse(r#"{"preset":"windows11"}"#).unwrap();
        bar.studio_appearance(preview, tokens.clone(), true);
        assert_eq!(bar.slide.value, 0.0);
        bar.reserve();
        assert!(bar.reservation.is_none());
        assert!(bar.studio_reveal.is_some());
        bar.studio_appearance(config, tokens, false);
        assert_eq!(bar.band, original_band);
        assert_eq!(bar.slide.value, 1.0);
        assert!(bar.reservation.is_none());
    }
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
        bar.kind = BarKind::Dock;
        bar.islands.clear();
        let plates = bar.plates();
        assert_eq!(plates.len(), 3);
        assert_ne!(plates[0].id, plates[1].id);
        assert_ne!(plates[1].id, plates[2].id);
    }
}
