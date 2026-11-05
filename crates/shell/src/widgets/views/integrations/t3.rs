use super::*;
use crate::integrations::widgets::ITEM;
use mypaper_integrations::{
    Data,
    t3::{Status, Thread},
};
use mypaper_ui::kit::{Accessory, Lead, Row};

fn status(thread: &Thread) -> &'static str {
    if let Some(request) = &thread.pending_runtime_request {
        return if request.kind.contains("approval") {
            "Approval needed"
        } else {
            "Question for you"
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

fn thread_row(f: &mut Frame<'_>, m: &Model, rect: Rect, index: usize, thread: &Thread) {
    let subtitle = format!("{} · {}", status(thread), thread.provider_instance_id);
    f.row(
        &Row {
            id: ITEM + index as u32,
            lead: Lead::Icon(if thread.attention() {
                Icon::Message
            } else if thread.running() {
                Icon::Terminal
            } else if thread.status == Status::Completed {
                Icon::Check
            } else if thread.status == Status::Failed {
                Icon::Warning
            } else {
                Icon::Clock
            }),
            title: &thread.title,
            subtitle: Some(&subtitle),
            accessory: Accessory::Chevron,
            selected: false,
            enabled: m.integration.thread_links.contains_key(&thread.id),
        },
        rect,
    );
}

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let l = layout(f, rect, "T3 Code", Icon::Terminal, true);
    let Some(Data::T3(state)) = &m.integration.snapshot.data else {
        setup(
            f,
            m,
            l,
            "Connect T3 Code",
            "Pair a read-only client to see threads and agents, questions and approvals.",
            Icon::Terminal,
        );
        return;
    };
    let running = state
        .threads
        .iter()
        .filter(|thread| thread.running())
        .count();
    let attention = state
        .threads
        .iter()
        .filter(|thread| thread.attention())
        .count();
    let mut threads: Vec<_> = state.threads.iter().enumerate().collect();
    threads.sort_by(|(_, a), (_, b)| {
        b.attention()
            .cmp(&a.attention())
            .then_with(|| b.running().cmp(&a.running()))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    if l.compact {
        let (summary, rest) = l.body.take_top(f.px(48.0), f.px(4.0));
        metric(
            f,
            summary,
            &running.to_string(),
            if attention > 0 {
                "threads · input needed"
            } else {
                "active threads"
            },
            if attention > 0 {
                f.theme.color.warning
            } else {
                f.theme.color.label
            },
        );
        if let Some((index, thread)) = threads.first() {
            thread_row(f, m, rest, *index, thread);
        }
    } else if !l.large {
        let (summary, rows) = l.body.take_left(f.px(90.0), f.px(10.0));
        metric(
            f,
            summary,
            &running.to_string(),
            "active threads",
            f.theme.color.label,
        );
        for (row, (index, thread)) in threads.iter().take(2).enumerate() {
            thread_row(
                f,
                m,
                Rect::new(
                    rows.left,
                    rows.top + row as f32 * f.px(43.0),
                    rows.width(),
                    f.px(42.0),
                ),
                *index,
                thread,
            );
        }
    } else {
        let (summary, rows) = l.body.take_top(f.px(62.0), f.px(9.0));
        let (active, waiting) = summary.take_left(summary.width() * 0.5, f.px(10.0));
        metric(
            f,
            active,
            &running.to_string(),
            "active threads",
            f.theme.color.label,
        );
        metric(
            f,
            waiting,
            &attention.to_string(),
            "need your input",
            if attention > 0 {
                f.theme.color.warning
            } else {
                f.theme.color.secondary
            },
        );
        let count = (rows.height() / f.px(48.0)).floor().max(0.0) as usize;
        for (row, (index, thread)) in threads.iter().take(count).enumerate() {
            thread_row(
                f,
                m,
                Rect::new(
                    rows.left,
                    rows.top + row as f32 * f.px(48.0),
                    rows.width(),
                    f.px(47.0),
                ),
                *index,
                thread,
            );
        }
    }
    if state.threads.is_empty() {
        f.text_in(
            l.body,
            "No active threads",
            TextRole::Callout,
            f.theme.color.secondary,
            Align::Center,
        );
    }
    footer(
        f,
        m,
        l.footer,
        if attention > 0 {
            "Your agents are waiting for you"
        } else {
            "Updates from your local environment"
        },
    );
}
