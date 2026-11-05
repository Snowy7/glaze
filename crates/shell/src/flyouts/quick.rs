//! Quick settings with real system controls. DND, Focus, Night light and Energy saver have
//! no unrestricted setter, so they are not switches here. Secondary clicks open Settings.

use std::time::{Duration, Instant};

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::quick::{self as provider, PowerMode, RadioKind};
use mypaper_bars::network::{self, Kind as Link, NetworkInfo};
use mypaper_bars::system::{self, Battery};
use mypaper_bars::wake::Msg;
use mypaper_ui::{HitId, Point};

use super::controls;
use super::kit::{DECORATION, Kit, Tile, glyph};
use super::{Cx, Effect, Kind, Panel, Reply};

const WIFI: HitId = 1;
const WIFI_MORE: HitId = 2;
const BLUETOOTH: HitId = 3;
const POWER: HitId = 4;
const HOTSPOT: HitId = 5;
const BRIGHTNESS: HitId = 9;
const VOLUME: HitId = 10;
const MUTE: HitId = 11;
const MIXER: HitId = 12;
const SETTINGS: HitId = 13;
const BLUETOOTH_MORE: HitId = 14;

pub struct Quick {
    state: Option<provider::Quick>,
    volume: Option<Volume>,
    network: NetworkInfo,
    battery: Option<Battery>,
    /// The brightness being dragged (applied when the drag ends: DDC/CI is slow).
    brightness: Option<u32>,
    error: Option<String>,
    pending: bool,
    refreshing: bool,
    hotspot: Option<bool>,
    display_due: Instant,
}

impl Quick {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        cx.lanes
            .quick(|_| Reply::QuickControl(controls::read(), None));
        Self {
            state: None,
            // The endpoint (COM activation, tens of ms cold) is opened once on screen.
            volume: None,
            network: network::read(None),
            battery: system::battery(),
            brightness: None,
            error: None,
            pending: true,
            refreshing: false,
            hotspot: None,
            display_due: Instant::now() + Duration::from_secs(5),
        }
    }

    /// Shown with this state (tests).
    #[cfg(test)]
    pub fn with(state: provider::Quick, volume: Option<Volume>, battery: Option<Battery>) -> Self {
        Self {
            state: Some(state),
            volume,
            network: NetworkInfo {
                kind: Link::Wifi,
                name: Some("Home".into()),
                signal: Some(80),
            },
            battery,
            brightness: None,
            error: None,
            pending: false,
            refreshing: false,
            hotspot: None,
            display_due: Instant::now() + Duration::from_secs(5),
        }
    }

    fn radio(&self, kind: RadioKind) -> Option<bool> {
        self.state
            .as_ref()?
            .radios
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.on)
            .reduce(|a, b| a || b)
    }

    fn set_radio(&mut self, kind: RadioKind, cx: &mut Cx) -> Effect {
        if self.pending || self.refreshing {
            return Effect::None;
        }
        let Some(on) = self.radio(kind) else {
            return Effect::None;
        };
        self.pending = true;
        change_radio(cx, kind, !on);
        Effect::Redraw
    }
}

/// Request consent in a UI context, wait and write off-thread, and always read back the OS
/// state, even on failure. Never leave an optimistic switch showing a refused operation.
pub(super) fn change_radio(cx: &mut Cx, kind: RadioKind, on: bool) {
    let access = windows::Devices::Radios::Radio::RequestAccessAsync();
    cx.lanes.quick(move |_| {
        let result = access
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())
            .and_then(|access| controls::set_radio(kind, on, access));
        Reply::QuickControl(controls::read(), result.err())
    });
}

fn settings(uri: &str) -> Effect {
    system::open(uri.into());
    Effect::Close
}

fn power_label(mode: PowerMode) -> &'static str {
    match mode {
        PowerMode::Efficiency => "Best efficiency",
        PowerMode::Balanced => "Balanced",
        PowerMode::Performance => "Best performance",
    }
}

fn next_power(mode: PowerMode) -> PowerMode {
    match mode {
        PowerMode::Efficiency => PowerMode::Balanced,
        PowerMode::Balanced => PowerMode::Performance,
        PowerMode::Performance => PowerMode::Efficiency,
    }
}

impl Panel for Quick {
    fn width(&self) -> f32 {
        340.0
    }

