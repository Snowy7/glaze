//! Weather. By its box, as the web widget: **mini** (temperature and symbol), a one-line
//! **strip**, a **stack** (place, temperature and symbol, conditions, high and low), **tall**
//! (the stack, the next hours and the days) and **side** (the stack with the days in a column
//! beside it). The symbols are drawn in the web widget's colours (sun, moon, cloud, rain), the
//! way Apple's multicolour weather symbols read on glass.
use super::{Ctx, PRIMARY, PRIVACY, RETRY};
use glaze_ui::calendar::{Date, WEEKDAYS};
use glaze_ui::icons::Icon;
use glaze_ui::{Align, ButtonKind, Op, Point, Rect, Rgba, Role, TextRole, TextStyle};
use serde_json::Value;
use std::f32::consts::TAU;

/// The symbols' colours, from the web widget's calm artwork.
const SUN: [u8; 3] = [255, 200, 61];
const MOON: [u8; 3] = [243, 238, 214];
const CLOUD: [u8; 3] = [255, 255, 255];
const GREY_CLOUD: [u8; 3] = [223, 228, 238];
const DARK_CLOUD: [u8; 3] = [152, 162, 186];
const GREY_CLOUD_ON_LIGHT: [u8; 3] = [176, 186, 204];
const MOON_ON_LIGHT: [u8; 3] = [196, 170, 92];
const RAIN: [u8; 3] = [142, 201, 255];
const BOLT: [u8; 3] = [255, 204, 51];

#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Mini,
    Strip,
    Stack,
    Tall,
    Side,
}

pub fn draw(c: &mut Ctx, r: Rect) {
    let current = &c.m.data["current"];
    if !current.is_object() {
        return status(c, r);
    }
    let s = c.s();
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let layout = match c.m.str("variant", "card").as_str() {
        "forecast" if h < 125.0 => Layout::Mini,
        "forecast" if h < 200.0 && w >= 260.0 => Layout::Strip,
        "forecast" => Layout::Tall,
        _ if h < 160.0 && w < 300.0 => Layout::Mini,
        _ if h < 160.0 => Layout::Strip,
        "card" if w >= 400.0 => Layout::Side,
        "card" if h >= 270.0 => Layout::Tall,
        _ => Layout::Stack,
    };
    c.hit(PRIMARY, r, Role::Button, "Open weather forecast");
    match layout {
        Layout::Mini => mini(c, r),
        Layout::Strip => strip(c, r),
        Layout::Stack => {
            stack(c, r, false);
        }
        Layout::Tall => {
            let used = stack(c, r, true);
            let rest = Rect { top: r.top + used + c.px(12.0), ..r };
            below(c, rest);
        }
        Layout::Side => {
            let (left, right) = r.take_left((r.width() * 0.46).round(), c.px(20.0));
            stack(c, left, false);
            days(c, right, false);
        }
    }
    stale(c, r);
}

struct Now {
    temperature: String,
    place: String,
    condition: String,
    range: Option<String>,
}

fn now(c: &Ctx) -> Now {
    let m = c.m;
    let current = &m.data["current"];
    let first = &m.data["forecast"]["days"][0];
    Now {
        temperature: format!("{:.0}°", current["temperature"].as_f64().unwrap_or(0.0)),
        place: current["location"]["name"]
            .as_str()
            .unwrap_or("Local weather")
            .to_owned(),
        condition: current["condition"].as_str().unwrap_or("").to_owned(),
        range: first.is_object().then(|| {
            format!(
                "H {:.0}°  L {:.0}°",
                first["max"].as_f64().unwrap_or(0.0),
                first["min"].as_f64().unwrap_or(0.0)
            )
        }),
    }
}

fn temperature_style(c: &Ctx, size: f32) -> TextStyle {
    TextStyle {
        tracking: -0.03 * size * c.s(),
        ..c.display(size, 400)
    }
}

