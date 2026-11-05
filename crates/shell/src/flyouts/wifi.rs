//! Wi-Fi: the radio, the networks in range (signal, security), connecting and disconnecting
//! (the `wifi` lane; its scan notification refreshes the list). A network that needs a
//! password goes to Windows' own prompt, never a field of ours. Windows 11 24H2+ lists
//! networks to desktop apps only with location access on: then it says so and links there.

use mypaper_bars::flyouts::quick::RadioKind;
use mypaper_bars::flyouts::wifi::{Network, Security, WifiError, WifiState};
use mypaper_bars::flyouts::{Done, Watched};
use mypaper_bars::system;
use mypaper_bars::wake::Msg;
use mypaper_ui::{HitId, Point};
use std::time::{Duration, Instant};

use super::kit::{Icon, Kit, Row, glyph};
use super::{Cx, Effect, LIST, Panel, Reply, controls};

const RADIO: HitId = 1;
const SCAN: HitId = 2;
const SETTINGS: HitId = 3;
const LOCATION: HitId = 4;
/// The expanded network's action buttons.
const CONNECT: HitId = 5;
const DISCONNECT: HitId = 6;
const PROMPT: HitId = 7;
const ETHERNET: HitId = 8;
const VPN: HitId = 9;

/// What a network takes to join from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Connected: it can only be left.
    Leave,
    /// A saved profile or an open network: one click.
    Direct,
    /// A password: Windows' own prompt asks for it.
    Prompt,
    /// Enterprise sign-in and the rest: Settings.
    Settings,
}

pub fn join(network: &Network) -> Join {
    match network.security {
        _ if network.connected => Join::Leave,
        _ if network.known => Join::Direct,
        Security::Open => Join::Direct,
        Security::Wpa2 | Security::Wpa3 | Security::Wpa => Join::Prompt,
        Security::Enterprise | Security::Other => Join::Settings,
    }
}

/// 1 to 4 bars for a 0-100 signal quality.
pub fn bars(signal: u32) -> usize {
    match signal {
        0..=24 => 1,
        25..=49 => 2,
        50..=74 => 3,
        _ => 4,
    }
}

pub struct Wifi {
    state: Option<WifiState>,
    radio: Option<bool>,
    expanded: Option<String>,
    status: Option<String>,
    scroll: usize,
    radio_pending: bool,
}

impl Wifi {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes.wifi(|lane| {
            lane.scan();
            Reply::Wifi(lane.read())
        });
        cx.lanes
            .quick(|_| Reply::QuickControl(controls::read(), None));
        Self {
            state: None,
            radio: None,
            expanded: None,
            status: None,
            scroll: 0,
            radio_pending: true,
        }
    }

    #[cfg(test)]
    pub fn with(state: WifiState, radio: Option<bool>, expanded: Option<&str>) -> Self {
        Self {
            state: Some(state),
            radio,
            expanded: expanded.map(Into::into),
            status: None,
            scroll: 0,
            radio_pending: false,
        }
    }

    fn network(&self, id: HitId) -> Option<&Network> {
        let index = id.checked_sub(LIST)? as usize;
        self.state.as_ref()?.networks.get(index)
    }

    fn expanded(&self) -> Option<&Network> {
        let name = self.expanded.as_deref()?;
        self.state
            .as_ref()?
            .networks
            .iter()
            .find(|n| n.ssid == name)
    }
}

fn security(network: &Network) -> &'static str {
    match network.security {
        _ if network.connected => "Connected, secured",
        Security::Open => "Open",
        Security::Wpa3 => "Secured (WPA3)",
        Security::Wpa => "Secured (WPA)",
        Security::Enterprise => "Secured (enterprise)",
        Security::Wpa2 | Security::Other => "Secured",
    }
}

impl Panel for Wifi {
    fn width(&self) -> f32 {
        340.0
    }