    fn view(&self, kit: &mut Kit) {
        let state = self.state.as_ref();
        let on_off = |on: bool| if on { "On" } else { "Off" };
        let wifi = self.radio(RadioKind::Wifi);
        let wifi_detail = match wifi {
            None if state.is_none() => "…".to_owned(),
            None => "Unavailable".to_owned(),
            Some(true) => self
                .network
                .name
                .clone()
                .filter(|_| self.network.kind == Link::Wifi)
                .unwrap_or_else(|| "On".into()),
            Some(false) => "Off".into(),
        };
        let bluetooth = self.radio(RadioKind::Bluetooth);
        let power = state.and_then(|s| s.power);
        let mut tiles = vec![
            Tile {
                id: WIFI,
                more: Some(WIFI_MORE),
                glyph: glyph::WIFI[3],
                label: "Wi-Fi",
                detail: &wifi_detail,
                on: wifi == Some(true),
                enabled: wifi.is_some() && !self.pending,
            },
            Tile {
                id: BLUETOOTH,
                more: Some(BLUETOOTH_MORE),
                glyph: glyph::BLUETOOTH,
                label: "Bluetooth",
                detail: match bluetooth {
                    None if state.is_none() => "…",
                    None => "Unavailable",
                    Some(on) => on_off(on),
                },
                on: bluetooth == Some(true),
                enabled: bluetooth.is_some() && !self.pending,
            },
            Tile {
                id: POWER,
                more: None,
                glyph: glyph::GAUGE,
                label: "Power mode",
                detail: power.map_or("Unavailable", power_label),
                on: power.is_some_and(|p| p != PowerMode::Balanced),
                enabled: power.is_some() && !self.pending,
            },
        ];
        if let Some(on) = self.hotspot {
            tiles.push(Tile {
                id: HOTSPOT,
                more: None,
                glyph: glyph::WIFI[3],
                label: "Mobile hotspot",
                detail: on_off(on),
                on,
                enabled: !self.pending,
            });
        }
        kit.tiles(&tiles);
        if let Some(level) = self.brightness.or(state.and_then(|s| s.brightness)) {
            kit.slider(
                BRIGHTNESS,
                level as f32 / 100.0,
                "Brightness",
                Some((DECORATION, glyph::BRIGHTNESS, "Brightness")),
                None,
            );
        }
        if let Some(volume) = self.volume {
            let level = if volume.muted { 0.0 } else { volume.level };
            let icon = if volume.muted || level == 0.0 {
                glyph::MUTE
            } else {
                glyph::VOLUME[((level * 3.0).ceil() as usize).min(3)]
            };
            kit.slider(
                VOLUME,
                level,
                "Volume",
                Some((MUTE, icon, if volume.muted { "Unmute" } else { "Mute" })),
                Some((MIXER, glyph::CHEVRON_RIGHT, "Volume mixer")),
            );
        }
        if let Some(error) = &self.error {
            kit.paragraph(error, 2, kit.look.dim());
        }
        kit.separator();
        let height = kit.px(30.0);
        let center = kit.y + height / 2.0;
        if let Some(b) = self.battery {
            let text = format!(
                "{}%{}",
                b.percent,
                if b.charging { " · Charging" } else { "" }
            );
            let style = kit.font(13.0, 400);
            let (_, h) = kit.text.measure(&text, &style);
            let (left, room) = (kit.left, kit.width() - kit.px(40.0));
            kit.text_at(left, center - h / 2.0, &text, &style, kit.look.dim(), room);
        }
        let right = kit.right;
        kit.icon_button(
            SETTINGS,
            Point::new(right - kit.px(15.0), center),
            glyph::SETTINGS,
            "All settings",
        );
        kit.y += height;
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.error = None;
        match id {
            WIFI => self.set_radio(RadioKind::Wifi, cx),
            WIFI_MORE => Effect::Switch(Kind::Wifi),
            BLUETOOTH => self.set_radio(RadioKind::Bluetooth, cx),
            BLUETOOTH_MORE => settings("ms-settings:bluetooth"),
            POWER => {
                if self.pending || self.refreshing {
                    return Effect::None;
                }
                let Some(mode) = self.state.as_ref().and_then(|s| s.power) else {
                    return Effect::None;
                };
                let next = next_power(mode);
                self.pending = true;
                cx.lanes.quick(move |_| {
                    let error = controls::set_power_mode(next).err();
                    Reply::QuickControl(controls::read(), error)
                });
                Effect::Redraw
            }
            HOTSPOT => {
                if self.pending || self.refreshing {
                    return Effect::None;
                }
                let Some(on) = self.hotspot else {
                    return Effect::None;
                };
                self.pending = true;
                cx.lanes.quick(move |_| {
                    let error = controls::set_hotspot(!on).err();
                    Reply::QuickControl(controls::read(), error)
                });
                Effect::Redraw
            }
            MUTE => {
                let muted = self.volume.is_some_and(|v| v.muted);
                self.volume = cx
                    .lanes
                    .audio()
                    .and_then(|a| a.change(None, Some(!muted), None));
                Effect::Redraw
            }
            MIXER => Effect::Switch(Kind::Mixer),
            SETTINGS => settings("ms-settings:"),
            _ => Effect::None,
        }
    }