/// Small: the place, the temperature large, the symbol in the corner, the conditions.
fn mini(c: &mut Ctx, r: Rect) {
    let n = now(c);
    let color = c.color();
    let s = c.s();
    let h = c.rect.height() / s;
    let size = (h * 0.4).clamp(30.0, 46.0);
    let style = temperature_style(c, size);
    let art = r.height().min(r.width() * 0.4).min(c.px(64.0)).floor();
    let (art_box, text) = r.take_right(art, c.px(8.0));
    symbol(c, art_box.place(art, art, Align::End, Align::Center), &c.m.data["current"].clone());
    let place_h = c.f.line(TextRole::Subheadline);
    let temp_h = (style.size * 1.08).round();
    let detail_h = c.f.line(TextRole::Footnote);
    let tall = r.height() >= place_h + temp_h + detail_h;
    let total = temp_h + if tall { place_h + detail_h } else { 0.0 };
    let mut y = r.top + ((r.height() - total) / 2.0).round();
    if tall {
        c.text(Rect::new(text.left, y, text.width(), place_h), &n.place, TextRole::Subheadline, color.secondary, Align::Start);
        y += place_h;
    }
    c.styled(Rect::new(text.left, y, text.width(), temp_h), &n.temperature, &style, color.label, Align::Start, Align::Center);
    y += temp_h;
    if tall {
        let detail = n.range.clone().unwrap_or(n.condition.clone());
        c.text(Rect::new(text.left, y, text.width(), detail_h), &detail, TextRole::Footnote, color.secondary, Align::Start);
    }
}

/// One line: symbol, temperature, then the place over the conditions; the next days at the
/// right when the strip is long enough.
fn strip(c: &mut Ctx, r: Rect) {
    let n = now(c);
    let color = c.color();
    let s = c.s();
    let h = c.rect.height() / s;
    let art = r.height().min(r.width() * 0.3).floor();
    let (art_box, rest) = r.take_left(art, c.px(12.0));
    symbol(c, art_box.place(art, art, Align::Start, Align::Center), &c.m.data["current"].clone());
    let style = temperature_style(c, (h * 0.42).clamp(32.0, 50.0));
    let tw = c.f.text.measure(&n.temperature, &style).0;
    let (temp, rest) = rest.take_left(tw + 2.0, c.px(14.0));
    c.styled(temp, &n.temperature, &style, color.label, Align::Start, Align::Center);
    // Days as columns at the right when there is room for three or more.
    let day_count = c.m.data["forecast"]["days"].as_array().map_or(0, Vec::len);
    let room_for = ((rest.width() - c.px(150.0)) / c.px(52.0)).floor().max(0.0) as usize;
    let shown = room_for.min(day_count).min(7);
    let rest = if shown >= 3 {
        let (cols, rest) = rest.take_right(shown as f32 * c.px(52.0), c.px(12.0));
        day_columns(c, cols, shown);
        rest
    } else {
        rest
    };
    let (a, b) = (c.f.line(TextRole::Headline), c.f.line(TextRole::Footnote));
    let y = rest.top + ((rest.height() - a - b) / 2.0).round();
    c.text(Rect::new(rest.left, y, rest.width(), a), &n.place, TextRole::Headline, color.label, Align::Start);
    let detail = match &n.range {
        Some(range) if !n.condition.is_empty() => format!("{} · {range}", n.condition),
        Some(range) => range.clone(),
        None => n.condition.clone(),
    };
    c.text(Rect::new(rest.left, y + a, rest.width(), b), &detail, TextRole::Footnote, color.secondary, Align::Start);
}

/// The place, the temperature with the symbol beside it, the conditions and the range. The
/// height used.
fn stack(c: &mut Ctx, r: Rect, tall: bool) -> f32 {
    let n = now(c);
    let color = c.color();
    let s = c.s();
    let (w, h) = (r.width() / s, c.rect.height() / s);
    let size = if tall {
        (h * 0.21).min(w * 0.19).clamp(42.0, 68.0)
    } else {
        (h * 0.23).min(w * 0.17 * 2.0).clamp(36.0, 64.0)
    };
    let style = temperature_style(c, size);
    let place_h = c.f.line(TextRole::Headline);
    let temp_h = (style.size * 1.1).round();
    let cond = c.display((size * 0.3).clamp(12.0, 16.0), 600);
    let rng = c.display((size * 0.26).clamp(11.0, 14.0), 400);
    let (cond_h, rng_h) = ((cond.size * 1.35).round(), (rng.size * 1.35).round());
    let total = place_h + temp_h + cond_h + rng_h;
    let top = if tall {
        r.top
    } else {
        r.top + ((r.height() - total) / 2.0).max(0.0).round()
    };
    let art = (temp_h + place_h).min(r.width() * 0.4).min(c.px(130.0)).floor();
    let art_rect = Rect::new(r.right - art, top, art, art);
    symbol(c, art_rect, &c.m.data["current"].clone());
    let text_w = r.width() - art - c.px(6.0);
    let mut y = top;
    c.text(Rect::new(r.left, y, text_w, place_h), &n.place, TextRole::Headline, color.label, Align::Start);
    y += place_h;
    c.styled(Rect::new(r.left - (size * 0.03 * s).round(), y, text_w, temp_h), &n.temperature, &style, color.label, Align::Start, Align::Center);
    y += temp_h;
    c.styled(Rect::new(r.left, y, r.width(), cond_h), &n.condition, &cond, color.label, Align::Start, Align::Center);
    y += cond_h;
    if let Some(range) = &n.range {
        c.styled(Rect::new(r.left, y, r.width(), rng_h), range, &rng, color.secondary, Align::Start, Align::Center);
    }
    y + rng_h - r.top
}

