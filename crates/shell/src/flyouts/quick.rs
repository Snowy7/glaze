//! Quick settings: Wi-Fi and Bluetooth (on the `quick` lane), power mode, battery saver, do not
//! disturb and focus (shown; their settings pages change them, there is no API), night light,
//! brightness and volume. Wi-Fi's and volume's chevrons open the Wi-Fi list and the mixer.

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::quick::{self as provider, Dnd, PowerMode, QuickLane, RadioKind};
use mypaper_bars::network::{self, Kind as Link, NetworkInfo};
use mypaper_bars::system::{self, Battery};
use mypaper_bars::wake::Msg;
use mypaper_ui::{HitId, Point};

use super::kit::{DECORATION, Kit, Tile, glyph};
use super::{Cx, Effect, Kind, Panel, Reply};

const WIFI: HitId = 1;
const WIFI_MORE: HitId = 2;
const BLUETOOTH: HitId = 3;
const POWER: HitId = 4;
const SAVER: HitId = 5;
const DND: HitId = 6;
const FOCUS: HitId = 7;
const NIGHT: HitId = 8;
const BRIGHTNESS: HitId = 9;
const VOLUME: HitId = 10;
const MUTE: HitId = 11;
const MIXER: HitId = 12;
const SETTINGS: HitId = 13;

pub struct Quick {
    state: Option<provider::Quick>,
    volume: Option<Volume>,
    network: NetworkInfo,
    battery: Option<Battery>,
    /// The brightness being dragged (applied when the drag ends: DDC/CI is slow).
    brightness: Option<u32>,
    error: Option<String>,
}

impl Quick {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        Self {
            state: None,
            volume: cx.lanes.audio().and_then(|a| a.read()),
            network: network::read(None),
            battery: system::battery(),
            brightness: None,
            error: None,
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
        }
    }

    fn radio(&self, kind: RadioKind) -> Option<bool> {
        self.state
            .as_ref()?
            .radios
            .iter()
            .find(|r| r.kind == kind)
            .map(|r| r.on)
    }

    fn set_radio(&mut self, kind: RadioKind, cx: &mut Cx) -> Effect {
        let Some(on) = self.radio(kind) else {
            return settings(match kind {
                RadioKind::Wifi => "ms-settings:network-wifi",
                _ => "ms-settings:bluetooth",
            });
        };
        if let Some(radio) = self
            .state
            .as_mut()
            .and_then(|s| s.radios.iter_mut().find(|r| r.kind == kind))
        {
            radio.on = !on;
        }
        cx.lanes.quick(
            move |lane: &mut QuickLane| match lane.set_radio(kind, !on) {
                Ok(()) => Reply::Quick(lane.read()),
                Err(e) => Reply::Failed(e),
            },
        );
        Effect::Redraw
    }
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
                enabled: wifi.is_some() || state.is_none(),
            },
            Tile {
                id: BLUETOOTH,
                more: None,
                glyph: glyph::BLUETOOTH,
                label: "Bluetooth",
                detail: match bluetooth {
                    None if state.is_none() => "…",
                    None => "Unavailable",
                    Some(on) => on_off(on),
                },
                on: bluetooth == Some(true),
                enabled: bluetooth.is_some() || state.is_none(),
            },
            Tile {
                id: POWER,
                more: None,
                glyph: glyph::GAUGE,
                label: "Power mode",
                detail: power.map_or("Settings", power_label),
                on: power.is_some_and(|p| p != PowerMode::Balanced),
                enabled: true,
            },
        ];
        if let Some(saver) = state.and_then(|s| s.saver) {
            tiles.push(Tile {
                id: SAVER,
                more: None,
                glyph: glyph::SAVER,
                label: "Battery saver",
                detail: on_off(saver),
                on: saver,
                enabled: true,
            });
        }
        if let Some(dnd) = state.and_then(|s| s.dnd) {
            tiles.push(Tile {
                id: DND,
                more: None,
                glyph: glyph::MOON,
                label: "Do not disturb",
                detail: match dnd {
                    Dnd::Off => "Off",
                    Dnd::Priority => "Priority only",
                    Dnd::Alarms => "Alarms only",
                },
                on: dnd != Dnd::Off,
                enabled: true,
            });
        }
        if let Some(focus) = state.and_then(|s| s.focus) {
            tiles.push(Tile {
                id: FOCUS,
                more: None,
                glyph: glyph::FOCUS,
                label: "Focus",
                detail: if focus { "In a session" } else { "Off" },
                on: focus,
                enabled: true,
            });
        }
        tiles.push(Tile {
            id: NIGHT,
            more: None,
            glyph: glyph::BRIGHTNESS,
            label: "Night light",
            detail: "Settings",
            on: false,
            enabled: true,
        });
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
            POWER => {
                let Some(mode) = self.state.as_ref().and_then(|s| s.power) else {
                    return settings("ms-settings:powersleep");
                };
                let next = next_power(mode);
                if let Some(s) = &mut self.state {
                    s.power = Some(next);
                }
                cx.lanes
                    .quick(move |lane| match provider::set_power_mode(next) {
                        Ok(()) => Reply::Quick(lane.read()),
                        Err(e) => Reply::Failed(e),
                    });
                Effect::Redraw
            }
            SAVER => settings("ms-settings:batterysaver"),
            DND => settings("ms-settings:notifications"),
            FOCUS => settings("ms-settings:quiethours"),
            NIGHT => settings("ms-settings:nightlight"),
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

    fn reply(&mut self, reply: &Reply, _: &mut Cx) -> bool {
        match reply {
            Reply::Quick(state) => {
                self.state = Some(state.clone());
                self.brightness = None;
                true
            }
            Reply::Failed(e) => {
                log::warn!("quick settings: {e}");
                self.error = Some(e.clone());
                true
            }
            _ => false,
        }
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
        false
    }
}
