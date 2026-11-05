//! The mixer: the output devices (switching the default one only with
//! `audio.switchOutput`, which uses an undocumented interface), the master volume, and every
//! app playing on the default output with its own volume and mute (the `mixer` lane, which
//! reports sessions coming, going and changing).
//!
//! On the kit: outputs as rows, the master volume in a Control Center card, the apps in a
//! scroll view (only the apps scroll; the footer stays put).

use std::time::Instant;

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::mixer::{MixerState, Session};
use mypaper_bars::flyouts::{Done, Watched};
use mypaper_bars::system;
use mypaper_bars::wake::Msg;
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{Align, Stack};
use mypaper_ui::{Accessory, Frame, HitId, Lead, ListRow, Op, Rect, Scroll, TextRole};

use super::kit::{Kit, footer, slider_card, slider_card_height, volume_icon};
use super::{Cx, Effect, LIST, Panel, Reply};

const MASTER: HitId = 1;
const MUTE: HitId = 2;
const SETTINGS: HitId = 3;
/// Output devices: `DEVICES + index`.
const DEVICES: HitId = 100;
/// Sessions: a slider at `LIST + 2 * index`, its mute button at `+ 1`.
const SESSIONS: HitId = LIST;

pub struct Mixer {
    state: Option<MixerState>,
    volume: Option<Volume>,
    switchable: bool,
    /// Session icons, decoded.
    icons: Vec<Option<std::sync::Arc<mypaper_ui::Image>>>,
    error: Option<String>,
    dragging: bool,
    pending: Option<MixerState>,
    /// The apps list's scroll position (only it scrolls).
    scroll: Scroll,
}

impl Mixer {
    pub fn new(cx: &mut Cx) -> Self {
        let switchable = cx.look.switch_output;
        cx.lanes
            .mixer(move |lane| Reply::Mixer(lane.read(switchable)));
        Self {
            state: None,
            // The endpoint (COM activation, tens of ms cold) is opened once on screen.
            volume: None,
            switchable,
            icons: Vec::new(),
            error: None,
            dragging: false,
            pending: None,
            scroll: Scroll::new(&cx.look.theme().motion),
        }
    }

    #[cfg(test)]
    pub fn with(state: MixerState, volume: Option<Volume>) -> Self {
        Self {
            icons: vec![None; state.sessions.len()],
            switchable: state.switchable,
            state: Some(state),
            volume,
            error: None,
            dragging: false,
            pending: None,
            scroll: Scroll::default(),
        }
    }

    fn reread(&self, cx: &mut Cx) {
        let switchable = self.switchable;
        cx.lanes
            .mixer(move |lane| Reply::Mixer(lane.read(switchable)));
    }

    fn session(&mut self, id: HitId) -> Option<(usize, &mut Session)> {
        let index = id.checked_sub(SESSIONS)? as usize / 2;
        let session = self.state.as_mut()?.sessions.get_mut(index)?;
        Some((index, session))
    }

    /// One app: its icon and name with a mute button, over its Control Center slider.
    fn app(&self, f: &mut Frame, i: usize, session: &Session, rect: Rect) {
        let c = f.theme.color;
        let slider = SESSIONS + 2 * i as HitId;
        let (head, bar) = rect.take_top(f.px(26.0), f.px(4.0));
        let (mute, head) = head.take_right(f.px(26.0), f.px(4.0));
        let level = if session.muted { 0.0 } else { session.level };
        let label = if session.muted { "Unmute" } else { "Mute" };
        let icon = if session.muted { Icon::Mute } else { Icon::Speaker };
        f.icon_button(slider + 1, mute, icon, 26.0, label, false);
        let size = f.px(18.0);
        let (lead, name) = head.take_left(size, f.px(8.0));
        match self.icons.get(i).cloned().flatten() {
            Some(image) => f.push(Op::Image {
                rect: lead.place(size, size, Align::Center, Align::Center),
                image,
                opacity: 1.0,
                source: None,
            }),
            None => f.icon(lead, Icon::Apps, 15.0, c.secondary),
        }
        let ink = if session.active { c.label } else { c.secondary };
        f.text_in(name, &session.name, TextRole::Callout, ink, Align::Start);
        f.slider(slider, bar, level, &session.name, Some(volume_icon(level, session.muted)));
    }
}

/// An app's block: its name line and its slider, design px.
const APP: f32 = 26.0 + 4.0 + 28.0;
const APP_GAP: f32 = 12.0;

impl Panel for Mixer {
    fn width(&self) -> f32 {
        340.0
    }