/// The next hours (when there is room for them and the days) and the days.
fn below(c: &mut Ctx, r: Rect) {
    let hourly = c.m.data["forecast"]["hourly"].as_array().cloned().unwrap_or_default();
    let slot = c.px(46.0) + c.px(6.0);
    let cols = ((r.width() / slot).floor() as usize).min(hourly.len());
    let hours_h = c.px(78.0);
    let r = if cols >= 4 && r.height() >= hours_h + c.px(100.0) {
        let (row, rest) = r.take_top(hours_h, c.px(10.0));
        let color = c.color();
        let w = row.width() / cols as f32;
        let cap = c.f.line(TextRole::Caption);
        let value = c.f.line(TextRole::Headline);
        for (i, hour) in hourly.iter().take(cols).enumerate() {
            let x = row.left + i as f32 * w;
            let label = if i == 0 {
                "NOW".to_owned()
            } else {
                hour["time"].as_str().and_then(|t| t.get(11..13)).unwrap_or("").to_owned()
            };
            c.text(Rect::new(x, row.top, w, cap), &label, TextRole::Caption, color.secondary, Align::Center);
            let icon = c.px(26.0);
            symbol(c, Rect::new((x + (w - icon) / 2.0).round(), row.top + cap + c.px(4.0), icon, icon), hour);
            let t = format!("{:.0}°", hour["temperature"].as_f64().unwrap_or(0.0));
            c.text(Rect::new(x, row.top + cap + icon + c.px(8.0), w, value), &t, TextRole::Headline, color.label, Align::Center);
        }
        rest
    } else {
        r
    };
    days(c, r, true);
}

/// The days as rows: name, symbol, low, a range bar against the week's span, high.
fn days(c: &mut Ctx, r: Rect, below: bool) {
    let list = c.m.data["forecast"]["days"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return;
    }
    let color = c.color();
    let row = c.px(32.0).max((r.height() / list.len().min(7) as f32).min(c.px(40.0))).floor();
    let fit = ((r.height() / row).floor() as usize).min(list.len()).min(7);
    let low = list.iter().filter_map(|d| d["min"].as_f64()).reduce(f64::min).unwrap_or(0.0);
    let high = list.iter().filter_map(|d| d["max"].as_f64()).reduce(f64::max).unwrap_or(1.0);
    let top = if below {
        r.top
    } else {
        r.top + ((r.height() - fit as f32 * row) / 2.0).max(0.0).round()
    };
    for (i, day) in list.iter().take(fit).enumerate() {
        let line = Rect::new(r.left, top + i as f32 * row, r.width(), row);
        let name = day["date"]
            .as_str()
            .and_then(Date::parse)
            .map(|d| if i == 0 { "Today".to_owned() } else { WEEKDAYS[d.weekday() as usize].to_owned() })
            .unwrap_or_default();
        let (name_box, rest) = line.take_left(c.px(48.0), c.px(4.0));
        c.text(name_box, &name, TextRole::Headline, color.label, Align::Start);
        let icon = c.px(22.0);
        let (icon_box, rest) = rest.take_left(icon, c.px(8.0));
        symbol(c, icon_box.place(icon, icon, Align::Center, Align::Center), day);
        let min = day["min"].as_f64().unwrap_or(0.0);
        let max = day["max"].as_f64().unwrap_or(0.0);
        let value_w = c.px(34.0);
        let (lo, rest) = rest.take_left(value_w, c.px(8.0));
        let (hi, bar) = rest.take_right(value_w, c.px(8.0));
        c.text(lo, &format!("{min:.0}°"), TextRole::Body, color.secondary, Align::End);
        c.text(hi, &format!("{max:.0}°"), TextRole::Headline, color.label, Align::End);
        if bar.width() >= c.px(30.0) {
            let h = c.px(4.0);
            let track = bar.place(bar.width(), h, Align::Start, Align::Center);
            c.f.fill(track, h / 2.0, color.fill);
            let span = (high - low).max(1.0);
            let from = track.left + ((min - low) / span) as f32 * track.width();
            let width = (((max - min) / span) as f32 * track.width()).max(h);
            c.f.fill(Rect::new(from.round(), track.top, width.round(), h), h / 2.0, color.label);
        }
    }
}

