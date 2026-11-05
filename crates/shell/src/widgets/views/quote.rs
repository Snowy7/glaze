//! A quote: the words as large as they fit (13 to 24 px, down to 11), the author under them
//! after a short dash. A click shows another.
use super::{Ctx, PRIMARY};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Rect, Role, TextRole};
use serde_json::Value;

pub fn draw(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let m = c.m;
    let builtins: Vec<Value> = serde_json::from_str(include_str!("../quotes.json")).unwrap_or_default();
    let source = m.str("source", "builtin");
    let mut quotes = if source == "custom" { Vec::new() } else { builtins };
    if source != "builtin" {
        quotes.extend_from_slice(m.list("custom"));
    }
    quotes.retain(|q| q["text"].as_str().is_some_and(|t| !t.trim().is_empty()));
    c.hit(PRIMARY, r, Role::Button, "Next quote");
    if quotes.is_empty() {
        return c.empty(r, Some(Icon::Message), "Your words", Some("Add quotes in Studio"));
    }
    let period = match m.str("mode", "daily").as_str() {
        "click" => 0,
        "hourly" => c.now.date.ordinal() * 24 + i64::from(c.now.hour),
        _ => c.now.date.ordinal(),
    };
    let index = (period.rem_euclid(quotes.len() as i64) as usize + m.quote_offset) % quotes.len();
    let q = &quotes[index];
    let text = q["text"].as_str().unwrap_or("");
    let author = q["author"].as_str().unwrap_or("").trim();
    let r = r.inset(mypaper_ui::Insets::xy(c.px(2.0), c.px(2.0)));
    let author_h = if author.is_empty() { 0.0 } else { c.f.line(TextRole::Caption) + c.px(8.0) };
    let room = r.height() - author_h;
    let (w, h) = (r.width() / s, r.height() / s);
    let mut size = (w / 14.0).min(h / 3.0).clamp(13.0, 24.0);
    let (style, lines) = loop {
        let style = c.display(size, 450);
        let line = (style.size * 1.32).round();
        let max = (room / line).floor().max(1.0) as usize;
        let lines = c.f.text.lines(text, &style, r.width(), max + 1);
        if lines.len() <= max || size <= 11.0 {
            let fitted = c.f.text.lines(text, &style, r.width(), max);
            break (style, fitted);
        }
        size -= 0.5;
    };
    let line = (style.size * 1.32).round();
    let total = line * lines.len() as f32 + author_h;
    let mut y = r.top + ((r.height() - total) / 2.0).max(0.0).round();
    for l in &lines {
        c.styled(Rect::new(r.left, y, r.width(), line), l, &style, color.label, Align::Start, Align::Center);
        y += line;
    }
    if !author.is_empty() {
        let at = Rect::new(r.left, y + c.px(8.0), r.width(), author_h - c.px(8.0));
        let dash = c.px(14.0);
        c.f.fill(Rect::new(at.left, (at.center().y).round(), dash, 1.0_f32.max(s.floor())), 0.0, color.tertiary);
        c.text(Rect { left: at.left + dash + c.px(8.0), ..at }, &author.to_uppercase(), TextRole::Caption, color.secondary, Align::Start);
    }
}
