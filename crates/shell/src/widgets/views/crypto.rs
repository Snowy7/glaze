//! Markets: a row per coin (its badge, ticker and name, the price and the day's change as a
//! green or red chip), in two columns when wide, with the time of the last update.
use super::{Ctx, ITEM, price_label};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Rect, Role, TextRole};
use serde_json::Value;

pub fn draw(c: &mut Ctx, r: Rect) {
    let m = c.m;
    let coins: Vec<&Value> = m.data["coins"]
        .as_array()
        .or_else(|| m.data["items"].as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if coins.is_empty() {
        let (title, detail) = match &m.error {
            Some(_) => ("Can't reach CoinGecko", "Trying again in a few minutes"),
            None if m.list("coins").is_empty() => ("No coins set", "Add some in Settings"),
            None => ("Loading prices", ""),
        };
        let detail = (!detail.is_empty()).then_some(detail);
        return c.empty(r, Some(Icon::Gauge), title, detail);
    }
    let currency = m.str("currency", "usd");
    let at = m.data["at"].as_i64().map(|at| crate::widgets::time::Stamp::at(at).clock(true, false));
    let foot = if c.rect.height() >= c.px(140.0) + 2.0 * c.pad {
        match (&m.error, at) {
            (Some(_), Some(at)) => format!("Offline · {at}"),
            (Some(_), None) => "Offline".to_owned(),
            (None, Some(at)) => format!("Updated {at}"),
            (None, None) => String::new(),
        }
    } else {
        String::new()
    };
    let r = c.footer(r, &foot);
    let narrow = r.width() < c.px(130.0);
    let min_row = c.px(if narrow { 58.0 } else { 40.0 });
    let two = r.width() >= c.px(330.0) && (coins.len() >= 4 || coins.len() as f32 * min_row > r.height());
    let cols = if two { 2 } else { 1 };
    let gap = (r.width() * 0.06).clamp(c.px(18.0), c.px(32.0));
    let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
    let per_col = coins.len().div_ceil(cols);
    let row_h = (r.height() / per_col as f32).clamp(min_row, c.px(72.0)).floor();
    let total = per_col as f32 * row_h;
    c.scroll(r, total, |c, top| {
        let top = top + ((r.height() - total) / 2.0).max(0.0).round();
        for (i, coin) in coins.iter().enumerate() {
            let rect = Rect::new(r.left + (i / per_col) as f32 * (w + gap), top + (i % per_col) as f32 * row_h, w, row_h);
            coin_row(c, rect, i, coin, &currency, narrow);
            if i % per_col != 0 {
                c.f.separator(rect.left, rect.right, rect.top);
            }
        }
    });
}

fn coin_row(c: &mut Ctx, rect: Rect, i: usize, coin: &Value, currency: &str, narrow: bool) {
    let color = c.color();
    let s = c.s();
    let name = coin["name"].as_str().unwrap_or("Coin");
    let symbol = coin["symbol"]
        .as_str()
        .map(str::to_uppercase)
        .unwrap_or_else(|| coin["id"].as_str().unwrap_or(name).chars().take(3).collect::<String>().to_uppercase());
    let price = coin["current_price"].as_f64().or_else(|| coin["price"].as_f64());
    let change = coin["price_change_percentage_24h"].as_f64().or_else(|| coin["change"].as_f64());
    let price = price.map_or("—".to_owned(), |p| price_label(p, currency));
    c.control(ITEM + i as u32, rect, c.px(8.0), Role::Link, name, None);
    let h = rect.height() / s;
    if narrow {
        let (a, b) = (c.f.line(TextRole::Caption), (h * 0.36).clamp(14.0, 26.0) * s * 1.2);
        let y = rect.top + ((rect.height() - a - b) / 2.0).round();
        c.text(Rect::new(rect.left, y, rect.width(), a), &symbol, TextRole::Caption, color.secondary, Align::Start);
        let style = c.display((h * 0.36).clamp(14.0, 26.0), 500);
        c.styled(Rect::new(rect.left, y + a, rect.width(), b), &price, &style, color.label, Align::Start, Align::Center);
        return;
    }
    let d = (h * 0.56).clamp(24.0, 36.0).round() * s;
    let (badge, rest) = rect.take_left(d, c.px(10.0));
    let badge = badge.place(d, d, Align::Start, Align::Center);
    c.f.fill(badge, d / 2.0, color.accent_wash);
    let glyph = c.display((h * 0.28).clamp(11.0, 16.0), 650);
    c.styled(badge, &symbol.chars().next().unwrap_or('•').to_string(), &glyph, color.label, Align::Center, Align::Center);
    let price_style = c.display((h * 0.36).clamp(14.0, 22.0), 500);
    let change_w = if c.m.flag("showChange", true) && change.is_some() { c.px(64.0) } else { 0.0 };
    let pw = c.f.text.measure(&price, &price_style).0;
    let right_w = pw.max(change_w).min(rest.width() * 0.6);
    let (right, left) = rest.take_right(right_w, c.px(8.0));
    let (a, b) = (c.f.line(TextRole::Headline), c.f.line(TextRole::Caption));
    let y = left.top + ((left.height() - a - b) / 2.0).round();
    c.text(Rect::new(left.left, y, left.width(), a), &symbol, TextRole::Headline, color.label, Align::Start);
    c.text(Rect::new(left.left, y + a, left.width(), b), name, TextRole::Caption, color.secondary, Align::Start);
    let ph = (price_style.size * 1.2).round();
    let stacked = change_w > 0.0;
    let py = if stacked { right.top + ((right.height() - ph - c.px(18.0)) / 2.0).round() } else { right.top + ((right.height() - ph) / 2.0).round() };
    c.styled(Rect::new(right.left, py, right.width(), ph), &price, &price_style, color.label, Align::End, Align::Center);
    if let (true, Some(change)) = (stacked, change) {
        let up = change >= 0.0;
        let ink = if up { color.success } else { color.destructive };
        let text = format!("{}{:.2}%", if up { "▲ " } else { "▼ " }, change.abs());
        let tw = c.measure(&text, TextRole::Caption).0;
        let chip = Rect::new(right.right - tw - c.px(12.0), py + ph + c.px(1.0), tw + c.px(12.0), c.px(17.0));
        c.f.fill(chip, chip.height() / 2.0, ink.with_alpha(0.18));
        c.text(chip, &text, TextRole::Caption, ink, Align::Center);
    }
}