    fn view(&self, kit: &mut Kit) {
        let top = kit.y;
        kit.header("Wi-Fi", &[(SCAN, glyph::REFRESH, "Scan again")]);
        if let Some(on) = self.radio {
            let center = top + kit.px(16.0);
            let right = kit.right - kit.px(40.0);
            kit.switch(RADIO, on, right, center, "Wi-Fi");
        }
        match &self.state {
            None => kit.notice("Looking for networks…"),
            Some(WifiState {
                error: Some(WifiError::Location),
                ..
            }) => {
                kit.notice("Windows shows apps Wi-Fi networks only with location access on.");
                kit.buttons(&[(LOCATION, "Location settings", false)]);
            }
            Some(WifiState {
                error: Some(WifiError::NoAdapter),
                ..
            }) => kit.notice("No Wi-Fi adapter"),
            Some(WifiState { error: Some(_), .. }) => kit.notice("Wi-Fi isn't available right now"),
            Some(state) if state.networks.is_empty() => kit.notice(if self.radio == Some(false) {
                "Wi-Fi is off"
            } else {
                "Looking for networks…"
            }),
            Some(state) => {
                let row = kit.px(50.0);
                let actions = if self.expanded.is_some() {
                    kit.px(46.0)
                } else {
                    0.0
                };
                let room = ((kit.limit - kit.y - kit.px(50.0) - actions) / row)
                    .floor()
                    .max(1.0) as usize;
                for (i, network) in state
                    .networks
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(room)
                {
                    kit.row(Row {
                        id: LIST + i as HitId,
                        icon: Icon::Glyph(glyph::WIFI[bars(network.signal) - 1]),
                        title: &network.ssid,
                        detail: Some(security(network)),
                        trailing: (network.security != Security::Open).then_some(glyph::LOCK),
                        selected: network.connected,
                        enabled: true,
                    });
                    if self.expanded.as_deref() == Some(network.ssid.as_str()) {
                        let button = match join(network) {
                            Join::Leave => (DISCONNECT, "Disconnect", false),
                            Join::Direct => (CONNECT, "Connect", true),
                            Join::Prompt => (PROMPT, "Connect…", true),
                            Join::Settings => (PROMPT, "Connect in Settings", false),
                        };
                        if let Some(status) = &self.status {
                            let style = kit.font(12.0, 400);
                            let (left, y, room) =
                                (kit.left, kit.y + kit.px(8.0), kit.width() / 2.0);
                            kit.text_at(left, y, status, &style, kit.look.dim(), room);
                        }
                        kit.buttons(&[button]);
                    }
                }
                if state.networks.len() > room {
                    let more = state.networks.len() - room - self.scroll.min(state.networks.len());
                    if more > 0 {
                        let center = Point::new(kit.left + kit.width() / 2.0, kit.y + kit.px(8.0));
                        let style = kit.font(12.0, 400);
                        kit.text_centered(
                            center,
                            &format!("{more} more — scroll"),
                            &style,
                            kit.look.faint(),
                        );
                        kit.y += kit.px(16.0);
                    }
                }
            }
        }
        kit.footer_link(SETTINGS, "Network & internet settings");
        kit.buttons(&[(ETHERNET, "Ethernet", false), (VPN, "VPN", false)]);
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        if id == ETHERNET || id == VPN {
            system::open(
                if id == ETHERNET {
                    "ms-settings:network-ethernet"
                } else {
                    "ms-settings:network-vpn"
                }
                .into(),
            );
            return Effect::Close;
        }
        match id {
            RADIO => {
                if self.radio_pending {
                    return Effect::None;
                }
                let Some(on) = self.radio else {
                    return Effect::None;
                };
                self.radio_pending = true;
                self.status = None;
                super::quick::change_radio(cx, RadioKind::Wifi, !on);
                Effect::Redraw
            }
            SCAN => {
                cx.lanes.wifi(|lane| {
                    lane.scan();
                    Reply::Wifi(lane.read())
                });
                Effect::None
            }
            SETTINGS => open("ms-settings:network-status"),
            LOCATION => open("ms-settings:privacy-location"),
            CONNECT => {
                let Some(ssid) = self.expanded().map(|n| n.ssid.clone()) else {
                    return Effect::None;
                };
                self.status = Some("Connecting…".into());
                cx.lanes.wifi(move |lane| match lane.connect(&ssid, None) {
                    Ok(()) => Reply::Done,
                    Err(e) => Reply::Failed(e),
                });
                Effect::Redraw
            }
            DISCONNECT => {
                self.status = Some("Disconnecting…".into());
                cx.lanes.wifi(|lane| match lane.disconnect() {
                    Ok(()) => Reply::Done,
                    Err(e) => Reply::Failed(e),
                });
                Effect::Redraw
            }
            PROMPT => match self.expanded().map(join) {
                // Windows' own list asks for the password (and runs enterprise sign-in).
                Some(Join::Prompt) => open("ms-availablenetworks:"),
                _ => open("ms-settings:network-wifi"),
            },
            id => {
                let Some(network) = self.network(id) else {
                    return Effect::None;
                };
                let ssid = network.ssid.clone();
                self.status = None;
                self.expanded = (self.expanded.as_deref() != Some(&ssid)).then_some(ssid);
                Effect::Redraw
            }
        }
    }

