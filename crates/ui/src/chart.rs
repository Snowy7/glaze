//! Dashboard charts on the kit's existing ops: a smooth overlaid area chart and a quota bar.
//! No path primitive is needed: an area is one-pixel columns under dense line segments.
use crate::geom::{Point, Rect, Rgba};
use crate::kit::Frame;
use crate::layout::Align;
use crate::scene::Op;
use crate::theme::TextRole;

/// One area series, drawn in list order (put the largest first, as T3 does).
pub struct Series<'a> {
    pub color: Rgba,
    pub values: &'a [f64],
}

/// A smooth area chart with horizontal gridlines, left value ticks and first/middle/last x
/// labels. `format` prints a tick value. Values below zero clamp to the baseline.
pub fn area_chart(
    f: &mut Frame<'_>,
    rect: Rect,
    series: &[Series<'_>],
    x_labels: [&str; 3],
    format: impl Fn(f64) -> String,
) {
    let caption = f.line(TextRole::Caption);
    let max = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0_f64, f64::max);
    let step = nice_step(max / 3.0);
    let top = (step * 3.0).max(step);
    let labels: Vec<String> = (0..=3).map(|i| format(step * f64::from(i))).collect();
    let style = f.font(TextRole::Caption);
    let gutter = labels
        .iter()
        .map(|l| f.text.measure(l, &style).0)
        .fold(0.0, f32::max)
        + f.px(8.0);
    let plot = Rect::new(
        rect.left + gutter,
        rect.top + caption / 2.0,
        (rect.width() - gutter).max(1.0),
        (rect.height() - caption * 2.0).max(1.0),
    );
    for (i, label) in labels.iter().enumerate() {
        let y = (plot.bottom - plot.height() * i as f32 / 3.0).round() + 0.5;
        f.push(Op::Line {
            from: Point::new(plot.left, y),
            to: Point::new(plot.right, y),
            width: 1.0,
            color: f.theme.color.separator.with_alpha(if i == 0 { 0.9 } else { 0.45 }),
        });
        f.text_in(
            Rect::new(rect.left, y - caption / 2.0, gutter - f.px(8.0), caption),
            label,
            TextRole::Caption,
            f.theme.color.tertiary,
            Align::End,
        );
    }
    let axis = Rect::new(plot.left, plot.bottom + f.px(4.0), plot.width(), caption);
    for (label, align) in x_labels.iter().zip([Align::Start, Align::Center, Align::End]) {
        f.text_in(axis, label, TextRole::Caption, f.theme.color.tertiary, align);
    }
    let width = plot.width().floor() as usize;
    let line = f.px(1.5).max(1.0);
    for s in series {
        if s.values.is_empty() {
            continue;
        }
        let ys: Vec<f32> = (0..=width)
            .map(|x| {
                let v = smooth(s.values, x as f32 / width.max(1) as f32).clamp(0.0, top);
                plot.bottom - (v / top) as f32 * plot.height()
            })
            .collect();
        let wash = s.color.with_alpha(s.color.a * 0.16);
        for x in 0..width {
            let y = ys[x].min(ys[x + 1]);
            if plot.bottom - y > 0.5 {
                f.push(Op::Fill {
                    rect: Rect::new(plot.left + x as f32, y, 1.0, plot.bottom - y),
                    radius: 0.0,
                    color: wash,
                });
            }
        }
        for x in (0..width).step_by(2) {
            let next = (x + 2).min(width);
            f.push(Op::Line {
                from: Point::new(plot.left + x as f32, ys[x]),
                to: Point::new(plot.left + next as f32, ys[next]),
                width: line,
                color: s.color,
            });
        }
    }
}

/// Catmull-Rom through the samples at `t` in 0..=1.
fn smooth(values: &[f64], t: f32) -> f64 {
    if values.len() == 1 {
        return values[0];
    }
    let at = f64::from(t) * (values.len() - 1) as f64;
    let i = (at.floor() as usize).min(values.len() - 2);
    let u = at - i as f64;
    let p = |k: isize| values[(i as isize + k).clamp(0, values.len() as isize - 1) as usize];
    let (p0, p1, p2, p3) = (p(-1), p(0), p(1), p(2));
    0.5 * (2.0 * p1
        + (p2 - p0) * u
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u * u * u)
}

/// 1, 2, 2.5 or 5 times a power of ten, at least `raw`.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|v| *v >= raw)
        .unwrap_or(10.0 * power)
}

/// A quota bar: what is left fills from the start in `fill`, the used rest is hatched, a pace
/// marker shows where an even burn would be now, and a reset chip sits at the end.
pub struct LimitBar<'a> {
    /// The fraction left, 0..=1.
    pub left: f32,
    /// The fraction of the window's time still to run, 0..=1 (the pace marker).
    pub pace: Option<f32>,
    pub fill: Rgba,
    /// A monogram at the start (an account's initials).
    pub badge: Option<&'a str>,
    /// Text after the badge, usually the percent left.
    pub label: Option<&'a str>,
    /// The reset countdown chip at the end, such as "3d 19h".
    pub reset: Option<&'a str>,
}

pub fn limit_bar(f: &mut Frame<'_>, rect: Rect, bar: &LimitBar<'_>) {
    draw_limit_bar(f, rect, bar, false);
}

/// A quota bar for narrow dashboard columns. Monograms and percentages keep their full text;
/// the reset chip gives way when the column cannot fit all three.
pub fn compact_limit_bar(f: &mut Frame<'_>, rect: Rect, bar: &LimitBar<'_>) {
    draw_limit_bar(f, rect, bar, true);
}

