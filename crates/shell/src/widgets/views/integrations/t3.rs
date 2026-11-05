//! Four independent T3 faces. Small tiles expose one state; larger tiles reveal controls.
use super::*;
use crate::integrations::{
    t3_widgets::{
        APPROVE, DENY, MODEL, MODEL_CHOICE, PROJECT, PROJECT_CHOICE, Picker, REAUTHORIZE, SEND,
        THREAD,
    },
    widgets::{CONNECT, REFRESH},
};
use mypaper_integrations::{
    Data,
    t3::{Snapshot, Status, Thread},
};
use mypaper_ui::{
    Op,
    kit::{Accessory, Lead, Row},
    layout::SizeClass,
};

pub(super) struct Areas {
    pub body: Rect,
    pub footer: Option<Rect>,
    pub compact: bool,
}

pub(super) fn chrome(
    f: &mut Frame<'_>,
    rect: Rect,
    title: &str,
    icon: Icon,
    refresh: bool,
) -> Areas {
    let compact = SizeClass::of(rect.width() / f.s, rect.height() / f.s) == SizeClass::Compact;
    let inner = rect.inset(Insets::all(f.px(if compact { 12.0 } else { 16.0 })));
    let (header, mut body) = inner.take_top(f.px(22.0), f.px(8.0));
    let (symbol, label) = header.take_left(f.px(17.0), f.px(6.0));
    f.icon(symbol, icon, 16.0, f.theme.color.accent);
    let (control, label) = if refresh {
        label.take_right(f.px(22.0), f.px(3.0))
    } else {
        (Rect::default(), label)
    };
    f.text_in(
        label,
        title,
        TextRole::Subheadline,
        f.theme.color.secondary,
        Align::Start,
    );
    if refresh {
        f.icon_button(
            REFRESH,
            control,
            Icon::Refresh,
            15.0,
            "Retry or refresh",
            false,
        );
    }
    let footer = if rect.height() / f.s >= 170.0 {
        let (footer, rest) = body.take_bottom(f.px(16.0), f.px(6.0));
        body = rest;
        Some(footer)
    } else {
        None
    };
    Areas {
        body,
        footer,
        compact: compact || body.width() / f.s < 190.0 || body.height() / f.s < 70.0,
    }
}

pub(super) fn connection(f: &mut Frame<'_>, m: &Model, area: Rect, normal: &str) {
    let text = if m.integration.busy() {
        "Working…"
    } else if m.integration.notice.is_some() || m.integration.snapshot.action_error.is_some() {
        "Action failed · retry"
    } else {
        match m.integration.snapshot.availability {
            Availability::Live => normal,
            Availability::Connecting => "Connecting…",
            Availability::Cached => "Snapshot · refresh to update",
            Availability::Stale(_) => "Disconnected · tap refresh",
            Availability::Unavailable(_) => "Open the app, then connect",
        }
    };
    f.text_in(
        area,
        text,
        TextRole::Caption,
        f.theme.color.tertiary,
        Align::Start,
    );
}

pub(super) fn connect_face(
    f: &mut Frame<'_>,
    m: &Model,
    l: &Areas,
    title: &str,
    detail: &str,
    icon: Icon,
) {
    let (button, description) = l.body.take_bottom(f.px(30.0), f.px(7.0));
    if description.height() >= f.line(TextRole::Callout) {
        f.paragraph(
            description.left,
            description.top,
            description.width(),
            detail,
            TextRole::Callout,
            f.theme.color.secondary,
            (description.height() / f.line(TextRole::Callout))
                .floor()
                .max(1.0) as usize,
        );
    }
    f.button(
        CONNECT,
        button,
        if m.integration.busy() {
            "Connecting…"
        } else {
            title
        },
        Some(icon),
        ButtonKind::Tinted,
        !m.integration.busy(),
    );
}

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let face = m.str("variant", "running");
    let title = match face.as_str() {
        "usage" => "T3 · Usage",
        "new" => "T3 · New thread",
        "attention" => "T3 · Needs you",
        _ => "T3 · Running now",
    };
    let l = chrome(f, rect, title, Icon::Terminal, true);
    let Some(Data::T3(state)) = &m.integration.snapshot.data else {
        connect_face(
            f,
            m,
            &l,
            "Connect T3",
            "Finds T3 Code on this PC. Existing pairing is reused; otherwise paste one T3 pairing code.",
            Icon::Terminal,
        );
        if let Some(footer) = l.footer {
            connection(f, m, footer, "Local T3 Code");
        }
        return;
    };
    match face.as_str() {
        "usage" => usage(f, m, state, &l),
        "new" => composer(f, m, state, &l),
        "attention" => threads(f, m, state, &l, true),
        _ => threads(f, m, state, &l, false),
    }
    if let Some(footer) = l.footer {
        let usage_label = state
            .usage_day
            .as_ref()
            .map_or("Usage access needed".into(), |day| {
                format!("{day} · UTC · refresh to update")
            });
        connection(
            f,
            m,
            footer,
            if face == "usage" {
                &usage_label
            } else {
                "Live from your local T3 Code"
            },
        );
    }
}

