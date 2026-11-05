//! T3 · Usage: the last 30 days as T3's own usage page shows them. A big total cost and
//! sessions, one line per provider, the daily cost area chart, and token totals.
use super::t3::{Areas, number};
use super::*;
use mypaper_integrations::t3::{History, Snapshot};
use mypaper_ui::chart::{Series, area_chart};

/// Claude's ember, Codex's white, everyone else a quiet gray (T3's own palette).
fn color(f: &Frame<'_>, provider: &str) -> Rgba {
    match provider {
        p if p.contains("claude") => Rgba::rgb8([217, 119, 87], 1.0),
        p if p.contains("codex") => f.theme.color.label,
        _ => f.theme.color.tertiary,
    }
}

fn name(provider: &str) -> &str {
    match provider {
        p if p.contains("claude") => "Claude Code",
        p if p.contains("codex") => "Codex",
        p if p.contains("grok") => "Grok Build",
        p if p.contains("cursor") => "Cursor",
        p => p,
    }
}

fn dollars(micro: u64) -> String {
    let cents = (micro + 5_000) / 10_000;
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("${grouped}.{:02}", cents % 100)
}

/// "OCT 7" from "2026-10-07".
fn day_label(day: &str) -> String {
    const MONTHS: [&str; 12] = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let month = day.get(5..7).and_then(|m| m.parse::<usize>().ok());
    let date = day.get(8..10).and_then(|d| d.parse::<u32>().ok());
    match (month, date) {
        (Some(m @ 1..=12), Some(d)) => format!("{} {d}", MONTHS[m - 1]),
        _ => day.into(),
    }
}