/// Days as columns (a strip's right side): name over symbol over high and low.
fn day_columns(c: &mut Ctx, r: Rect, n: usize) {
    let list = c.m.data["forecast"]["days"].as_array().cloned().unwrap_or_default();
    let color = c.color();
    let w = r.width() / n as f32;
    let cap = c.f.line(TextRole::Caption);
    let icon = c.px(20.0);
    let value = c.f.line(TextRole::Footnote);
    let total = cap + icon + c.px(4.0) + value;
    let top = r.top + ((r.height() - total) / 2.0).round();
    for (i, day) in list.iter().take(n).enumerate() {
        let x = r.left + i as f32 * w;
        let name = day["date"]
            .as_str()
            .and_then(Date::parse)
            .map(|d| WEEKDAYS[d.weekday() as usize].to_uppercase())
            .unwrap_or_default();
        c.text(Rect::new(x, top, w, cap), &name, TextRole::Caption, color.secondary, Align::Center);
        symbol(c, Rect::new((x + (w - icon) / 2.0).round(), top + cap, icon, icon), day);
        let t = format!(
            "{:.0}° {:.0}°",
            day["max"].as_f64().unwrap_or(0.0),
            day["min"].as_f64().unwrap_or(0.0)
        );
        c.text(Rect::new(x, top + cap + icon + c.px(4.0), w, value), &t, TextRole::Footnote, color.label, Align::Center);
    }
}

/// Locating, failed or unavailable, with Retry and the location settings after a failure.
fn status(c: &mut Ctx, r: Rect) {
    let failed = c.m.error.is_some();
    let message = c.m.error.clone().unwrap_or_else(|| "Finding your local forecast…".into());
    let control = c.px(c.f.theme.metrics.control_small);
    let (body, buttons) = if failed && r.height() >= c.px(110.0) {
        let (buttons, body) = r.take_bottom(control, c.px(8.0));
        (body, Some(buttons))
    } else {
        (r, None)
    };
    c.empty(
        body,
        Some(if failed { Icon::Warning } else { Icon::Location }),
        if failed { "Weather unavailable" } else { "Locating…" },
        Some(&message),
    );
    if let Some(buttons) = buttons {
        let gap = c.px(8.0);
        let (retry, privacy) = buttons.take_left((buttons.width() - gap) / 2.0, gap);
        c.f.button(RETRY, retry, "Retry", None, ButtonKind::Gray, true);
        c.f.button(PRIVACY, privacy, "Location", None, ButtonKind::Gray, true);
    } else if failed {
        c.hit(RETRY, r, Role::Button, "Retry weather");
    }
}

/// "Stale" and Retry when the last refresh failed but older data is shown.
fn stale(c: &mut Ctx, r: Rect) {
    let Some(error) = c.m.error.clone() else { return };
    let color = c.color();
    let control = c.px(c.f.theme.metrics.control_small);
    let (row, _) = r.take_bottom(control, 0.0);
    let (w, _) = c.f.button_size("Retry", false);
    let (button, text) = row.take_right(w, c.px(8.0));
    c.text(text, &format!("Stale · {error}"), TextRole::Footnote, color.tertiary, Align::Start);
    c.f.button(RETRY, button, "Retry", None, ButtonKind::Gray, true);
}

#[derive(Clone, Copy, PartialEq)]
enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Storm,
}

/// What the sky looks like, from an Open-Meteo code (or a condition with one), and whether
/// it is night.
fn sky(v: &Value) -> (Sky, bool) {
    let code = v["code"]
        .as_u64()
        .or_else(|| v["condition"]["code"].as_u64())
        .unwrap_or(0);
    let night = v["is_day"].as_bool() == Some(false)
        || v["icon"].as_str().is_some_and(|s| s.ends_with("night"));
    let sky = match code {
        0 => Sky::Clear,
        1 | 2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Storm,
        _ => Sky::Rain,
    };
    (sky, night)
}