fn draw_limit_bar(f: &mut Frame<'_>, rect: Rect, bar: &LimitBar<'_>, compact: bool) {
    let radius = f.px(5.0);
    let c = &f.theme.color;
    let (label, quiet, track) = (c.label, c.secondary, c.fill.with_alpha(c.fill.a * 0.6));
    f.fill(rect, radius, track);
    let split = (rect.left + rect.width() * bar.left.clamp(0.0, 1.0)).round();
    // The used part: hairline diagonals, clipped to the bar.
    f.push(Op::Clip {
        rect: Rect::new(split, rect.top, rect.right - split, rect.height()),
    });
    let gap = f.px(6.0);
    let mut x = split - rect.height();
    while x < rect.right {
        f.push(Op::Line {
            from: Point::new(x, rect.bottom),
            to: Point::new(x + rect.height(), rect.top),
            width: 1.0,
            color: label.with_alpha(0.13),
        });
        x += gap;
    }
    f.push(Op::Unclip);
    if split > rect.left + 1.0 {
        f.fill(
            Rect::new(rect.left, rect.top, split - rect.left, rect.height()),
            radius,
            bar.fill,
        );
    }
    let pad = f.px(if compact { 6.0 } else { 8.0 });
    let mut cursor = rect.left + pad;
    if let Some(badge) = bar.badge {
        let d = (rect.height() - f.px(10.0)).max(f.px(14.0)).min(f.px(20.0));
        let circle = Rect::new(cursor, rect.top + (rect.height() - d) / 2.0, d, d);
        f.fill(circle, d / 2.0, label.with_alpha(0.85));
        if compact {
            let mut style = f.font(TextRole::Caption);
            style.size = f.px(9.0);
            style.weight = 600;
            let (w, h) = f.text.measure(badge, &style);
            let at = circle.place(w, h, Align::Center, Align::Center);
            f.label_styled(at.left, at.top, badge, &style, Rgba::rgb8([20, 20, 22], 1.0), w + 1.0);
        } else {
            f.text_in(circle, badge, TextRole::Caption, Rgba::rgb8([20, 20, 22], 1.0), Align::Center);
        }
        cursor = circle.right + f.px(if compact { 4.0 } else { 6.0 });
    }
    let mut end = rect.right - pad;
    if let Some(reset) = bar.reset {
        let mut style = f.font(TextRole::Caption);
        if compact { style.size = f.px(9.0); }
        let text = format!("↻ {reset}");
        let w = f.text.measure(&text, &style).0 + f.px(if compact { 8.0 } else { 12.0 });
        let h = (rect.height() - f.px(8.0)).min(f.px(22.0));
        let chip = Rect::new(rect.right - f.px(4.0) - w, rect.top + (rect.height() - h) / 2.0, w, h);
        let label_width = bar.label.map_or(0.0, |text| f.text.measure(text, &f.font(TextRole::Caption)).0);
        if !compact || chip.left - f.px(4.0) >= cursor + label_width {
            f.fill(chip, f.px(4.0), Rgba::rgb8([0, 0, 0], 0.55));
            if compact {
                let (w, h) = f.text.measure(&text, &style);
                let at = chip.place(w, h, Align::Center, Align::Center);
                f.label_styled(at.left, at.top, &text, &style, label, w + 1.0);
            } else {
                f.text_in(chip, &text, TextRole::Caption, label, Align::Center);
            }
            end = chip.left - f.px(if compact { 4.0 } else { 6.0 });
        }
    }
    if let Some(text) = bar.label
        && end > cursor
    {
        f.text_in(
            Rect::new(cursor, rect.top, end - cursor, rect.height()),
            text,
            if compact { TextRole::Caption } else { TextRole::Headline },
            label,
            Align::Start,
        );
    }
    if let Some(pace) = bar.pace {
        let x = (rect.left + rect.width() * pace.clamp(0.0, 1.0)).round();
        f.fill(
            Rect::new(x - f.px(1.5), rect.top - f.px(2.0), f.px(3.0), rect.height() + f.px(4.0)),
            f.px(1.5),
            quiet.with_alpha(1.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interaction, Scheme, Style, Theme, Ui};

    #[test]
    fn compact_accounts_keep_initials_and_percent_at_dashboard_widths() {
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        for scale in [1.0, 1.5, 2.0] {
            for width in [80.0, 112.0, 160.0] {
                let mut f = Frame::new(&mut ui.text, &theme, scale, &input);
                let rect = Rect::new(0.0, 0.0, width * scale, 30.0 * scale);
                compact_limit_bar(&mut f, rect, &LimitBar {
                    left: 0.57, pace: None, fill: theme.color.accent,
                    badge: Some("ON"), label: Some("57%"), reset: Some("4d 1h"),
                });
                let text: Vec<_> = f.list.ops.iter().filter_map(|op| match op {
                    Op::Text { text, rect, .. } => Some((text.as_ref(), *rect)),
                    _ => None,
                }).collect();
                for expected in ["ON", "57%"] {
                    assert!(text.iter().any(|(text, _)| *text == expected), "{expected}: width {width}, scale {scale}: {text:?}");
                }
                for (_, bounds) in &text {
                    assert!(bounds.left >= rect.left && bounds.right <= rect.right && bounds.top >= rect.top && bounds.bottom <= rect.bottom);
                }
                for pair in text.windows(2) {
                    assert!(pair[0].1.right <= pair[1].1.left || pair[1].1.right <= pair[0].1.left, "overlapping account text: {text:?}");
                }
            }
        }
    }
}