fn status(thread: &Thread) -> &'static str {
    if let Some(request) = &thread.pending_runtime_request {
        return if matches!(request.kind.as_str(), "user_input" | "user-input") {
            "Question for you"
        } else {
            "Approval needed"
        };
    }
    match thread.status {
        Status::Idle => "Idle",
        Status::Queued => "Queued",
        Status::Preparing => "Preparing",
        Status::Starting => "Starting",
        Status::Running => "Working",
        Status::Waiting => "Waiting",
        Status::Completed => "Completed",
        Status::Failed => "Failed",
        Status::Cancelled => "Cancelled",
        Status::Interrupted => "Interrupted",
        Status::RolledBack => "Rolled back",
    }
}

fn threads(f: &mut Frame<'_>, m: &Model, state: &Snapshot, l: &Areas, attention: bool) {
    let mut threads: Vec<_> = state
        .threads
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            if attention {
                t.attention()
            } else {
                t.running()
            }
        })
        .collect();
    threads.sort_by(|(_, a), (_, b)| {
        b.attention()
            .cmp(&a.attention())
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    let count = threads.len();
    if l.compact {
        metric(
            f,
            l.body,
            &count.to_string(),
            if attention {
                "need your input"
            } else {
                "threads & agents"
            },
            if attention && count > 0 {
                f.theme.color.warning
            } else {
                f.theme.color.label
            },
        );
        return;
    }
    if count == 0 {
        f.empty(
            l.body,
            Some(if attention {
                Icon::Check
            } else {
                Icon::Terminal
            }),
            if attention {
                "You're all caught up"
            } else {
                "No work running"
            },
            Some(if attention {
                "Approvals and questions appear here."
            } else {
                "Start a thread in T3 Code or use a New thread widget."
            }),
        );
        return;
    }
    let mut body = l.body;
    if body.height() / f.s >= 180.0 {
        let (summary, rest) = body.take_top(f.px(52.0), f.px(8.0));
        body = rest;
        metric(
            f,
            summary,
            &count.to_string(),
            if attention {
                "need your input"
            } else {
                "running threads & agents"
            },
            f.theme.color.label,
        );
    }
    let row_h = f.px(if attention && state.can_operate() {
        75.0
    } else {
        49.0
    });
    let visible = (body.height() / row_h).floor().max(1.0) as usize;
    m.scroll_max
        .set(count.saturating_sub(visible) as f32 * row_h);
    m.scroll_page.set(body.height());
    let first = (m.scroll.max(0.0) / row_h).floor() as usize;
    for (row, (index, thread)) in threads.iter().skip(first).take(visible).enumerate() {
        let top = body.top + row as f32 * row_h;
        let detail = state.details.get(&thread.id);
        let model = detail.map_or(thread.provider_instance_id.as_str(), |d| {
            if d.model.is_empty() {
                thread.provider_instance_id.as_str()
            } else {
                d.model.as_str()
            }
        });
        let subtitle = format!(
            "{} · {}{}",
            status(thread),
            model,
            if detail.is_some_and(|d| d.agent) {
                " · agent"
            } else {
                ""
            }
        );
        f.row(
            &Row {
                id: THREAD + *index as u32,
                lead: Lead::Icon(if thread.attention() {
                    Icon::Message
                } else {
                    Icon::Terminal
                }),
                title: &thread.title,
                subtitle: Some(&subtitle),
                accessory: Accessory::Chevron,
                selected: false,
                enabled: state.environment_id.is_some()
                    || m.integration.thread_links.contains_key(&thread.id),
            },
            Rect::new(body.left, top, body.width(), f.px(46.0).min(body.height())),
        );
        if attention
            && state.can_operate()
            && let Some(request) = &thread.pending_runtime_request
            && !matches!(
                request.kind.as_str(),
                "user_input" | "user-input" | "auth_refresh" | "dynamic_tool_call"
            )
        {
            let controls = Rect::new(
                body.left + f.px(34.0),
                top + f.px(47.0),
                body.width() - f.px(34.0),
                f.px(25.0),
            );
            let (approve, deny) =
                controls.take_left((controls.width() - f.px(6.0)) * 0.5, f.px(6.0));
            let enabled = m.integration.actionable() && !m.integration.busy();
            f.button(
                APPROVE + *index as u32,
                approve,
                "Approve",
                Some(Icon::Check),
                ButtonKind::Tinted,
                enabled,
            );
            f.button(
                DENY + *index as u32,
                deny,
                "Deny",
                None,
                ButtonKind::Gray,
                enabled,
            );
        }
    }
}

fn access(f: &mut Frame<'_>, m: &Model, l: &Areas, label: &str, detail: &str) {
    let (button, body) = l.body.take_bottom(f.px(29.0), f.px(7.0));
    if body.height() >= f.line(TextRole::Callout) {
        f.paragraph(
            body.left,
            body.top,
            body.width(),
            detail,
            TextRole::Callout,
            f.theme.color.secondary,
            (body.height() / f.line(TextRole::Callout)).floor().max(1.0) as usize,
        );
    }
    f.button(
        REAUTHORIZE,
        button,
        label,
        Some(Icon::Lock),
        ButtonKind::Tinted,
        !m.integration.busy(),
    );
}

fn usage(f: &mut Frame<'_>, m: &Model, state: &Snapshot, l: &Areas) {
    let Some(usage) = &state.usage else {
        access(
            f,
            m,
            l,
            "Allow usage",
            if l.compact {
                "Needs T3 usage access"
            } else {
                "Pair again with diagnostics access to read tokens and cost from T3 Code."
            },
        );
        return;
    };
    let tokens: u64 = usage.iter().map(|u| u.tokens).sum();
    if l.compact {
        metric(
            f,
            l.body,
            &number(tokens),
            "tokens · UTC day",
            f.theme.color.label,
        );
        return;
    }
    let (summary, body) = l.body.take_top(f.px(55.0), f.px(7.0));
    let (left, right) = summary.take_left(summary.width() * 0.5, f.px(7.0));
    metric(
        f,
        left,
        &number(tokens),
        "tokens · UTC day",
        f.theme.color.label,
    );
    let cost: u64 = usage.iter().map(|u| u.cost_microusd).sum();
    metric(
        f,
        right,
        &format!("${:.2}", cost as f64 / 1_000_000.0),
        if usage.iter().any(|u| u.unpriced) {
            "known cost · partial"
        } else {
            "estimated cost"
        },
        f.theme.color.label,
    );
    for (i, usage) in usage
        .iter()
        .take((body.height() / f.px(43.0)).floor().max(0.0) as usize)
        .enumerate()
    {
        f.row(
            &Row {
                id: 0,
                lead: Lead::None,
                title: &usage.model,
                subtitle: Some(&usage.provider),
                accessory: Accessory::Text(&format!(
                    "{} · ${:.2}",
                    number(usage.tokens),
                    usage.cost_microusd as f64 / 1_000_000.0
                )),
                selected: false,
                enabled: true,
            },
            Rect::new(
                body.left,
                body.top + i as f32 * f.px(43.0),
                body.width(),
                f.px(42.0),
            ),
        );
    }
}

pub(super) fn prompt_field(f: &mut Frame<'_>, m: &Model, rect: Rect, hint: &str) {
    let field = m.field.as_ref();
    let inner = f.field_frame(
        rect,
        hint,
        field.is_none(),
        field.is_some(),
        Some(Icon::Edit),
    );
    if let Some(field) = field {
        field.set_rect(inner);
        f.push(Op::TextField {
            rect: inner,
            field: field.clone(),
            revision: field.revision(),
        });
    }
    f.hit(
        crate::widgets::views::FIELD,
        rect,
        Role::TextInput,
        hint,
        true,
        None,
    );
}

fn composer(f: &mut Frame<'_>, m: &Model, state: &Snapshot, l: &Areas) {
    if !state.can_operate() {
        access(
            f,
            m,
            l,
            "Allow controls",
            if l.compact {
                "Thread access needed"
            } else {
                "Pair again with thread controls. New threads start only when you send a prompt here."
            },
        );
        return;
    }
    if let Some(picker) = m.integration.t3.picker {
        let (header, body) = l.body.take_top(f.px(28.0), f.px(7.0));
        let project = picker == Picker::Project;
        f.button(
            if project { PROJECT } else { MODEL },
            header,
            if project {
                "Choose project"
            } else {
                "Choose model"
            },
            Some(Icon::ChevronLeft),
            ButtonKind::Plain,
            true,
        );
        let count = if project {
            state.projects.len()
        } else {
            state.models.len()
        };
        let row_h = f.px(if project { 34.0 } else { 46.0 });
        let visible = (body.height() / row_h).floor().max(1.0) as usize;
        m.scroll_max
            .set(count.saturating_sub(visible) as f32 * row_h);
        m.scroll_page.set(body.height());
        let first = (m.scroll.max(0.0) / row_h).floor() as usize;
        for row in 0..visible.min(count.saturating_sub(first)) {
            let index = first + row;
            let title = if project {
                state.projects.values().nth(index).map(String::as_str)
            } else {
                state.models.get(index).map(|model| model.name.as_str())
            }
            .unwrap_or_default();
            let subtitle = if project {
                None
            } else {
                state
                    .models
                    .get(index)
                    .map(|model| model.provider_name.as_str())
            };
            f.row(
                &Row {
                    id: if project {
                        PROJECT_CHOICE
                    } else {
                        MODEL_CHOICE
                    } + index as u32,
                    lead: Lead::None,
                    title,
                    subtitle,
                    accessory: Accessory::None,
                    selected: index
                        == if project {
                            m.integration.t3.project
                        } else {
                            m.integration.t3.model
                        },
                    enabled: true,
                },
                Rect::new(
                    body.left,
                    body.top + row as f32 * row_h,
                    body.width(),
                    row_h - f.px(1.0),
                ),
            );
        }
        return;
    }
    if l.compact {
        let (button, body) = l.body.take_bottom(f.px(29.0), f.px(5.0));
        f.text_in(
            body,
            "Ready to build",
            TextRole::Headline,
            f.theme.color.label,
            Align::Start,
        );
        if m.field.is_some() {
            prompt_field(f, m, button, "What should we build?");
        } else {
            f.button(
                crate::widgets::views::FIELD,
                button,
                "Write a prompt",
                Some(Icon::Edit),
                ButtonKind::Tinted,
                state.can_operate(),
            );
        }
        return;
    }
    let (project, body) = l.body.take_top(f.px(29.0), f.px(6.0));
    let (model, body) = body.take_top(f.px(29.0), f.px(8.0));
    let project_name = state
        .projects
        .values()
        .nth(m.integration.t3.project)
        .map_or("Choose project", String::as_str);
    let model_name = state
        .models
        .get(m.integration.t3.model)
        .map_or("Choose model", |m| m.name.as_str());
    f.button(
        PROJECT,
        project,
        project_name,
        Some(Icon::Folder),
        ButtonKind::Gray,
        !state.projects.is_empty(),
    );
    f.button(
        MODEL,
        model,
        model_name,
        Some(Icon::ChevronDown),
        ButtonKind::Gray,
        !state.models.is_empty(),
    );
    if body.height() / f.s < 34.0 {
        return;
    }
    let (send, input) = body.take_right(f.px(34.0), f.px(6.0));
    let input = input.place(input.width(), f.px(34.0), Align::Start, Align::Start);
    let send = send.place(f.px(34.0), f.px(34.0), Align::Start, Align::Start);
    prompt_field(
        f,
        m,
        input,
        if state.can_operate() {
            "What should we build?"
        } else {
            "Allow thread controls in T3"
        },
    );
    icon_action(
        f,
        SEND,
        send,
        Icon::Upload,
        "Start a thread with approval required",
        state.can_operate() && m.integration.actionable() && !m.integration.busy(),
    );
    if body.height() / f.s >= 80.0 {
        f.paragraph(
            body.left,
            body.top + f.px(45.0),
            body.width(),
            "Starts in the selected project. T3 asks before edits or commands.",
            TextRole::Caption,
            f.theme.color.tertiary,
            2,
        );
    }
}

pub(super) fn number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1000 {
        format!("{:.1}K", value as f64 / 1000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "t3_tests.rs"]
mod tests;
