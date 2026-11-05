//! Grok Bot: local discovery, supported routine triggers, receipts and native prompt handoff.
use super::t3::{chrome, connection, number, prompt_field};
use super::*;
use crate::integrations::{
    grok::{OPEN, PROMPT},
    widgets::RUN,
};
use mypaper_integrations::launcher::RoutineState;
use mypaper_ui::kit::{Accessory, Lead, Row};

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    m.integration.grok.targets.borrow_mut().clear();
    let variant = m.str("variant", "routines");
    let title = match variant.as_str() {
        "prompt" => "Grok · Prompt",
        "recent" => "Grok · Recent",
        _ => "Grok · Routines",
    };
    let l = chrome(f, rect, title, Icon::Star, true);
    let state = &m.integration;
    let discovery = &state.grok.discovery;
    if !discovery.installed && state.routines.is_empty() {
        super::t3::connect_face(
            f,
            m,
            &l,
            "Connect Grok Bot",
            "Detects Grok Bot on this PC. Sign in once in Grok; your account stays there.",
            Icon::Star,
        );
    } else if variant == "prompt" {
        let (button, body) = l.body.take_bottom(f.px(30.0), f.px(7.0));
        if !l.compact {
            let field = body.place(body.width(), f.px(35.0), Align::Start, Align::Start);
            prompt_field(f, m, field, "Ask your Bot…");
            if body.height() / f.s > 75.0 {
                f.paragraph(
                    body.left,
                    body.top + f.px(46.0),
                    body.width(),
                    "Opens a draft in Grok Bot for you to review and send.",
                    TextRole::Caption,
                    f.theme.color.secondary,
                    2,
                );
            }
        } else if m.field.is_some() {
            prompt_field(f, m, body, "Ask your Bot…");
        } else {
            f.button(
                crate::widgets::views::FIELD,
                body,
                "Write a prompt",
                Some(Icon::Edit),
                ButtonKind::Plain,
                discovery.installed,
            );
        }
        f.button(
            PROMPT,
            button,
            "Open prompt",
            Some(Icon::External),
            ButtonKind::Tinted,
            discovery.installed,
        );
    } else if variant == "recent" {
        if l.compact {
            metric(
                f,
                l.body,
                &number(state.runs.len() as u64),
                "requests this session",
                f.theme.color.label,
            );
        } else if state.runs.is_empty() && state.grok.cards.is_empty() {
            f.empty(l.body,Some(Icon::Clock),"No recent requests",Some("Routine request receipts and Grok notifications appear here. Results open in Grok Bot."));
        } else {
            let row_h = f.px(48.0);
            let count = (l.body.height() / row_h).floor().max(0.0) as usize;
            for (i, run) in state.runs.iter().take(count).enumerate() {
                let subtitle = match run.state {
                    RoutineState::Sending => "Sending…",
                    RoutineState::Accepted => "Accepted · open Grok for results",
                    RoutineState::Failed => "Request failed · retry routine",
                };
                f.row(
                    &Row {
                        id: OPEN,
                        lead: Lead::Icon(match run.state {
                            RoutineState::Sending => Icon::Upload,
                            RoutineState::Accepted => Icon::Check,
                            RoutineState::Failed => Icon::Warning,
                        }),
                        title: &run.title,
                        subtitle: Some(subtitle),
                        accessory: Accessory::Chevron,
                        selected: false,
                        enabled: discovery.installed,
                    },
                    Rect::new(
                        l.body.left,
                        l.body.top + i as f32 * row_h,
                        l.body.width(),
                        row_h - f.px(1.0),
                    ),
                );
            }
            let used = state.runs.len().min(count);
            for (i, card) in state
                .grok
                .cards
                .iter()
                .take(count.saturating_sub(used))
                .enumerate()
            {
                let title = if card.title.is_empty() {
                    "Grok Bot notification"
                } else {
                    &card.title
                };
                f.row(
                    &Row {
                        id: OPEN,
                        lead: Lead::Icon(Icon::Bell),
                        title,
                        subtitle: Some(if card.body.is_empty() {
                            "Open Grok Bot to read it"
                        } else {
                            &card.body
                        }),
                        accessory: Accessory::Chevron,
                        selected: false,
                        enabled: discovery.installed,
                    },
                    Rect::new(
                        l.body.left,
                        l.body.top + (used + i) as f32 * row_h,
                        l.body.width(),
                        row_h - f.px(1.0),
                    ),
                );
            }
        }
    } else if state.routines.is_empty() {
        let (button, body) = l.body.take_bottom(f.px(30.0), f.px(7.0));
        if l.compact {
            f.text_in(
                body,
                if discovery.signed_in {
                    "Signed in"
                } else if discovery.running {
                    "Sign in to Grok"
                } else {
                    "Grok installed"
                },
                TextRole::Headline,
                f.theme.color.label,
                Align::Start,
            );
        } else {
            let (heading, detail) = body.take_top(f.px(23.0), f.px(5.0));
            f.text_in(
                heading,
                if discovery.signed_in {
                    "Grok Bot found"
                } else {
                    "Open Grok Bot"
                },
                TextRole::Headline,
                f.theme.color.label,
                Align::Start,
            );
            f.paragraph(detail.left,detail.top,detail.width(),"Routine discovery isn't available. Add a webhook under Advanced to run a routine here.",TextRole::Callout,f.theme.color.secondary,(detail.height()/f.line(TextRole::Callout)).floor().max(1.0)as usize);
        }
        f.button(
            OPEN,
            button,
            "Open Grok Bot",
            Some(Icon::External),
            ButtonKind::Tinted,
            true,
        );
    } else if l.compact {
        let (button, body) = l.body.take_bottom(f.px(30.0), f.px(5.0));
        f.text_in(
            body,
            &state.routines[0].title,
            TextRole::Headline,
            f.theme.color.label,
            Align::Start,
        );
        let busy = state
            .runs
            .iter()
            .any(|r| r.id == state.routines[0].id && r.state == RoutineState::Sending);
        state
            .grok
            .targets
            .borrow_mut()
            .insert(RUN, state.routines[0].id.clone());
        f.button(
            RUN,
            button,
            if busy { "Sending…" } else { "Run routine" },
            Some(Icon::Play),
            ButtonKind::Tinted,
            !busy && !state.busy(),
        );
    } else {
        let row_h = f.px(49.0);
        let visible = (l.body.height() / row_h).floor().max(1.0) as usize;
        m.scroll_max
            .set(state.routines.len().saturating_sub(visible) as f32 * row_h);
        m.scroll_page.set(l.body.height());
        let first = (m.scroll.max(0.0) / row_h).floor() as usize;
        for (row, (i, routine)) in state
            .routines
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .enumerate()
        {
            let busy = state
                .runs
                .iter()
                .any(|run| run.id == routine.id && run.state == RoutineState::Sending);
            state
                .grok
                .targets
                .borrow_mut()
                .insert(RUN + i as u32, routine.id.clone());
            f.row(
                &Row {
                    id: RUN + i as u32,
                    lead: Lead::Icon(Icon::Play),
                    title: &routine.title,
                    subtitle: Some(if busy {
                        "Sending request…"
                    } else {
                        "Run now"
                    }),
                    accessory: Accessory::Chevron,
                    selected: false,
                    enabled: !busy && !state.busy(),
                },
                Rect::new(
                    l.body.left,
                    l.body.top + row as f32 * row_h,
                    l.body.width(),
                    row_h - f.px(1.0),
                ),
            );
        }
    }
    if let Some(footer) = l.footer {
        connection(
            f,
            m,
            footer,
            if variant == "recent" {
                "Acceptance is a receipt, not completion"
            } else if discovery.signed_in {
                "Signed in · detected on this PC"
            } else if discovery.running {
                "Sign in once in Grok Bot"
            } else {
                "Open Grok Bot to connect"
            },
        );
    }
}