pub(super) fn view(f: &mut Frame<'_>, state: &Snapshot, l: &Areas) {
    let Some(history) = &state.history else {
        f.empty(
            l.body,
            Some(Icon::Gauge),
            if state.usage_notice.is_some() {
                "Usage unavailable"
            } else {
                "Loading usage…"
            },
            Some(
                state
                    .usage_notice
                    .as_deref()
                    .unwrap_or("Reading the last 30 days from T3 Code."),
            ),
        );
        return;
    };
    // Largest first, as T3 stacks them.
    let mut providers: Vec<_> = history
        .providers
        .iter()
        .map(|p| (p, p.cost.iter().sum::<u64>(), p.tokens.iter().sum::<u64>()))
        .collect();
    providers.sort_by(|a, b| b.1.cmp(&a.1));
    let cost: u64 = providers.iter().map(|p| p.1).sum();
    let sessions: u64 = providers.iter().map(|p| p.0.sessions).sum();
    let headline = dollars(cost);
    let caption = if sessions > 0 && providers.iter().all(|p| p.0.sessions_distinct) {
        format!("{sessions} sessions · 30 days")
    } else {
        "30 days · API estimate".into()
    };
    if l.compact {
        metric(f, l.body, &headline, &caption, f.theme.color.label);
        return;
    }
    let wide = l.body.width() / f.s >= 520.0;
    let (summary, rest) = if wide {
        l.body.take_left(l.body.width() * 0.4, f.px(18.0))
    } else {
        (l.body, l.body)
    };
    // The headline.
    let mut big = f.font(TextRole::LargeTitle);
    big.size = f.px(32.0);
    let (_, h) = f.label_styled(
        summary.left,
        summary.top,
        &headline,
        &big,
        f.theme.color.label,
        summary.width(),
    );
    let mut y = summary.top + h + f.px(2.0);
    f.text_in(
        Rect::new(summary.left, y, summary.width(), f.line(TextRole::Callout)),
        &caption,
        TextRole::Callout,
        f.theme.color.secondary,
        Align::Start,
    );
    y += f.line(TextRole::Callout) + f.px(12.0);
    // One line per provider: name, sessions and cost, then share and tokens.
    let row_h = f.line(TextRole::Body) + f.line(TextRole::Caption) + f.px(10.0);
    let list_end = if wide {
        summary.bottom
    } else {
        (y + row_h * providers.len().min(3) as f32).min(l.body.bottom)
    };
    for (p, p_cost, tokens) in &providers {
        if y + row_h > list_end + 1.0 {
            break;
        }
        let line = Rect::new(summary.left, y, summary.width(), f.line(TextRole::Body));
        let (dot, line) = line.take_left(f.px(8.0), f.px(8.0));
        let d = f.px(7.0);
        f.fill(
            dot.place(d, d, Align::Center, Align::Center),
            d / 2.0,
            color(f, &p.provider),
        );
        let amount = dollars(*p_cost);
        let style = f.font(TextRole::Body);
        let amount_w = f.text.measure(&amount, &style).0;
        let (value, label) = line.take_right(amount_w, f.px(8.0));
        f.text_in(
            value,
            &amount,
            TextRole::Body,
            f.theme.color.label,
            Align::End,
        );
        // The name, then its session count in a quieter caption, as T3 lists them.
        let title = name(&p.provider);
        let title_w = f.text.measure(title, &style).0.min(label.width());
        f.text_in(
            label,
            title,
            TextRole::Body,
            f.theme.color.label,
            Align::Start,
        );
        if p.sessions > 0 && label.width() - title_w > f.px(40.0) {
            f.text_in(
                Rect::new(
                    label.left + title_w + f.px(6.0),
                    label.top,
                    label.width() - title_w - f.px(6.0),
                    label.height(),
                ),
                &format!(
                    "{} {}",
                    p.sessions,
                    if p.sessions_distinct {
                        "sessions"
                    } else {
                        "daily/model entries"
                    }
                ),
                TextRole::Caption,
                f.theme.color.secondary,
                Align::Start,
            );
        }
        let share = if cost == 0 {
            0.0
        } else {
            *p_cost as f64 * 100.0 / cost as f64
        };
        let share = if share > 0.0 && share < 0.1 {
            "<0.1%".to_owned()
        } else {
            format!("{share:.1}%")
        };
        f.text_in(
            Rect::new(
                line.left,
                y + f.line(TextRole::Body),
                summary.right - line.left,
                f.line(TextRole::Caption),
            ),
            &format!("{share} of cost · {} tokens", number(*tokens)),
            TextRole::Caption,
            f.theme.color.secondary,
            Align::Start,
        );
        y += row_h;
    }
    let chart_top = if wide { rest.top } else { y + f.px(4.0) };
    let totals_h = f.line(TextRole::Caption) + f.line(TextRole::Headline) + f.px(10.0);
    let room = rest.bottom - chart_top;
    let show_totals = room / f.s >= 170.0;
    let chart_bottom = if show_totals {
        rest.bottom - totals_h
    } else {
        rest.bottom
    };
    if chart_bottom - chart_top >= f.px(70.0) {
        let (title, plot) = Rect::new(rest.left, chart_top, rest.width(), chart_bottom - chart_top)
            .take_top(f.line(TextRole::Callout), f.px(4.0));
        f.text_in(
            title,
            "Daily cost",
            TextRole::Callout,
            f.theme.color.label,
            Align::Start,
        );
        let values: Vec<Vec<f64>> = providers
            .iter()
            .map(|(p, ..)| p.cost.iter().map(|c| *c as f64 / 1_000_000.0).collect())
            .collect();
        let series: Vec<Series<'_>> = providers
            .iter()
            .zip(&values)
            .map(|((p, ..), values)| Series {
                color: color(f, &p.provider),
                values,
            })
            .collect();
        let days = &history.days;
        let labels = [
            days.first().map(|d| day_label(d)).unwrap_or_default(),
            days.get(days.len() / 2)
                .map(|d| day_label(d))
                .unwrap_or_default(),
            days.last().map(|d| day_label(d)).unwrap_or_default(),
        ];
        area_chart(
            f,
            plot,
            &series,
            [&labels[0], &labels[1], &labels[2]],
            |v| {
                if v >= 1000.0 {
                    format!("${:.1}K", v / 1000.0)
                } else {
                    format!("${v:.0}")
                }
            },
        );
    }
    if show_totals {
        totals(
            f,
            history,
            Rect::new(
                l.body.left,
                l.body.bottom - totals_h + f.px(10.0),
                l.body.width(),
                totals_h - f.px(10.0),
            ),
        );
    }
}

/// Processed tokens, cached and uncached input, output, and cache savings, in equal columns.
fn totals(f: &mut Frame<'_>, history: &History, rect: Rect) {
    let processed = history.uncached + history.cached + history.cache_write + history.output;
    let mut cells = vec![
        ("Processed", number(processed)),
        ("Cached input", number(history.cached)),
        ("Uncached", number(history.uncached)),
        ("Output", number(history.output)),
    ];
    if history.savings_microusd > 0 {
        cells.push(("Cache savings", dollars(history.savings_microusd)));
    }
    let fit = ((rect.width() / f.px(84.0)).floor() as usize).clamp(2, cells.len());
    let each = rect.width() / fit as f32;
    for (i, (label, value)) in cells.iter().take(fit).enumerate() {
        let cell = Rect::new(
            rect.left + i as f32 * each,
            rect.top,
            each - f.px(6.0),
            rect.height(),
        );
        let (top, bottom) = cell.take_top(f.line(TextRole::Caption), f.px(0.0));
        f.text_in(
            top,
            label,
            TextRole::Caption,
            f.theme.color.secondary,
            Align::Start,
        );
        f.text_in(
            bottom,
            value,
            TextRole::Headline,
            f.theme.color.label,
            Align::Start,
        );
    }
}
