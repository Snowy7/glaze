//! What each bar gets about its look besides `config` (docs/bars.md, "Look"): the resolved
//! appearance with the theme's CSS and variables, Windows' accent and light/dark mode, the icon
//! pack and copied images (`appearance`); reduce-motion, performance mode and whether live blur
//! is on (`mode`); and the wallpaper-glass snapshot with where the bar's window sits on its
//! monitor (`backdrop`). Each is re-sent to a bar only when it changed. Live blur itself (the
//! page's `glass` panes) is drawn here too, through `crate::blur`, and liquid glass's captures
//! of what's behind a bar (`capture`, `crate::capture`), with `behind` telling a bar when what's
//! behind it changed (its window moved, the wallpaper changed).

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use mypaper_bars::BarsConfig;
use mypaper_bars::config::Performance;
use mypaper_bars::style::{Fill, GlassKind, Look, Motion, ReduceMotion};
use mypaper_desktop::Rect;
use serde_json::{Value, json};

use super::App;
use crate::accent::{self, System};
use crate::assets::{Assets, is_image_path};
use crate::bar::Bar;
use crate::blur::{self, LiveBlur, Pane};
use crate::bridge::{BarError, BarKind};
use crate::capture::Behind;
use crate::glass::{BACKDROP_HOST, Backdrop, Glass};
use crate::look::{self, Pack};
use crate::wake::Waker;

pub(super) struct Looks {
    pub system: System,
    pub glass: Glass,
    pub assets: Assets,
    pub pack: Option<Pack>,
    /// The theme's variables, as CSS values.
    pub vars: BTreeMap<String, String>,
    pub on_battery: bool,
    /// Live blur's compositor, unless Windows refused one (logged).
    pub live: Option<LiveBlur>,
    /// Settings → Personalization → Colors → Transparency effects.
    transparency: bool,
    /// Liquid glass's captures of what's behind the bars.
    pub behind: Behind,
    /// What each bar last got per event, so unchanged ones aren't re-sent.
    sent: HashMap<(u32, &'static str), String>,
}

impl Looks {
    pub fn new(waker: Waker) -> Result<Self> {
        let live = LiveBlur::new()
            .inspect_err(|e| log::warn!("no live blur: {e:#}"))
            .ok();
        let backdrop = mypaper_bars::files::local("backdrop")?;
        Ok(Self {
            system: accent::read(),
            behind: Behind::new(backdrop.join("behind")),
            glass: Glass::new(backdrop, waker),
            assets: Assets::new(mypaper_bars::files::local("bars-assets")?),
            pack: None,
            vars: BTreeMap::new(),
            on_battery: mypaper_desktop::on_battery(),
            live,
            transparency: blur::transparency_on(),
            sent: HashMap::new(),
        })
    }

    /// A bar starts over (it said hello): it gets everything again.
    pub fn forget(&mut self, bar: u32) {
        self.sent.retain(|(id, _), _| *id != bar);
    }
}

/// Seconds between wallpaper snapshots a liquid bar asks for (as wallpaper glass's default).
const DEFAULT_INTERVAL: u32 = 10;

/// The look a bar draws: its own (`top.look` or `dock.look`, already layered over its design's
/// and the shared one).
pub(super) fn bar_look(bar: &Bar) -> &Look {
    match bar.kind {
        BarKind::Top => &bar.config.top.look,
        BarKind::Dock => &bar.config.dock.look,
    }
}

fn bar_motion(bar: &Bar) -> &Motion {
    match bar.kind {
        BarKind::Top => &bar.config.top.animation,
        BarKind::Dock => &bar.config.dock.animation,
    }
}

/// Image files a config names: picture backgrounds (either bar's) and module icons given as
/// files.
fn image_paths(config: &BarsConfig) -> impl Iterator<Item = &str> + '_ {
    let backgrounds = [&config.top.look, &config.dock.look]
        .into_iter()
        .filter_map(|look| match &look.background {
            Some(Fill::Image(image)) => Some(image.path.as_str()),
            _ => None,
        });
    let icons = config
        .modules
        .values()
        .filter_map(|m| m.icon.as_deref())
        .filter(|icon| is_image_path(icon));
    backgrounds.chain(icons)
}

