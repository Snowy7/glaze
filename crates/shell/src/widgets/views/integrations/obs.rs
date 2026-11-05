use super::*;
use crate::integrations::widgets::{MIC, PRIMARY, SCENE, SECONDARY};
use mypaper_integrations::Data;
use mypaper_ui::kit::{Accessory, Lead, Row};

fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn clock(f: &mut Frame<'_>, rect: Rect, title: &str, time: &str, active: bool) {
    let (label, rest) = rect.take_top(f.px(19.0), f.px(2.0));
    pill(
        f,
        label,
        title,
        if active {
            f.theme.color.destructive
        } else {
            f.theme.color.tertiary
        },
    );
    f.text_in(
        rest,
        time,
        TextRole::Title1,
        if active {
            f.theme.color.label
        } else {
            f.theme.color.secondary
        },
        Align::Start,
    );
}

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, now_ms: i64) {
    let l = layout(f, rect, "OBS Studio", Icon::Camera, true);
    let Some(Data::Obs(state)) = &m.integration.snapshot.data else {
        setup(
            f,
            m,
            l,
            "Connect OBS",
            "Enable OBS WebSocket and save its password in Settings. Your scenes and microphones appear here.",
            Icon::Camera,
        );
        return;
    };
    let enabled = m.integration.actionable();
    // A disconnected clock must stop advancing while its retained state stays visible.
    let at = if m.integration.snapshot.availability == Availability::Live {
        now_ms
    } else {
        m.integration.observed_at_ms
    };
    let record = duration(state.record_clock.elapsed(at));
    let stream_at = if m.integration.snapshot.availability == Availability::Live {
        now_ms
    } else {
        m.integration.observed_at_ms
    };
    let stream = duration(state.stream_clock.elapsed(stream_at));
    let (controls, content) = l.body.take_bottom(f.px(30.0), f.px(8.0));
    let (record_button, stream_button) =
        controls.take_left((controls.width() - f.px(6.0)) * 0.5, f.px(6.0));
    f.button(
        PRIMARY,
        record_button,
        if state.recording { "Stop" } else { "Record" },
        Some(Icon::Camera),
        if state.recording {
            ButtonKind::Destructive
        } else {
            ButtonKind::Gray
        },
        enabled,
    );
    f.button(
        SECONDARY,
        stream_button,
        if state.streaming { "End" } else { "Stream" },
        Some(Icon::Cast),
        if state.streaming {
            ButtonKind::Destructive
        } else {
            ButtonKind::Gray
        },
        enabled,
    );
    if l.compact {
        clock(
            f,
            content,
            if state.paused {
                "PAUSED"
            } else if state.recording {
                "REC"
            } else if state.streaming {
                "LIVE"
            } else {
                "STANDBY"
            },
            if !state.recording && state.streaming {
                &stream
            } else {
                &record
            },
            state.recording || state.streaming,
        );
    } else if !l.large {
        let (timer, details) = content.take_left(f.px(132.0), f.px(10.0));
        clock(
            f,
            timer,
            if state.paused {
                "PAUSED"
            } else if state.recording {
                "REC"
            } else if state.streaming {
                "LIVE"
            } else {
                "STANDBY"
            },
            if !state.recording && state.streaming {
                &stream
            } else {
                &record
            },
            state.recording || state.streaming,
        );
        let (scene, rest) = details.take_top(f.px(18.0), f.px(1.0));
        f.text_in(
            scene,
            &state.scene,
            TextRole::Headline,
            f.theme.color.label,
            Align::Start,
        );
        let (stream_state, mic) = rest.take_top(f.px(17.0), f.px(1.0));
        f.text_in(
            stream_state,
            &if state.streaming {
                format!("Live · {stream}")
            } else {
                "Stream offline".into()
            },
            TextRole::Caption,
            if state.streaming {
                f.theme.color.destructive
            } else {
                f.theme.color.tertiary
            },
            Align::Start,
        );
        if let Some(input) = state.microphones.first() {
            f.button(
                MIC,
                mic,
                if input.muted { "Mic muted" } else { "Mic on" },
                Some(if input.muted { Icon::Mute } else { Icon::Mic }),
                ButtonKind::Plain,
                enabled,
            );
        }
    } else {
        let (clocks, details) = content.take_top(f.px(54.0), f.px(10.0));
        let (record_rect, stream_rect) =
            clocks.take_left((clocks.width() - f.px(12.0)) * 0.5, f.px(12.0));
        clock(
            f,
            record_rect,
            if state.paused { "PAUSED" } else { "RECORDING" },
            &record,
            state.recording,
        );
        clock(f, stream_rect, "STREAMING", &stream, state.streaming);
        let mic_height = if state.microphones.is_empty() {
            0.0
        } else {
            f.px(37.0)
        };
        let (microphones, scenes) = details.take_bottom(mic_height, f.px(5.0));
        let count = (scenes.height() / f.px(36.0)).floor().max(0.0) as usize;
        for (index, name) in state.scenes.iter().enumerate().take(count) {
            f.row(
                &Row {
                    id: SCENE + index as u32,
                    lead: Lead::Icon(Icon::Window),
                    title: name,
                    subtitle: None,
                    accessory: if *name == state.scene {
                        Accessory::Check
                    } else {
                        Accessory::None
                    },
                    selected: *name == state.scene,
                    enabled,
                },
                Rect::new(
                    scenes.left,
                    scenes.top + index as f32 * f.px(36.0),
                    scenes.width(),
                    f.px(35.0),
                ),
            );
        }
        if let Some(input) = state.microphones.first() {
            f.button(
                MIC,
                microphones,
                &format!(
                    "{} · {}",
                    input.name,
                    if input.muted { "muted" } else { "on" }
                ),
                Some(Icon::Mic),
                if input.muted {
                    ButtonKind::Tinted
                } else {
                    ButtonKind::Gray
                },
                enabled,
            );
        }
    }
    footer(f, m, l.footer, &state.scene);
}