/// A filled weather symbol in `rect` (square), in 24-unit space: a sun with rays or the moon,
/// layered clouds, rain, snow, fog and a bolt. Flat shapes stay crisp at every size.
fn symbol(c: &mut Ctx, rect: Rect, v: &Value) {
    let (sky, night) = sky(v);
    // White clouds vanish on light glass: there they take the darker greys.
    let (cloud, grey, moon) = if c.f.theme.dark() {
        (CLOUD, GREY_CLOUD, MOON)
    } else {
        (GREY_CLOUD_ON_LIGHT, DARK_CLOUD, MOON_ON_LIGHT)
    };
    let u = rect.width() / 24.0;
    let at = |x: f32, y: f32| Point::new(rect.left + x * u, rect.top + y * u);
    let rgb = |c: [u8; 3]| Rgba::rgb8(c, 1.0);
    let body = |c: &mut Ctx, cx: f32, cy: f32, r: f32| {
        let p = at(cx, cy);
        if night {
            c.icon(
                Rect::new(p.x - r * 1.3 * u, p.y - r * 1.3 * u, 2.6 * r * u, 2.6 * r * u),
                Icon::Moon,
                2.6 * r * u / c.s(),
                rgb(moon),
            );
            return;
        }
        c.f.fill(Rect::new(p.x - r * u, p.y - r * u, 2.0 * r * u, 2.0 * r * u), r * u, rgb(SUN));
        for i in 0..8 {
            let (sin, cos) = (i as f32 * TAU / 8.0).sin_cos();
            c.f.push(Op::Line {
                from: Point::new(p.x + cos * (r + 2.0) * u, p.y + sin * (r + 2.0) * u),
                to: Point::new(p.x + cos * (r + 4.0) * u, p.y + sin * (r + 4.0) * u),
                width: (1.8 * u).max(1.0),
                color: rgb(SUN),
            });
        }
    };
    // A cloud: a base capsule and two puffs, its left edge at x.
    let puff = |c: &mut Ctx, x: f32, y: f32, k: f32, color: [u8; 3]| {
        let k = k * u;
        let o = at(x, y);
        let ink = rgb(color);
        c.f.fill(Rect::new(o.x, o.y + 6.0 * k, 18.0 * k, 7.0 * k), 3.5 * k, ink);
        c.f.fill(Rect::new(o.x + 2.5 * k, o.y + 2.5 * k, 8.0 * k, 8.0 * k), 4.0 * k, ink);
        c.f.fill(Rect::new(o.x + 7.0 * k, o.y, 9.5 * k, 9.5 * k), 4.75 * k, ink);
    };
    match sky {
        Sky::Clear => body(c, 12.0, 12.0, if night { 6.0 } else { 5.0 }),
        Sky::PartlyCloudy => {
            body(c, 9.0, 8.5, if night { 4.5 } else { 4.0 });
            puff(c, 5.0, 8.5, 1.0, cloud);
        }
        Sky::Cloudy => {
            puff(c, 7.0, 3.5, 0.8, grey);
            puff(c, 2.0, 8.0, 1.1, cloud);
        }
        Sky::Fog => {
            puff(c, 3.0, 2.0, 1.0, grey);
            for (i, w) in [16.0, 12.0, 16.0].into_iter().enumerate() {
                let p = at(4.0, 17.0 + i as f32 * 3.0);
                c.f.fill(Rect::new(p.x, p.y, w * u, 1.6 * u), 0.8 * u, rgb(grey));
            }
        }
        Sky::Rain | Sky::Snow | Sky::Storm => {
            puff(c, 3.0, 1.5, 1.0, if sky == Sky::Storm { DARK_CLOUD } else { cloud });
            for i in 0..3 {
                let x = 7.0 + i as f32 * 4.5;
                if sky == Sky::Snow {
                    let p = at(x, 19.5);
                    c.f.fill(Rect::new(p.x - 1.4 * u, p.y - 1.4 * u, 2.8 * u, 2.8 * u), 1.4 * u, rgb(cloud));
                } else {
                    c.f.push(Op::Line {
                        from: at(x + 0.8, 16.5),
                        to: at(x - 0.8, 21.0),
                        width: (1.8 * u).max(1.0),
                        color: rgb(RAIN),
                    });
                }
            }
            if sky == Sky::Storm {
                let r = Rect::new(at(8.0, 13.0).x, at(8.0, 13.0).y, 9.0 * u, 10.0 * u);
                c.icon(r, Icon::Charging, 9.0 * u / c.s(), rgb(BOLT));
            }
        }
    }
}
