use super::*;
use crate::integrations::widgets::RUN;
use mypaper_integrations::launcher::RoutineState;
use mypaper_ui::kit::{Accessory, Lead, Row};

fn status(state: RoutineState) -> &'static str {
    match state {
        RoutineState::Sending => "Sending…",
        RoutineState::Accepted => "Accepted by Grok Bot",
        RoutineState::Failed => "Request failed",
    }
}

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let l = layout(f, rect, "Grok Bot", Icon::Star, false);
    let state = &m.integration;
    if state.routines.is_empty() {
        setup(
            f,
            m,
            l,
            "Connect Grok Bot",
            "Add a saved routine and its webhook key in Settings. Keys stay in Windows Credential Manager.",
            Icon::Star,
        );
        return;
    }
    if l.compact {
        let (run, summary) = l.body.take_bottom(f.px(30.0), f.px(7.0));
        let routine = &state.routines[0];
        let sending = state
            .runs
            .iter()
            .any(|run| run.id == routine.id && run.state == RoutineState::Sending);
        f.text_in(
            summary,
            &routine.title,
            TextRole::Headline,
            f.theme.color.label,
            Align::Start,
        );
        f.button(
            RUN,
            run,
            if sending { "Sending…" } else { "Run routine" },
            Some(Icon::Play),
            ButtonKind::Tinted,
            !sending,
        );
    } else {
        let mut rows = l.body;
        if l.large {
            let (summary, rest) = rows.take_top(f.px(53.0), f.px(8.0));
            metric(
                f,
                summary,
                &state.routines.len().to_string(),
                "saved routines",
                f.theme.color.label,
            );
            rows = rest;
        }
        let count = if l.large { 3 } else { 2 };
        for (index, routine) in state.routines.iter().enumerate().take(count) {
            let sending = state
                .runs
                .iter()
                .any(|run| run.id == routine.id && run.state == RoutineState::Sending);
            f.row(
                &Row {
                    id: RUN + index as u32,
                    lead: Lead::Icon(Icon::Star),
                    title: &routine.title,
                    subtitle: Some(if sending {
                        "Sending request…"
                    } else {
                        "Start saved routine"
                    }),
                    accessory: Accessory::Chevron,
                    selected: false,
                    enabled: !sending,
                },
                Rect::new(
                    rows.left,
                    rows.top + index as f32 * f.px(44.0),
                    rows.width(),
                    f.px(43.0),
                ),
            );
        }
        if l.large {
            let top = rows.top + state.routines.len().min(count) as f32 * f.px(44.0) + f.px(6.0);
            let room = Rect { top, ..rows };
            for (index, run) in state
                .runs
                .iter()
                .take((room.height() / f.px(37.0)).floor().max(0.0) as usize)
                .enumerate()
            {
                f.row(
                    &Row {
                        id: 0,
                        lead: Lead::Icon(match run.state {
                            RoutineState::Accepted => Icon::Check,
                            RoutineState::Failed => Icon::Warning,
                            RoutineState::Sending => Icon::Upload,
                        }),
                        title: &run.title,
                        subtitle: Some(status(run.state)),
                        accessory: Accessory::None,
                        selected: false,
                        enabled: true,
                    },
                    Rect::new(
                        room.left,
                        room.top + index as f32 * f.px(37.0),
                        room.width(),
                        f.px(36.0),
                    ),
                );
            }
        }
    }
    let detail = state
        .runs
        .first()
        .map_or("Runs start only when you click", |run| status(run.state));
    f.text_in(
        l.footer,
        state.notice.as_deref().unwrap_or(detail),
        TextRole::Caption,
        if state.notice.is_some() {
            f.theme.color.destructive
        } else {
            f.theme.color.tertiary
        },
        Align::Start,
    );
}