    fn slide(&mut self, id: HitId, value: f32, done: bool, cx: &mut Cx) -> Effect {
        match id {
            VOLUME => {
                let unmute = self.volume.is_some_and(|v| v.muted).then_some(false);
                self.volume = cx
                    .lanes
                    .audio()
                    .and_then(|a| a.change(Some(value), unmute, None));
                Effect::Redraw
            }
            BRIGHTNESS => {
                let level = (value * 100.0).round() as u32;
                self.brightness = Some(level);
                if done {
                    cx.lanes
                        .quick(move |lane| match lane.set_brightness(level) {
                            Ok(()) => Reply::Quick(lane.read()),
                            Err(e) => Reply::Failed(e),
                        });
                }
                Effect::Redraw
            }
            _ => Effect::None,
        }
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::Quick(state) => {
                let mut state = state.clone();
                // The slow brightness response must not overwrite more recent radio/power state.
                if let Some(current) = &self.state {
                    state.radios = current.radios.clone();
                    state.power = current.power;
                }
                let changed = self.brightness.is_some() || self.state.as_ref() != Some(&state);
                self.state = Some(state);
                self.brightness = None;
                changed
            }
            Reply::QuickControl(live, error) => {
                let changed = self.pending
                    || self.hotspot != live.hotspot
                    || error.is_some()
                    || self
                        .state
                        .as_ref()
                        .is_none_or(|s| s.radios != live.radios || s.power != live.power);
                self.pending = false;
                self.refreshing = false;
                self.hotspot = live.hotspot;
                if error.is_some() {
                    self.error = error.clone();
                }
                let state = self.state.get_or_insert_with(|| provider::Quick {
                    radios: vec![],
                    brightness: None,
                    power: None,
                    saver: None,
                    dnd: None,
                    focus: None,
                });
                state.radios = live.radios.clone();
                state.power = live.power;
                changed
            }
            Reply::Failed(e) => {
                log::warn!("quick settings: {e}");
                self.error = Some(e.clone());
                self.brightness = None;
                cx.lanes.quick(|lane| Reply::Quick(lane.read()));
                true
            }
            _ => false,
        }
    }

    fn opened(&mut self, cx: &mut Cx) -> bool {
        self.volume = cx.lanes.audio().and_then(|a| a.read());
        self.volume.is_some()
    }

    fn context(&mut self, id: HitId, _: &mut Cx) -> Effect {
        match id {
            WIFI => settings("ms-settings:network-wifi"),
            BLUETOOTH => settings("ms-settings:bluetooth"),
            POWER => settings("ms-settings:powersleep"),
            HOTSPOT => settings("ms-settings:network-mobilehotspot"),
            _ => Effect::None,
        }
    }

    fn tick(&mut self, now: Instant, cx: &mut Cx) -> (bool, Option<Instant>) {
        if !self.pending && !self.refreshing {
            self.refreshing = true;
            cx.lanes
                .quick(|_| Reply::QuickControl(controls::read(), None));
        }
        if now >= self.display_due && self.brightness.is_none() {
            self.display_due = now + Duration::from_secs(5);
            cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        }
        (false, Some(now + Duration::from_secs(1)))
    }

    fn changed(&mut self, msg: &Msg, cx: &mut Cx) -> bool {
        if matches!(msg, Msg::Volume) {
            let audio = cx.lanes.audio.as_mut();
            if let Some(a) = audio {
                a.follow_default();
                self.volume = a.read();
            }
            return true;
        }
        if matches!(msg, Msg::Network) {
            let network = network::read(None);
            let changed = self.network != network;
            self.network = network;
            return changed;
        }
        false
    }
}
