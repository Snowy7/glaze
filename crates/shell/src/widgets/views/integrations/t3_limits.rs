//! T3 · Limits: per provider, one card per limit window, one bar per account, as T3's own
//! limits page. Compact tiles show the tightest window's percent left.
use super::t3::Areas;
use super::*;
use mypaper_integrations::t3::{Limit, Snapshot};
use mypaper_ui::chart::{LimitBar, limit_bar};

/// The brand fill for a provider's bars (Claude's ember, Codex's graphite).
fn brand(driver: &str) -> Rgba {
    match driver {
        d if d.contains("claude") => Rgba::rgb8([84, 46, 37], 1.0),
        d if d.contains("codex") => Rgba::rgb8([96, 96, 98], 1.0),
        _ => Rgba::rgb8([70, 70, 74], 1.0),
    }
}

fn left(limit: &Limit) -> f32 {
    1.0 - limit.used_permille as f32 / 1000.0
}

/// Seconds to the reset, and the fraction of the window still to run.
fn timing(limit: &Limit, now_ms: i64) -> (Option<i64>, Option<f32>) {
    let reset = limit
        .resets_at
        .as_deref()
        .and_then(epoch_ms)
        .map(|t| (t - now_ms).max(0) / 1000);
    let pace = reset
        .zip(limit.duration_mins.filter(|d| *d > 0))
        .map(|(secs, mins)| (secs as f32 / (mins as f32 * 60.0)).clamp(0.0, 1.0));
    (reset, pace)
}

fn countdown(secs: i64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn initials(account: &str) -> String {
    let name = account.split('@').next().unwrap_or(account);
    let mut parts = name.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty());
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => a.chars().take(1).chain(b.chars().take(1)).collect::<String>(),
        (Some(a), None) => a.chars().take(2).collect::<String>(),
        _ => "?".into(),
    }
    .to_uppercase()
}

pub(super) fn view(f: &mut Frame<'_>, state: &Snapshot, l: &Areas, now_ms: i64) {
    if state.limits.is_empty() {
        f.empty(
            l.body,
            Some(Icon::Gauge),
            "No limits reported",
            Some("T3 shows limits once a provider reports them."),
        );
        return;
    }
    if l.compact {
        let tight = state
            .limits
            .iter()
            .min_by(|a, b| left(a).total_cmp(&left(b)))
            .unwrap();
        metric(
            f,
            l.body,
            &format!("{:.0}%", left(tight) * 100.0),
            &format!("{} · {} left", tight.provider, tight.window),
            f.theme.color.label,
        );
        return;
    }
    // Provider → window → accounts, in the order T3 reports them.
    let mut groups: Vec<(&str, &str, Vec<(&str, Vec<&Limit>)>)> = Vec::new();
    for limit in &state.limits {
        let g = match groups.iter().position(|g| g.0 == limit.driver) {
            Some(i) => &mut groups[i],
            None => {
                groups.push((&limit.driver, &limit.provider, Vec::new()));
                groups.last_mut().unwrap()
            }
        };
        match g.2.iter().position(|w| w.0 == limit.window) {
            Some(i) => g.2[i].1.push(limit),
            None => g.2.push((&limit.window, vec![limit])),
        }
    }
    let header = f.px(24.0);
    let card = f.px(if l.body.width() / f.s >= 420.0 { 84.0 } else { 104.0 });
    let gap = f.px(10.0);
    let mut y = l.body.top;
    for (driver, provider, windows) in &groups {
        if y + header + card > l.body.bottom + 1.0 {
            break;
        }
        let row = Rect::new(l.body.left, y, l.body.width(), header);
        let (dot, name) = row.take_left(f.px(10.0), f.px(8.0));
        let d = f.px(8.0);
        let color = if driver.contains("claude") {
            Rgba::rgb8([217, 119, 87], 1.0)
        } else {
            f.theme.color.label
        };
        f.fill(dot.place(d, d, Align::Center, Align::Center), d / 2.0, color);
        f.text_in(name, provider, TextRole::Headline, f.theme.color.label, Align::Start);
        y += header + f.px(4.0);
        for (window, accounts) in windows {
            if y + card > l.body.bottom + 1.0 {
                break;
            }
            let inner = f.card(Rect::new(l.body.left, y, l.body.width(), card));
            let wide = inner.width() / f.s >= 360.0;
            let (stat, bars) = if wide {
                inner.take_left((inner.width() * 0.3).max(f.px(120.0)), f.px(14.0))
            } else {
                inner.take_top(f.px(44.0), f.px(6.0))
            };
            let avg = accounts.iter().map(|a| left(a)).sum::<f32>() / accounts.len() as f32;
            let pace = accounts
                .iter()
                .filter_map(|a| timing(a, now_ms).1)
                .fold(None, |m: Option<f32>, p| Some(m.map_or(p, |m| m.min(p))));
            let (title, number) = stat.take_top(f.line(TextRole::Callout), f.px(2.0));
            f.text_in(title, window, TextRole::Callout, f.theme.color.secondary, Align::Start);
            let mut big = f.font(TextRole::LargeTitle);
            big.size = f.px(if wide { 34.0 } else { 26.0 });
            let value = format!("{:.0}%", avg * 100.0);
            let (w, h) = f.label_styled(
                number.left,
                number.top,
                &value,
                &big,
                f.theme.color.label,
                number.width(),
            );
            // As T3: burning faster than an even pace (less left than time left) points up.
            let trend = match pace {
                Some(p) if avg + 0.02 < p => " left ↗",
                Some(_) => " left ↘",
                None => " left",
            };
            let caption = f.line(TextRole::Callout);
            f.label(
                number.left + w + f.px(4.0),
                number.top + h - caption - f.px(4.0),
                trend,
                TextRole::Callout,
                f.theme.color.secondary,
                number.width() - w,
            );
            let bar_h = f.px(30.0).min(bars.height());
            let bars = bars.place(bars.width(), bar_h, Align::Start, Align::Center);
            let n = accounts.len() as f32;
            let each = (bars.width() - f.px(6.0) * (n - 1.0)) / n;
            for (i, limit) in accounts.iter().enumerate() {
                let rect = Rect::new(bars.left + i as f32 * (each + f.px(6.0)), bars.top, each, bar_h);
                let (reset, pace) = timing(limit, now_ms);
                let badge = (accounts.len() > 1).then(|| initials(&limit.account));
                let label = format!("{:.0}%", left(limit) * 100.0);
                let reset = reset.map(countdown);
                limit_bar(
                    f,
                    rect,
                    &LimitBar {
                        left: left(limit),
                        pace,
                        fill: brand(driver),
                        badge: badge.as_deref(),
                        label: (accounts.len() > 1).then_some(label.as_str()),
                        reset: reset.as_deref().filter(|_| each / f.s > 120.0),
                    },
                );
            }
            y += card + gap;
        }
    }
}

/// Milliseconds since the epoch of a UTC RFC 3339 time ("2026-11-09T12:00:00.000Z").
fn epoch_ms(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Howard Hinnant's days-from-civil.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + se * 1000)
}
