use super::*;
use crate::integrations::widgets::ITEM;
use mypaper_integrations::{Data, docker::Container};
use mypaper_ui::kit::{Accessory, Lead, Row};

fn container_row(f: &mut Frame<'_>, m: &Model, rect: Rect, index: usize, container: &Container) {
    let running = container.state == "running";
    let detail = match &m.integration.snapshot.data {
        Some(Data::Docker(state)) => state.usage.get(&container.id).map(|usage| {
            format!(
                "{} · {:.0} MiB",
                usage.cpu_hundredths.map_or_else(
                    || "CPU —".into(),
                    |cpu| format!("{:.0}%", cpu as f64 / 100.0)
                ),
                usage.memory_bytes as f64 / 1_048_576.0
            )
        }),
        _ => None,
    };
    let (button, label) = rect.take_right(f.px(28.0), f.px(3.0));
    f.row(
        &Row {
            id: 0,
            lead: Lead::Icon(if container.failed {
                Icon::Warning
            } else {
                Icon::Layers
            }),
            title: &container.name,
            subtitle: Some(if container.failed {
                "Needs attention"
            } else {
                detail.as_deref().unwrap_or(&container.state)
            }),
            accessory: Accessory::None,
            selected: false,
            enabled: m.integration.actionable(),
        },
        label,
    );
    icon_action(
        f,
        ITEM + index as u32,
        button,
        if running { Icon::Pause } else { Icon::Play },
        if running {
            "Stop container"
        } else {
            "Start container"
        },
        m.integration.actionable(),
    );
}

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let l = layout(f, rect, "Docker", Icon::Layers, true);
    let Some(Data::Docker(state)) = &m.integration.snapshot.data else {
        setup(
            f,
            m,
            l,
            "Connect Docker",
            "Use your existing local Docker engine. Choose its named pipe in Settings.",
            Icon::Layers,
        );
        return;
    };
    let running = state
        .containers
        .iter()
        .filter(|c| c.state == "running")
        .count();
    let mut rows = l.body;
    if l.compact {
        let (summary, rest) = rows.take_top(f.px(47.0), f.px(5.0));
        metric(
            f,
            summary,
            &running.to_string(),
            "containers running",
            f.theme.color.label,
        );
        rows = rest;
    } else if !l.large {
        let (summary, rest) = rows.take_left(f.px(105.0), f.px(8.0));
        metric(
            f,
            summary,
            &running.to_string(),
            "running",
            f.theme.color.label,
        );
        rows = rest;
    } else {
        let (summary, rest) = rows.take_top(f.px(57.0), f.px(9.0));
        let (count, usage) = summary.take_left(summary.width() * 0.30, f.px(7.0));
        let (cpu_rect, memory) = usage.take_left(usage.width() * 0.47, f.px(10.0));
        metric(
            f,
            count,
            &running.to_string(),
            "running",
            f.theme.color.label,
        );
        let samples: Vec<_> = state
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .filter_map(|c| state.usage.get(&c.id))
            .collect();
        let cpu = if samples.is_empty() || samples.iter().any(|s| s.cpu_hundredths.is_none()) {
            "—".into()
        } else {
            format!(
                "{:.0}%",
                samples.iter().filter_map(|s| s.cpu_hundredths).sum::<u64>() as f64 / 100.0
            )
        };
        let ram = if samples.is_empty() {
            "—".into()
        } else {
            format!(
                "{:.0}",
                samples.iter().map(|s| s.memory_bytes).sum::<u64>() as f64 / 1_048_576.0
            )
        };
        metric(f, cpu_rect, &cpu, "CPU · sample", f.theme.color.secondary);
        metric(f, memory, &ram, "MiB · sample", f.theme.color.secondary);
        rows = rest;
    }
    let row_height = f.px(if l.compact { 42.0 } else { 45.0 });
    for (index, container) in state
        .containers
        .iter()
        .enumerate()
        .take((rows.height() / row_height).floor().max(1.0) as usize)
    {
        container_row(
            f,
            m,
            Rect::new(
                rows.left,
                rows.top + index as f32 * row_height,
                rows.width(),
                row_height.min(rows.height()),
            ),
            index,
            container,
        );
    }
    if state.containers.is_empty() {
        f.text_in(
            rows,
            "No containers yet",
            TextRole::Callout,
            f.theme.color.secondary,
            Align::Center,
        );
    }
    footer(f, m, l.footer, "Lifecycle live · refresh samples metrics");
}
