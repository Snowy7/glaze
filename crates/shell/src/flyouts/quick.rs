//! Quick settings, laid out as macOS's Control Center on the kit: toggle tiles two to a row,
//! the display and sound sliders in their own cards, the battery and Settings at the foot.
//!
//! Every tile is a real switch with live system readback (`controls`, read every second while
//! open): radios, power mode and the hotspot through documented APIs; do not disturb, night
//! light and airplane mode through `toggles` (undocumented, read back after each change). A
//! tile shows the state Windows reports, never the one asked for. Secondary clicks open the
//! matching Settings page.

use std::time::{Duration, Instant};

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::quick::{PowerMode, RadioKind};
use mypaper_bars::network::{self, Kind as Link, NetworkInfo};
use mypaper_bars::system::{self, Battery};
use mypaper_bars::wake::Msg;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{self, Align, Stack};
use mypaper_ui::{HitId, TextRole, Tile};

use super::controls;
use super::kit::{Kit, slider_card, slider_card_height, volume_icon};
use super::toggles::Toggle;
use super::{Cx, Effect, Kind, Panel, Reply};

const WIFI: HitId = 1;
const WIFI_MORE: HitId = 2;
const BLUETOOTH: HitId = 3;
const POWER: HitId = 4;
const HOTSPOT: HitId = 5;
const DND: HitId = 6;
const NIGHT_LIGHT: HitId = 7;
const AIRPLANE: HitId = 8;
const BRIGHTNESS: HitId = 9;
const VOLUME: HitId = 10;
const MUTE: HitId = 11;
const MIXER: HitId = 12;
const SETTINGS: HitId = 13;
const BLUETOOTH_MORE: HitId = 14;

/// A tile's height and the gap between tiles, design px.
const TILE: f32 = 58.0;
const GAP: f32 = 10.0;

pub struct Quick {
    /// What Windows reports (radios, power mode, the toggles), `None` until first read.
    live: Option<controls::State>,
    /// The display's brightness, 0–100 (its own slower read: DDC/CI).
    display: Option<u32>,
    volume: Option<Volume>,
    network: NetworkInfo,
    battery: Option<Battery>,
    /// The brightness being dragged (applied when the drag ends: DDC/CI is slow).
    brightness: Option<u32>,
    error: Option<String>,
    /// A change is on its way; tiles wait for its readback.
    pending: bool,
    refreshing: bool,
    display_due: Instant,
}