    fn wheel(&mut self, _: Option<HitId>, delta: i16, _: &mut Cx) -> Effect {
        let count = self.state.as_ref().map_or(0, |s| s.networks.len());
        let before = self.scroll;
        self.scroll = if delta > 0 {
            self.scroll.saturating_sub(1)
        } else {
            (self.scroll + 1).min(count.saturating_sub(1))
        };
        if before == self.scroll {
            Effect::None
        } else {
            Effect::Redraw
        }
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::Wifi(state) => {
                let changed = self.state.as_ref() != Some(state);
                self.state = Some(state.clone());
                if self.expanded().is_some_and(|n| n.connected) {
                    self.status = None;
                }
                changed
            }
            Reply::Quick(quick) => {
                let radio = quick
                    .radios
                    .iter()
                    .find(|r| r.kind == RadioKind::Wifi)
                    .map(|r| r.on);
                let changed = radio != self.radio;
                self.radio = radio;
                changed
            }
            Reply::QuickControl(live, error) => {
                self.radio_pending = false;
                let radio = live
                    .radios
                    .iter()
                    .filter(|r| r.kind == RadioKind::Wifi)
                    .map(|r| r.on)
                    .reduce(|a, b| a || b);
                let changed = radio != self.radio || error.is_some();
                if radio != self.radio {
                    cx.lanes.wifi(|lane| {
                        lane.scan();
                        Reply::Wifi(lane.read())
                    });
                }
                self.radio = radio;
                if error.is_some() {
                    self.status = error.clone();
                }
                changed
            }
            Reply::Done => {
                cx.lanes.wifi(|lane| Reply::Wifi(lane.read()));
                false
            }
            Reply::Failed(e) => {
                self.status = Some(e.clone());
                true
            }
            _ => false,
        }
    }

    fn changed(&mut self, msg: &Msg, cx: &mut Cx) -> bool {
        if matches!(msg, Msg::Flyout(Done::Changed(Watched::Wifi))) {
            cx.lanes.wifi(|lane| Reply::Wifi(lane.read()));
        }
        false
    }

    fn tick(&mut self, now: Instant, cx: &mut Cx) -> (bool, Option<Instant>) {
        if !self.radio_pending {
            self.radio_pending = true;
            cx.lanes
                .quick(|_| Reply::QuickControl(controls::read(), None));
        }
        (false, Some(now + Duration::from_secs(1)))
    }
}

fn open(uri: &str) -> Effect {
    system::open(uri.into());
    Effect::Close
}
