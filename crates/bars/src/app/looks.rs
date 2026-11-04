//! What each bar gets about its look besides `config` (docs/bars.md, "Look"): the resolved
//! appearance with the theme's CSS and variables, Windows' accent and light/dark mode, the icon
//! pack and copied images (`appearance`); reduce-motion and performance mode (`mode`); and the
//! wallpaper-glass snapshot with where the bar's window sits on its monitor (`backdrop`). Each is
//! re-sent to a bar only when it changed.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use mypaper_bars::BarsConfig;
use mypaper_bars::config::Performance;
use mypaper_bars::style::{Fill, ReduceMotion};
use serde_json::{Value, json};

use super::App;
use crate::accent::{self, System};
use crate::assets::{Assets, is_image_path};
use crate::bar::Bar;
use crate::glass::{Backdrop, Glass};
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
    /// What each bar last got per event, so unchanged ones aren't re-sent.
    sent: HashMap<(u32, &'static str), String>,
}

impl Looks {
    pub fn new(waker: Waker) -> Result<Self> {
        Ok(Self {
            system: accent::read(),
            glass: Glass::new(mypaper_bars::files::local("backdrop")?, waker),
            assets: Assets::new(mypaper_bars::files::local("bars-assets")?),
            pack: None,
            vars: BTreeMap::new(),
            on_battery: mypaper_desktop::on_battery(),
            sent: HashMap::new(),
        })
    }

    /// A bar starts over (it said hello): it gets everything again.
    pub fn forget(&mut self, bar: u32) {
        self.sent.retain(|(id, _), _| *id != bar);
    }
}

/// Image files a config names: a picture background and module icons given as files.
fn image_paths(config: &BarsConfig) -> impl Iterator<Item = &str> + '_ {
    let background = match &config.look.background {
        Some(Fill::Image(image)) => Some(image.path.as_str()),
        _ => None,
    };
    let icons = config
        .modules
        .values()
        .filter_map(|m| m.icon.as_deref())
        .filter(|icon| is_image_path(icon));
    background.into_iter().chain(icons)
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
        })
    }

    fn performance(&self, bar: &Bar) -> bool {
        match bar.config.performance {
            Performance::Auto => self.looks.on_battery,
            Performance::On => true,
            Performance::Off => false,
        }
    }

    /// The `mode` event: reduce motion (Windows' setting unless `animation.reduceMotion` says
    /// otherwise) and performance mode (no blur, no animation).
    pub(super) fn mode_payload(&self, bar: &Bar) -> Value {
        let reduce = match bar.config.animation.reduce_motion.unwrap_or_default() {
            ReduceMotion::System => !self.looks.system.animations,
            ReduceMotion::Always => true,
            ReduceMotion::Never => false,
        };
        json!({ "reduceMotion": reduce, "performance": self.performance(bar) })
    }

    /// The `backdrop` event: the wallpaper snapshot of the bar's monitor, its size and where the
    /// bar's window sits on it (DIPs), or null without wallpaper glass or a snapshot.
    pub(super) fn backdrop_payload(&self, bar: &Bar) -> Value {
        if bar.config.look.wallpaper().is_none() {
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
            let events = [
                ("appearance", self.look_payload(bar)),
                ("mode", self.mode_payload(bar)),
                ("backdrop", self.backdrop_payload(bar)),
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
            .filter_map(|bar| bar.config.look.wallpaper())
            .map(|wallpaper| wallpaper.interval)
            .min();
        self.looks.glass.request(interval);
    }

    /// Windows' colors, light/dark mode or animation setting may have changed.
    pub(super) fn settings_changed(&mut self) {
        let system = accent::read();
        if system != self.looks.system {
            log::info!("system colors or animations changed");
            self.looks.system = system;
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
