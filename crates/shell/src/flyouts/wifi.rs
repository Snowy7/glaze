//! Wi-Fi: the radio, the networks in range (signal, security), connecting and disconnecting
//! (the `wifi` lane; its scan notification refreshes the list). A network that needs a
//! password goes to Windows' own prompt, never a field of ours. Windows 11 24H2+ lists
//! networks to desktop apps only with location access on: then it says so and links there.

use glaze_bars::flyouts::quick::RadioKind;
use glaze_bars::flyouts::wifi::{Network, Security, WifiError, WifiState};
use glaze_bars::flyouts::{Done, Watched};
use glaze_bars::system;
use glaze_bars::wake::Msg;
use glaze_ui::icons::Icon;
use glaze_ui::layout::{Align, Stack};
use glaze_ui::{Accessory, ButtonKind, Frame, HitId, Lead, ListRow, Rect, Scroll, TextRole};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the open panel reads the radio again (a change is read back at once).
const POLL: Duration = Duration::from_secs(5);

use super::kit::{Kit, footer};
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

thread_local! {
    /// The networks and the radio when the panel last closed (the UI thread's).
    static LAST: std::cell::RefCell<(Option<WifiState>, Option<bool>)> =
        const { std::cell::RefCell::new((None, None)) };
}

impl Drop for Wifi {
    fn drop(&mut self) {
        let last = (self.state.take(), self.radio);
        LAST.with_borrow_mut(|l| *l = last);
    }
}