impl App {
    /// Re-resolves what the look depends on besides the theme and `appearance`: the theme's
    /// variables, the icon pack and copied images. Then sends each bar what changed.
    pub(super) fn refresh_look(&mut self) {
        let (vars, problems) = mypaper_bars::vars::resolve(&self.skin.vars, &self.config.vars);
        for problem in problems {
            log::warn!("{problem}");
        }
        self.looks.vars = vars;
        self.looks.pack = self
            .config
            .icons
            .as_deref()
            .and_then(|name| look::load_pack(name, self.page.as_deref()));
        let paths: Vec<&str> = self
            .bars
            .iter()
            .flat_map(|bar| image_paths(&bar.config))
            .chain(image_paths(&self.config))
            .collect();
        let theme = self.skin.dir.as_deref();
        self.looks.assets.sync(paths.into_iter(), theme);
        self.push_looks();
    }

    /// The `appearance` event (also part of `hello`).
    pub(super) fn look_payload(&self, bar: &Bar) -> Value {
        json!({
            "appearance": self.look(bar.kind),
            "themeCss": self.skin.css,
            "system": self.looks.system,
            "vars": self.looks.vars,
            "iconPack": self.looks.pack,
            "assets": self.looks.assets.urls(),
            "glass": self.skin.glass,
        })
    }

    fn performance(&self, bar: &Bar) -> bool {
        match bar.config.performance {
            Performance::Auto => self.looks.on_battery,
            Performance::On => true,
            Performance::Off => false,
        }
    }

    /// The `mode` event: reduce motion (Windows' setting unless the bar's `reduceMotion` says
    /// otherwise), performance mode (no blur, no animation), and whether live blur draws (this
    /// window has it, transparency effects are on, performance mode is off).
    pub(super) fn mode_payload(&self, bar: &Bar) -> Value {
        let reduce = match bar_motion(bar).reduce_motion.unwrap_or_default() {
            ReduceMotion::System => !self.looks.system.animations,
            ReduceMotion::Always => true,
            ReduceMotion::Never => false,
        };
        let performance = self.performance(bar);
        let blur =
            self.looks.live.is_some() && bar.has_blur() && self.looks.transparency && !performance;
        // Liquid glass needs no compositor: only the user's transparency choice and full power.
        let liquid = self.looks.transparency && !performance;
        json!({ "reduceMotion": reduce, "performance": performance, "blur": blur, "liquid": liquid })
    }

    /// The `backdrop` event: the wallpaper snapshot of the bar's monitor, its size and where the
    /// bar's window sits on it (DIPs), or null without wallpaper glass or a snapshot.
    pub(super) fn backdrop_payload(&self, bar: &Bar) -> Value {
        if bar_look(bar).wallpaper().is_none() {
            return Value::Null;
        }
        let Some(url) = self.looks.glass.url(&bar.monitor.device) else {
            return Value::Null;
        };
        let scale = bar.scale();
        let monitor = bar.monitor.rect;
        json!(Backdrop {
            url,
            width: f64::from(monitor.width) / scale,
            height: f64::from(monitor.height) / scale,
            x: f64::from(bar.rect.x - monitor.x) / scale,
            y: f64::from(bar.rect.y - monitor.y) / scale,
        })
    }

    /// Sends each ready bar its `appearance`, `mode` and `backdrop` where they changed, and keeps
    /// the engine's snapshot request in line with the bars on screen.
    pub(super) fn push_looks(&mut self) {
        for index in 0..self.bars.len() {
            let bar = &self.bars[index];
            if !bar.ready {
                continue;
            }
            // What's behind it changes when its window moves or the wallpaper's snapshot does.
            let behind = json!([
                bar.rect.x,
                bar.rect.y,
                bar.rect.width,
                bar.rect.height,
                self.looks.glass.url(&bar.monitor.device)
            ]);
            let events = [
                ("appearance", self.look_payload(bar)),
                ("mode", self.mode_payload(bar)),
                ("backdrop", self.backdrop_payload(bar)),
                ("behind", behind),
            ];
            for (event, data) in events {
                let message = json!({ "event": event, "data": data }).to_string();
                if self.looks.sent.get(&(bar.id, event)) != Some(&message) {
                    bar.post(&message);
                    self.looks.sent.insert((bar.id, event), message);
                }
            }
        }
        self.sync_glass();
    }

