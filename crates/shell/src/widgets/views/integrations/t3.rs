//! Four independent T3 faces. Small tiles expose one state; larger tiles reveal controls.
use super::*;
use crate::integrations::{
    t3_widgets::{
        APPROVE, DENY, MODEL, MODEL_CHOICE, PROJECT, PROJECT_CHOICE, Picker, SEND, THREAD,
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
    m.integration.t3.targets.borrow_mut().clear();
    let face = m.str("variant", "running");
    let title = match face.as_str() {
        "usage" => "T3 · Usage",
        "limits" => "T3 · Limits",
        "new" => "T3 · New thread",
        "attention" => "T3 · Needs you",
        _ => "T3 · Running now",
    };
    let l = chrome(f, rect, title, Icon::Terminal, true);
    let Some(Data::T3(state)) = &m.integration.snapshot.data else {
        pairing(f, m, &l);
        if let Some(footer) = l.footer {
            connection(f, m, footer, "Local T3 Code");
        }
        return;
    };
    match face.as_str() {
        "usage" => super::t3_usage::view(f, state, &l),
        "limits" => super::t3_limits::view(f, state, &l, _now_ms),
        "new" => composer(f, m, state, &l),
        "attention" => threads(f, m, state, &l, true),
        _ => threads(f, m, state, &l, false),
    }
    if let Some(footer) = l.footer {
        let usage_label = state
            .usage_day
            .as_ref()
            .map_or("Last 30 days".into(), |day| {
                format!("30 days to {day} · UTC")
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

/// No data yet. mypaper connects in the background through T3's CLI; only when that cannot
/// work (T3 closed, CLI missing) does the tile offer one field for a pasted pairing code.
fn pairing(f: &mut Frame<'_>, m: &Model, l: &Areas) {
    let failed = match &m.integration.snapshot.availability {
        Availability::Unavailable(reason) | Availability::Stale(reason) => Some(reason.as_str()),
        _ => None,
    };
    let Some(reason) = failed.filter(|_| !m.integration.busy()) else {
        f.empty(
            l.body,
            Some(Icon::Terminal),
            "Connecting to T3 Code…",
            (!l.compact).then_some("mypaper signs in through T3 Code on this PC."),
        );
        return;
    };
    let (button, rest) = l.body.take_bottom(f.px(30.0), f.px(7.0));
    let (field, description) = rest.take_bottom(f.px(30.0), f.px(7.0));
    if description.height() >= f.line(TextRole::Callout) {
        let detail = if reason.contains("not running") {
            "Open T3 Code and refresh, or paste a pairing code from T3 Code › Settings › Connections."
        } else {
            "Paste a pairing code from T3 Code › Settings › Connections."
        };
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
    prompt_field(f, m, field, "Paste pairing code");
    f.button(
        CONNECT,
        button,
        "Connect",
        Some(Icon::Terminal),
        ButtonKind::Tinted,
        !m.integration.busy(),
    );
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
    // Subagents roll up under their top-level thread: one item per proper thread.
    let groups = families(state, &threads);
    let agents: usize = groups.iter().map(|g| g.agents.len()).sum();
    let tops = groups.iter().filter(|g| g.top_matches).count();
    let count = groups.len();
    let counts = match (tops, agents) {
        (t, 0) => plural(t, "thread"),
        (0, a) => plural(a, "agent"),
        (t, a) => format!("{} · {}", plural(t, "thread"), plural(a, "agent")),
    };
    let threads: Vec<(usize, &Thread)> = groups.iter().map(|g| (g.index, g.actionable)).collect();
    if l.compact {
        metric(
            f,
            l.body,
            &count.to_string(),
            if attention {
                "need your input"
            } else {
                &counts
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
            &if attention {
                format!("need your input · {counts}")
            } else {
                format!("running · {counts}")
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
    for (row, ((index, thread), group)) in threads
        .iter()
        .zip(&groups)
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let top = body.top + row as f32 * row_h;
        let detail = state.details.get(&group.top.id);
        let model = detail.map_or(group.top.provider_instance_id.as_str(), |d| {
            if d.model.is_empty() {
                group.top.provider_instance_id.as_str()
            } else {
                d.model.as_str()
            }
        });
        let working = group.agents.iter().filter(|a| a.running()).count();
        let agents = match group.agents.len() {
            0 => String::new(),
            n if attention => format!(" · {} needs you", plural(n, "agent")),
            n if working == n => format!(" · {} working", plural(n, "agent")),
            n => format!(" · {} ({working} working)", plural(n, "agent")),
        };
        let subtitle = if group.top_matches {
            format!("{} · {model}{agents}", status(group.top))
        } else {
            format!("{} · {}", agents.trim_start_matches(" · "), status(thread))
        };
        m.integration.t3.targets.borrow_mut().insert(
            THREAD + *index as u32,
            crate::integrations::t3_widgets::Target {
                thread_id: thread.id.clone(),
                request_id: None,
            },
        );
        f.row(
            &Row {
                id: THREAD + *index as u32,
                lead: Lead::Icon(if thread.attention() {
                    Icon::Message
                } else {
                    Icon::Terminal
                }),
                title: &group.top.title,
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
            && request.approvable()
        {
            for hit in [APPROVE + *index as u32, DENY + *index as u32] {
                m.integration.t3.targets.borrow_mut().insert(
                    hit,
                    crate::integrations::t3_widgets::Target {
                        thread_id: thread.id.clone(),
                        request_id: Some(request.id.clone()),
                    },
                );
            }
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

pub(super) fn prompt_field(f: &mut Frame<'_>, m: &Model, rect: Rect, hint: &str) {
    let field = m.field.as_ref();
    let pairing = m.kind == crate::widgets::model::Widget::T3
        && !matches!(m.integration.snapshot.data, Some(Data::T3(_)));
    let draft = if m.kind == crate::widgets::model::Widget::T3 {
        m.integration.t3.draft.as_str()
    } else {
        ""
    };
    let inner = f.field_frame(
        rect,
        hint,
        field.is_none() && draft.is_empty(),
        field.is_some(),
        Some(Icon::Edit),
    );
    if pairing {
        // Input still belongs to RichEdit; only bullets enter the display list, focused or not.
        let text = field
            .and_then(|f| f.text().ok())
            .unwrap_or_else(|| draft.into());
        if let Some(field) = field {
            field.set_rect(inner);
        }
        f.text_in(
            inner,
            &"•".repeat(text.chars().count().min(32)),
            TextRole::Body,
            f.theme.color.label,
            Align::Start,
        );
    } else if let Some(field) = field {
        field.set_rect(inner);
        f.push(Op::TextField {
            rect: inner,
            field: field.clone(),
            revision: field.revision(),
        });
    } else if !draft.is_empty() {
        f.text_in(
            inner,
            draft,
            TextRole::Body,
            f.theme.color.label,
            Align::Start,
        );
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
        // mypaper's own grant includes thread controls; this only shows mid-reconnect.
        f.empty(
            l.body,
            Some(Icon::Terminal),
            "Thread controls unavailable",
            Some("Refresh to reconnect to T3 Code."),
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
    if value >= 1_000_000_000 {
        format!("{:.1}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
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

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// A top-level thread and the matching subagents beneath it (at any depth).
struct Family<'a> {
    index: usize,
    top: &'a Thread,
    /// The thread a click acts on: the top one when it matches, else its first matching agent.
    actionable: &'a Thread,
    top_matches: bool,
    agents: Vec<&'a Thread>,
}

/// Groups matching threads under their root thread, in the order the roots first appear.
fn families<'a>(state: &'a Snapshot, matching: &[(usize, &'a Thread)]) -> Vec<Family<'a>> {
    let root = |thread: &'a Thread| -> (usize, &'a Thread) {
        let mut current = thread;
        for _ in 0..16 {
            let Some(parent) = state
                .details
                .get(&current.id)
                .filter(|d| d.agent)
                .and_then(|d| d.parent_id.as_deref())
                .and_then(|id| state.threads.iter().find(|t| t.id == id))
            else {
                break;
            };
            current = parent;
        }
        let index = state
            .threads
            .iter()
            .position(|t| std::ptr::eq(t, current))
            .unwrap_or_default();
        (index, current)
    };
    let mut out: Vec<Family<'a>> = Vec::new();
    for &(index, thread) in matching {
        let (top_index, top) = root(thread);
        let is_top = std::ptr::eq(top, thread);
        let family = match out.iter().position(|f| std::ptr::eq(f.top, top)) {
            Some(i) => &mut out[i],
            None => {
                out.push(Family {
                    index: if is_top { index } else { top_index },
                    top,
                    actionable: thread,
                    top_matches: false,
                    agents: Vec::new(),
                });
                out.last_mut().unwrap()
            }
        };
        if is_top {
            family.top_matches = true;
            family.actionable = thread;
            family.index = index;
        } else {
            family.agents.push(thread);
        }
    }
    out
}