impl Quick {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        cx.lanes
            .quick(|_| Reply::QuickControl(controls::read(), None));
        Self {
            live: None,
            display: None,
            // The endpoint (COM activation, tens of ms cold) is opened once on screen.
            volume: None,
            network: network::read(None),
            battery: system::battery(),
            brightness: None,
            error: None,
            pending: true,
            refreshing: false,
            display_due: Instant::now() + Duration::from_secs(5),
        }
    }

    /// Shown with this state (tests).
    #[cfg(test)]
    pub fn with(
        live: controls::State,
        display: Option<u32>,
        volume: Option<Volume>,
        battery: Option<Battery>,
    ) -> Self {
        Self {
            live: Some(live),
            display,
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
            display_due: Instant::now() + Duration::from_secs(5),
        }
    }

    fn radio(&self, kind: RadioKind) -> Option<bool> {
        self.live
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

    /// Flips `toggle` from the state Windows last reported, then reads everything back.
    fn flip(&mut self, toggle: Toggle, on: Option<bool>, cx: &mut Cx) -> Effect {
        let Some(on) = on.filter(|_| !self.pending && !self.refreshing) else {
            return Effect::None;
        };
        self.pending = true;
        cx.lanes.quick(move |_| {
            let error = toggle.set(!on).err();
            Reply::QuickControl(controls::read(), error)
        });
        Effect::Redraw
    }

    /// The tiles, in Control Center's order: connectivity, then focus and display, then power.
    fn tiles(&self) -> Vec<(Tile<'static>, String)> {
        let live = self.live.as_ref();
        let loading = live.is_none();
        let enabled = !self.pending;
        let on_off = |on: bool| if on { "On" } else { "Off" };
        let mut tiles = Vec::new();
        let wifi = self.radio(RadioKind::Wifi);
        let wifi_detail = match wifi {
            None if loading => "…".to_owned(),
            None => "Unavailable".to_owned(),
            Some(true) => self
                .network
                .name
                .clone()
                .filter(|_| self.network.kind == Link::Wifi)
                .unwrap_or_else(|| "On".into()),
            Some(false) => "Off".into(),
        };
        tiles.push((
            Tile {
                id: WIFI,
                icon: Icon::Wifi(3),
                title: "Wi-Fi",
                subtitle: "",
                on: wifi == Some(true),
                enabled: wifi.is_some() && enabled,
                more: Some(WIFI_MORE),
            },
            wifi_detail,
        ));
        let bluetooth = self.radio(RadioKind::Bluetooth);
        tiles.push((
            Tile {
                id: BLUETOOTH,
                icon: Icon::Bluetooth,
                title: "Bluetooth",
                subtitle: "",
                on: bluetooth == Some(true),
                enabled: bluetooth.is_some() && enabled,
                more: Some(BLUETOOTH_MORE),
            },
            match bluetooth {
                None if loading => "…".into(),
                None => "Unavailable".into(),
                Some(on) => on_off(on).into(),
            },
        ));
        let mut toggle = |id, icon, title, state: Option<bool>| {
            if let Some(on) = state {
                tiles.push((
                    Tile {
                        id,
                        icon,
                        title,
                        subtitle: "",
                        on,
                        enabled,
                        more: None,
                    },
                    on_off(on).into(),
                ));
            }
        };
        toggle(DND, Icon::Moon, "Do Not Disturb", live.and_then(|l| l.dnd));
        toggle(
            NIGHT_LIGHT,
            Icon::NightLight,
            "Night Light",
            live.and_then(|l| l.night_light),
        );
        toggle(
            AIRPLANE,
            Icon::Airplane,
            "Airplane Mode",
            live.and_then(|l| l.airplane),
        );
        toggle(
            HOTSPOT,
            Icon::Hotspot,
            "Hotspot",
            live.and_then(|l| l.hotspot),
        );
        let power = live.and_then(|l| l.power);
        tiles.push((
            Tile {
                id: POWER,
                icon: Icon::Gauge,
                title: "Power Mode",
                subtitle: "",
                on: power.is_some_and(|p| p != PowerMode::Balanced),
                enabled: power.is_some() && enabled,
                more: None,
            },
            match power {
                None if loading => "…".into(),
                None => "Unavailable".into(),
                Some(mode) => power_label(mode).into(),
            },
        ));
        tiles
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
        360.0
    }

    fn view(&self, kit: &mut Kit) {
        let tiles = self.tiles();
        let brightness = self.brightness.or(self.display);
        let volume = self.volume;
        let battery = self.battery;
        let error = self.error.as_deref();
        kit.frame(|f, area| {
            let c = f.theme.color;
            let gap = f.px(GAP);
            let mut column = Stack::column(area, gap);
            // The tiles, two to a row.
            let height = f.px(TILE);
            let rows = tiles.len().div_ceil(2);
            let grid = column.next(rows as f32 * height + rows.saturating_sub(1) as f32 * gap);
            for (i, (tile, subtitle)) in tiles.iter().enumerate() {
                let tile = Tile {
                    subtitle: subtitle.as_str(),
                    ..tile.clone()
                };
                f.tile(&tile, layout::grid(grid, 2, gap, height, i));
            }
            // Display and Sound, each a card with a Control Center slider.
            let card = slider_card_height(f);
            if let Some(level) = brightness {
                slider_card(
                    f,
                    column.next(card),
                    "Display",
                    &[],
                    (BRIGHTNESS, level as f32 / 100.0, "Brightness", Icon::Brightness),
                );
            }
            if let Some(volume) = volume {
                let level = if volume.muted { 0.0 } else { volume.level };
                let icon = volume_icon(level, volume.muted);
                let mute = if volume.muted { "Unmute" } else { "Mute" };
                slider_card(
                    f,
                    column.next(card),
                    "Sound",
                    &[
                        (MUTE, if volume.muted { Icon::Mute } else { Icon::Speaker }, mute),
                        (MIXER, Icon::ChevronRight, "Volume mixer"),
                    ],
                    (VOLUME, level, "Volume", icon),
                );
            }
            if let Some(error) = error {
                let width = column.rect.width();
                let lines = f.line_count(error, TextRole::Footnote, width, 2);
                let at = column.next(lines as f32 * f.line(TextRole::Footnote));
                f.paragraph(at.left, at.top, width, error, TextRole::Footnote, c.secondary, 2);
            }
            // The foot: the battery, and all of Settings.
            let foot = column.next(f.px(28.0));
            let (button, rest) = foot.take_right(f.px(28.0), f.px(8.0));
            f.icon_button(SETTINGS, button, Icon::Settings, 28.0, "All settings", false);
            if let Some(b) = battery {
                let text = format!(
                    "{}%{}",
                    b.percent,
                    if b.charging { " · Charging" } else { "" }
                );
                let (icon, text_rect) = rest.take_left(f.px(20.0), f.px(6.0));
                let level = if b.charging {
                    Icon::Charging
                } else {
                    Icon::Battery((b.percent / 10).min(10) as u8)
                };
                f.icon(icon, level, 16.0, c.secondary);
                f.text_in(text_rect, &text, TextRole::Callout, c.secondary, Align::Start);
            }
            area.top + column.used()
        });
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.error = None;
        let live = self.live.as_ref();
        let (dnd, night, airplane, hotspot) = (
            live.and_then(|l| l.dnd),
            live.and_then(|l| l.night_light),
            live.and_then(|l| l.airplane),
            live.and_then(|l| l.hotspot),
        );
        match id {
            WIFI => self.set_radio(RadioKind::Wifi, cx),
            WIFI_MORE => Effect::Switch(Kind::Wifi),
            BLUETOOTH => self.set_radio(RadioKind::Bluetooth, cx),
            BLUETOOTH_MORE => settings("ms-settings:bluetooth"),
            DND => self.flip(Toggle::Dnd, dnd, cx),
            NIGHT_LIGHT => self.flip(Toggle::NightLight, night, cx),
            AIRPLANE => self.flip(Toggle::Airplane, airplane, cx),
            POWER => {
                if self.pending || self.refreshing {
                    return Effect::None;
                }
                let Some(mode) = live.and_then(|l| l.power) else {
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
                let Some(on) = hotspot.filter(|_| !self.pending && !self.refreshing) else {
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
                // Only the display comes from here; the switches from `QuickControl`.
                let changed = self.brightness.is_some() || self.display != state.brightness;
                self.display = state.brightness;
                self.brightness = None;
                changed
            }
            Reply::QuickControl(live, error) => {
                let changed =
                    self.pending || error.is_some() || self.live.as_ref() != Some(live);
                self.pending = false;
                self.refreshing = false;
                if error.is_some() {
                    self.error = error.clone();
                }
                self.live = Some(live.clone());
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
            DND => settings("ms-settings:notifications"),
            NIGHT_LIGHT => settings("ms-settings:nightlight"),
            AIRPLANE => settings("ms-settings:network-airplanemode"),
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