    fn view(&self, kit: &mut Kit) {
        kit.frame(|f, area| {
            let mut column = Stack::column(area, f.px(2.0));
            f.section_header(column.next(f.px(24.0)), "Output", None);
            match &self.state {
                None => {
                    let h = f.empty(column.rest(), None, "…", None);
                    column.next(h);
                }
                Some(state) => {
                    for (i, device) in state.devices.iter().enumerate() {
                        let row = ListRow {
                            id: DEVICES + i as HitId,
                            lead: Lead::Badge(Icon::Speaker, device.default),
                            title: &device.name,
                            subtitle: None,
                            accessory: if device.default {
                                Accessory::Check
                            } else {
                                Accessory::None
                            },
                            selected: false,
                            enabled: true,
                        };
                        let h = f.row_height(&row);
                        f.row(&row, column.next(h));
                    }
                }
            }
            column.space(f.px(8.0));
            if let Some(volume) = self.volume {
                let level = if volume.muted { 0.0 } else { volume.level };
                let label = if volume.muted { "Unmute" } else { "Mute" };
                let icon = if volume.muted { Icon::Mute } else { Icon::Speaker };
                let h = slider_card_height(f);
                slider_card(
                    f,
                    column.next(h),
                    "Volume",
                    &[(MUTE, icon, label)],
                    (MASTER, level, "Volume", volume_icon(level, volume.muted)),
                );
            }
            let sessions = self.state.as_ref().map_or(&[][..], |s| &s.sessions[..]);
            // The footer's room is kept; the apps get the rest, in a scroll view.
            let footer_h = f.px(9.0) + f.px(f.theme.metrics.row_single) + f.px(2.0);
            let error_h = self.error.as_ref().map_or(0.0, |_| 2.0 * f.line(TextRole::Footnote));
            if !sessions.is_empty() {
                column.space(f.px(8.0));
                f.section_header(column.next(f.px(24.0)), "Apps", None);
                let (app, gap) = (f.px(APP), f.px(APP_GAP));
                let n = sessions.len() as f32;
                let content = n * app + (n - 1.0) * gap;
                let room = (column.rest().height() - footer_h - error_h - f.px(4.0)).max(app);
                let viewport = column.next(content.min(room));
                let width = viewport.width();
                f.scroll(viewport, &self.scroll, Instant::now(), |f, at| {
                    let mut list = Stack::column(Rect::new(at.x, at.y, width, content), gap);
                    for (i, session) in sessions.iter().enumerate() {
                        self.app(f, i, session, list.next(app));
                    }
                    content
                });
            }
            if let Some(error) = &self.error {
                let at = column.next(error_h);
                let c = f.theme.color;
                f.paragraph(at.left, at.top, at.width(), error, TextRole::Footnote, c.secondary, 2);
            }
            footer(f, &mut column, SETTINGS, "Sound settings");
            area.top + column.used()
        });
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        self.error = None;
        match id {
            MUTE => {
                let muted = self.volume.is_some_and(|v| v.muted);
                self.volume = cx
                    .lanes
                    .audio()
                    .and_then(|a| a.change(None, Some(!muted), None));
                Effect::Redraw
            }
            SETTINGS => {
                system::open("ms-settings:apps-volume".into());
                Effect::Close
            }
            id if (DEVICES..SESSIONS).contains(&id) => {
                let device = self
                    .state
                    .as_ref()
                    .and_then(|s| s.devices.get((id - DEVICES) as usize))
                    .cloned();
                let Some(device) = device.filter(|d| !d.default) else {
                    return Effect::None;
                };
                if !self.switchable {
                    system::open("ms-settings:sound".into());
                    return Effect::Close;
                }
                let switchable = self.switchable;
                cx.lanes
                    .mixer(move |lane| match lane.set_output(&device.id) {
                        Ok(()) => Reply::Mixer(lane.read(switchable)),
                        Err(e) => Reply::Failed(e),
                    });
                Effect::None
            }
            id if id >= SESSIONS && (id - SESSIONS) % 2 == 1 => {
                let Some((_, session)) = self.session(id) else {
                    return Effect::None;
                };
                session.muted = !session.muted;
                let (sid, mute) = (session.id.clone(), session.muted);
                cx.lanes
                    .mixer(move |lane| match lane.set_session(&sid, None, Some(mute)) {
                        Ok(()) => Reply::Done,
                        Err(e) => Reply::Failed(e),
                    });
                Effect::Redraw
            }
            _ => Effect::None,
        }
    }

    fn slide(&mut self, id: HitId, value: f32, done: bool, cx: &mut Cx) -> Effect {
        self.dragging = !done;
        if done && self.pending.take().is_some() {
            self.reread(cx);
        }
        if id == MASTER {
            let unmute = self.volume.is_some_and(|v| v.muted).then_some(false);
            self.volume = cx
                .lanes
                .audio()
                .and_then(|a| a.change(Some(value), unmute, None));
            return Effect::Redraw;
        }
        let Some((_, session)) = self.session(id) else {
            return Effect::None;
        };
        session.level = value;
        let unmute = session.muted.then_some(false);
        session.muted = false;
        let sid = session.id.clone();
        cx.lanes.mixer(
            move |lane| match lane.set_session(&sid, Some(value), unmute) {
                Ok(()) => Reply::Done,
                Err(e) => Reply::Failed(e),
            },
        );
        Effect::Redraw
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

    fn tick(&mut self, now: Instant, _: &mut Cx) -> (bool, Option<Instant>) {
        self.scroll.tick(now)
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::Mixer(state) => {
                if self.dragging {
                    self.pending = Some(state.clone());
                    return false;
                }
                self.icons = state
                    .sessions
                    .iter()
                    .map(|s| cx.image(s.icon.as_deref()))
                    .collect();
                let changed = self.state.as_ref() != Some(state);
                self.state = Some(state.clone());
                changed
            }
            Reply::Failed(e) => {
                log::warn!("mixer: {e}");
                self.error = Some(e.clone());
                self.reread(cx);
                true
            }
            _ => false,
        }
    }

    fn opened(&mut self, cx: &mut Cx) -> bool {
        self.volume = cx.lanes.audio().and_then(|a| a.read());
        self.volume.is_some()
    }

    fn changed(&mut self, msg: &Msg, cx: &mut Cx) -> bool {
        match msg {
            Msg::Flyout(Done::Changed(Watched::Mixer)) => {
                self.reread(cx);
                false
            }
            Msg::Volume => {
                if let Some(a) = cx.lanes.audio.as_mut() {
                    a.follow_default();
                    self.volume = a.read();
                }
                self.reread(cx);
                true
            }
            _ => false,
        }
    }
}