    /// Wallpaper glass on screen: periodic snapshots at the shortest interval a visible bar
    /// wants, none in performance mode (one per theme load still comes).
    pub(super) fn sync_glass(&mut self) {
        let interval = self
            .bars
            .iter()
            .filter(|bar| !bar.hidden && !self.performance(bar))
            .filter_map(|bar| match bar_look(bar).wallpaper() {
                Some(wallpaper) => Some(wallpaper.interval),
                // A liquid bar re-captures on each snapshot, so it follows the wallpaper too.
                None => self.liquid(bar).then_some(DEFAULT_INTERVAL),
            })
            .min();
        self.looks.glass.request(interval);
    }

    /// Whether `bar` draws liquid glass: its look says so, or says nothing while the theme's
    /// appearance style is liquid (the page decides the same way, `style/look.ts`).
    fn liquid(&self, bar: &Bar) -> bool {
        match bar_look(bar).glass.style {
            Some(kind) => kind == GlassKind::Liquid,
            None => self.look(bar.kind).style.as_deref() == Some("liquid"),
        }
    }

    /// The page's `capture` request: `rect` (DIPs from the window's top-left) as the screen
    /// shows it without the bars, clipped to the monitor. Answers the file's URL and the rect
    /// it covers (DIPs from the window's top-left).
    pub(super) fn capture_behind(
        &mut self,
        index: usize,
        rect: [f64; 4],
    ) -> Result<Value, BarError> {
        let bar = &self.bars[index];
        if rect.iter().any(|v| !v.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
            return Err(BarError::invalid("capture: a rect with a size"));
        }
        let scale = bar.scale();
        let monitor = bar.monitor.rect;
        let px = |v: f64| (v * scale).round() as i32;
        let x0 = (bar.rect.x + px(rect[0])).max(monitor.x);
        let y0 = (bar.rect.y + px(rect[1])).max(monitor.y);
        let x1 = (bar.rect.x + px(rect[0] + rect[2])).min(monitor.x + monitor.width);
        let y1 = (bar.rect.y + px(rect[1] + rect[3])).min(monitor.y + monitor.height);
        if x1 <= x0 || y1 <= y0 {
            return Err(BarError::invalid("capture: the rect is off the monitor"));
        }
        let area = Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        };
        let ours: Vec<_> = self.bars.iter().map(Bar::hwnd).collect();
        let id = bar.id;
        let (origin_x, origin_y) = (bar.rect.x, bar.rect.y);
        let file = self
            .looks
            .behind
            .capture(id, &ours, area)
            .map_err(|e| BarError::unavailable(format!("{e:#}")))?;
        let dip = |v: i32| f64::from(v) / scale;
        Ok(json!({
            "url": format!("https://{BACKDROP_HOST}/behind/{file}"),
            "rect": [dip(x0 - origin_x), dip(y0 - origin_y), dip(area.width), dip(area.height)],
        }))
    }

    /// The page's `glass` request: where its live-blur panes are now.
    pub(super) fn set_glass(&mut self, index: usize, panes: &[Pane]) {
        if let Some(live) = &mut self.looks.live {
            self.bars[index].set_glass(live, panes);
        }
    }

    /// Windows' colors, light/dark mode, transparency or animation setting may have changed.
    pub(super) fn settings_changed(&mut self) {
        self.emit(crate::bridge::Topic::Clock);
        let system = accent::read();
        let transparency = blur::transparency_on();
        if system != self.looks.system || transparency != self.looks.transparency {
            log::info!("system colors, transparency or animations changed");
            self.looks.system = system;
            self.looks.transparency = transparency;
            self.push_looks();
        }
    }

    /// The power source changed: performance mode may follow it.
    pub(super) fn power_changed(&mut self, on_battery: bool) {
        if on_battery != self.looks.on_battery {
            self.looks.on_battery = on_battery;
            self.push_looks();
        }
    }

    /// The engine wrote a snapshot.
    pub(super) fn backdrop_changed(&mut self) {
        if self.looks.glass.rescan() {
            self.push_looks();
        }
    }
}