pub struct Wifi {
    state: Option<WifiState>,
    radio: Option<bool>,
    expanded: Option<String>,
    status: Option<String>,
    /// The list's scroll position (only the networks scroll).
    scroll: Scroll,
    /// Quick-lane reads of the radio in flight, oldest first: `true` for a change (see `quick`).
    jobs: VecDeque<bool>,
    /// When the radio is read again.
    poll_due: Instant,
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
            // What was last shown, so a reopen keeps its layout (see `quick`).
            state: LAST.with_borrow(|l| l.0.clone()),
            radio: LAST.with_borrow(|l| l.1),
            expanded: None,
            status: None,
            scroll: Scroll::new(&cx.look.theme().motion),
            jobs: VecDeque::from([false]),
            poll_due: Instant::now() + POLL,
        }
    }

    #[cfg(test)]
    pub fn with(state: WifiState, radio: Option<bool>, expanded: Option<&str>) -> Self {
        Self {
            state: Some(state),
            radio,
            expanded: expanded.map(Into::into),
            status: None,
            scroll: Scroll::default(),
            jobs: VecDeque::new(),
            poll_due: Instant::now() + POLL,
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

    /// The expanded network's line under its row: what is happening, and its one action.
    fn actions(&self, f: &mut Frame, network: &Network, rect: Rect) {
        let c = f.theme.color;
        let (id, label, kind) = match join(network) {
            Join::Leave => (DISCONNECT, "Disconnect", ButtonKind::Gray),
            Join::Direct => (CONNECT, "Connect", ButtonKind::Filled),
            Join::Prompt => (PROMPT, "Connect…", ButtonKind::Filled),
            Join::Settings => (PROMPT, "Connect in Settings", ButtonKind::Gray),
        };
        let (w, h) = f.button_size(label, false);
        let (button, status) = rect.take_right(w, f.px(8.0));
        let button = button.place(w, h, Align::End, Align::Start);
        f.button(id, button, label, None, kind, true);
        if let Some(text) = &self.status {
            let line = Rect { bottom: button.bottom, ..status };
            f.text_in(line, text, TextRole::Footnote, c.secondary, Align::Start);
        }
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
        kit.frame(|f, area| {
            let c = f.theme.color;
            let mut column = Stack::column(area, f.px(6.0));
            // The header: the title, the radio's switch and "scan again".
            let head = column.next(f.px(32.0));
            let (scan, title) = head.take_right(f.px(30.0), f.px(4.0));
            f.icon_button(SCAN, scan, Icon::Refresh, 30.0, "Scan again", false);
            let title = match self.radio {
                Some(on) => {
                    let mid = title.top + title.height() / 2.0;
                    let switch = f.switch(RADIO, title.right, mid, on, "Wi-Fi", !self.jobs.contains(&true));
                    Rect { right: switch.left - f.px(8.0), ..title }
                }
                None => title,
            };
            f.text_in(title, "Wi-Fi", TextRole::Title2, c.label, Align::Start);
            // The footer's room is kept: a separator, its row, and the two buttons.
            let footer_h = f.px(9.0)
                + f.px(f.theme.metrics.row_single)
                + f.px(f.theme.metrics.control)
                + 3.0 * f.px(6.0);
            let notice = |f: &mut Frame, column: &mut Stack, icon, title: &str, detail| {
                let rect = column.rest();
                let h = f.empty(rect, Some(icon), title, detail);
                column.next(h);
            };
            match &self.state {
                None => notice(f, &mut column, Icon::Wifi(3), "Looking for networks…", None),
                Some(WifiState {
                    error: Some(WifiError::Location),
                    ..
                }) => {
                    notice(
                        f,
                        &mut column,
                        Icon::Location,
                        "Location access is off",
                        Some("Windows shows apps Wi-Fi networks only with location access on."),
                    );
                    let row = column.next(f.px(f.theme.metrics.control));
                    let (w, _) = f.button_size("Location settings", false);
                    let r = row.place(w, row.height(), Align::Center, Align::Center);
                    f.button(LOCATION, r, "Location settings", None, ButtonKind::Gray, true);
                }
                Some(WifiState {
                    error: Some(WifiError::NoAdapter),
                    ..
                }) => notice(f, &mut column, Icon::Offline, "No Wi-Fi adapter", None),
                Some(WifiState { error: Some(_), .. }) => notice(
                    f,
                    &mut column,
                    Icon::Offline,
                    "Wi-Fi isn't available right now",
                    None,
                ),
                Some(state) if state.networks.is_empty() => {
                    let (icon, title) = if self.radio == Some(false) {
                        (Icon::Offline, "Wi-Fi is off")
                    } else {
                        (Icon::Wifi(3), "Looking for networks…")
                    };
                    notice(f, &mut column, icon, title, None)
                }
                Some(state) => {
                    // The networks scroll in a viewport sized by the list, never by where it
                    // is scrolled to.
                    let width = column.rect.width();
                    let actions = f.px(f.theme.metrics.control) + f.px(8.0);
                    let rows: Vec<f32> = state
                        .networks
                        .iter()
                        .map(|n| {
                            let open = self.expanded.as_deref() == Some(n.ssid.as_str());
                            f.px(f.theme.metrics.row) + if open { actions } else { 0.0 }
                        })
                        .collect();
                    let content = rows.iter().sum::<f32>();
                    let room = (column.rest().height() - footer_h).max(f.px(f.theme.metrics.row));
                    let viewport = column.next(content.min(room));
                    f.scroll(viewport, &self.scroll, Instant::now(), |f, at| {
                        let mut list = Stack::column(Rect::new(at.x, at.y, width, content), 0.0);
                        for (i, network) in state.networks.iter().enumerate() {
                            let mut rect = list.next(rows[i]);
                            let open = self.expanded.as_deref() == Some(network.ssid.as_str());
                            if open {
                                let (row, below) = rect.take_top(f.px(f.theme.metrics.row), f.px(4.0));
                                self.actions(f, network, below);
                                rect = row;
                            }
                            let row = ListRow {
                                id: LIST + i as HitId,
                                lead: Lead::Badge(Icon::Wifi(bars(network.signal) as u8 - 1), network.connected),
                                title: &network.ssid,
                                subtitle: Some(security(network)),
                                accessory: if network.connected {
                                    Accessory::Check
                                } else {
                                    Accessory::None
                                },
                                selected: false,
                                enabled: true,
                            };
                            f.row(&row, rect);
                        }
                        content
                    });
                }
            }
            footer(f, &mut column, SETTINGS, "Network & internet settings");
            let buttons = column.next(f.px(f.theme.metrics.control));
            let gap = f.px(8.0);
            let w = (buttons.width() - gap) / 2.0;
            f.button(ETHERNET, Rect { right: buttons.left + w, ..buttons }, "Ethernet", Some(Icon::Ethernet), ButtonKind::Gray, true);
            f.button(VPN, Rect { left: buttons.right - w, ..buttons }, "VPN", Some(Icon::Lock), ButtonKind::Gray, true);
            area.top + column.used()
        });
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
                if self.jobs.contains(&true) {
                    return Effect::None;
                }
                let Some(on) = self.radio else {
                    return Effect::None;
                };
                self.jobs.push_back(true);
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

    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        if self.scroll.wheel(delta, cx.scale, Instant::now()) {
            Effect::Redraw
        } else {
            Effect::None
        }
    }

    fn key(&mut self, key: u16, _: &mut Cx) -> Option<Effect> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_NEXT, VK_PRIOR};
        if key == VK_NEXT.0 || key == VK_PRIOR.0 {
            self.scroll.page(key == VK_NEXT.0, Instant::now());
            return Some(Effect::Redraw);
        }
        None
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
                self.jobs.pop_front();
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
        // The radio every few seconds; the scroll's glide and fade at frame rate while they run.
        if now >= self.poll_due {
            self.poll_due = now + POLL;
            if self.jobs.is_empty() {
                self.jobs.push_back(false);
                cx.lanes
                    .quick(|_| Reply::QuickControl(controls::read(), None));
            }
        }
        let (scrolled, next) = self.scroll.tick(now);
        (scrolled, Some(next.map_or(self.poll_due, |n| n.min(self.poll_due))))
    }
}

fn open(uri: &str) -> Effect {
    system::open(uri.into());
    Effect::Close
}
