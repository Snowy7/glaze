//! The mixer: the output devices (switching the default one only with
//! `audio.switchOutput`, which uses an undocumented interface), the master volume, and every
//! app playing on the default output with its own volume and mute (the `mixer` lane, which
//! reports sessions coming, going and changing).

use mypaper_bars::audio::Volume;
use mypaper_bars::flyouts::mixer::{MixerState, Session};
use mypaper_bars::flyouts::{Done, Watched};
use mypaper_bars::system;
use mypaper_bars::wake::Msg;
use mypaper_ui::{HitId, Point, Rect};

use super::kit::{Icon, Kit, Row, glyph};
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
}

impl Mixer {
    pub fn new(cx: &mut Cx) -> Self {
        let switchable = cx.look.switch_output;
        cx.lanes
            .mixer(move |lane| Reply::Mixer(lane.read(switchable)));
        Self {
            state: None,
            volume: cx.lanes.audio().and_then(|a| a.read()),
            switchable,
            icons: Vec::new(),
            error: None,
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
}

fn volume_glyph(level: f32, muted: bool) -> char {
    if muted || level == 0.0 {
        glyph::MUTE
    } else {
        glyph::VOLUME[((level * 3.0).ceil() as usize).min(3)]
    }
}

impl Panel for Mixer {
    fn width(&self) -> f32 {
        340.0
    }

    fn view(&self, kit: &mut Kit) {
        kit.caption("Output");
        match &self.state {
            None => kit.notice("…"),
            Some(state) => {
                for (i, device) in state.devices.iter().enumerate() {
                    kit.row(Row {
                        id: DEVICES + i as HitId,
                        icon: Icon::Glyph(glyph::SPEAKERS),
                        title: &device.name,
                        detail: None,
                        trailing: device.default.then_some(glyph::CHECK),
                        selected: device.default,
                        enabled: true,
                    });
                }
            }
        }
        if let Some(volume) = self.volume {
            let level = if volume.muted { 0.0 } else { volume.level };
            kit.slider(
                MASTER,
                level,
                "Volume",
                Some((MUTE, volume_glyph(level, volume.muted), "Mute")),
                None,
            );
        }
        let sessions = self.state.as_ref().map_or(&[][..], |s| &s.sessions[..]);
        if !sessions.is_empty() {
            kit.separator();
            kit.caption("Apps");
        }
        let row_height = kit.px(58.0);
        let room = ((kit.limit - kit.y - kit.px(60.0)) / row_height)
            .floor()
            .max(1.0) as usize;
        for (i, session) in sessions.iter().take(room).enumerate() {
            let slider = SESSIONS + 2 * i as HitId;
            let style = kit.font(13.0, 400);
            let icon = kit.px(20.0);
            let top = kit.y;
            match self.icons.get(i).cloned().flatten() {
                Some(image) => kit.image(Rect::new(kit.left, top + kit.px(2.0), icon, icon), image),
                None => kit.glyph(
                    Point::new(kit.left + icon / 2.0, top + kit.px(12.0)),
                    glyph::APP,
                    14.0,
                    kit.look.dim(),
                ),
            }
            let ink = if session.active {
                kit.look.ink
            } else {
                kit.look.dim()
            };
            let (left, room) = (
                kit.left + icon + kit.px(10.0),
                kit.width() - icon - kit.px(10.0),
            );
            kit.text_at(left, top + kit.px(3.0), &session.name, &style, ink, room);
            kit.y += kit.px(24.0);
            let level = if session.muted { 0.0 } else { session.level };
            kit.slider(
                slider,
                level,
                &session.name,
                Some((slider + 1, volume_glyph(level, session.muted), "Mute")),
                None,
            );
        }
        if let Some(error) = &self.error {
            kit.paragraph(error, 2, kit.look.dim());
        }
        kit.footer_link(SETTINGS, "Sound settings");
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

    fn slide(&mut self, id: HitId, value: f32, _: bool, cx: &mut Cx) -> Effect {
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

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::Mixer(state) => {
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
