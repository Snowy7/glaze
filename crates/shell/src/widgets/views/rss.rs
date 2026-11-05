//! Headlines: each a headline over its source and age, as rows or cards, scrolling; a quiet
//! header and a footer when feeds fail.
use super::{Ctx, ITEM, relative_age};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Rect, Role, TextRole};
use serde_json::Value;

pub fn draw(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let m = c.m;
    let items: Vec<&Value> = m.data["items"]
        .as_array()
        .map(|a| a.iter().take(m.num("maxItems", 12.0) as usize).collect())
        .unwrap_or_default();
    let r = c.header(r, Icon::Feed, "Headlines");
    if items.is_empty() {
        let (title, detail) = match &m.error {
            Some(e) => ("Couldn't load headlines", e.clone()),
            None => ("Loading", "Fetching your feeds…".to_owned()),
        };
        return c.empty(r, Some(Icon::Feed), title, Some(&detail));
    }
    let failures = m.data["errors"].as_array().map_or(0, Vec::len);
    let foot = if m.error.is_some() {
        "Showing saved headlines".to_owned()
    } else if failures > 0 {
        format!("{failures} feed{} unavailable", if failures == 1 { "" } else { "s" })
    } else {
        String::new()
    };
    let r = c.footer(r, &foot);
    let cards = m.str("variant", "list") == "cards";
    let wide = r.width() >= c.px(380.0);
    let title_role = if wide { TextRole::Headline } else { TextRole::Callout };
    let title_line = c.f.line(title_role);
    let meta_line = c.f.line(TextRole::Caption);
    let pad_y = c.px(if cards { 10.0 } else { 7.0 });
    let gap = c.px(if cards { 6.0 } else { 2.0 });
    let inset = c.px(if cards { 12.0 } else { 8.0 });
    // Heights: each headline wraps to at most two lines.
    let style = c.f.font(title_role);
    let heights: Vec<f32> = items
        .iter()
        .map(|item| {
            let title = item["title"].as_str().unwrap_or("Headline");
            let n = c.f.text.lines(title, &style, r.width() - 2.0 * inset, 2).len().max(1);
            n as f32 * title_line + meta_line + c.px(2.0) + 2.0 * pad_y
        })
        .collect();
    let total: f32 = heights.iter().sum::<f32>() + gap * (items.len() - 1) as f32;
    let radius = c.inner_radius().min(c.px(12.0));
    c.scroll(r, total, |c, top| {
        let mut y = top;
        for (i, item) in items.iter().enumerate() {
            let rect = Rect::new(r.left, y, r.width(), heights[i]);
            let title = item["title"].as_str().unwrap_or("Headline");
            if cards {
                c.f.fill(rect, radius, color.card);
            }
            c.control(ITEM + i as u32, rect, radius, Role::Link, title, None);
            let inner = rect.inset(mypaper_ui::Insets::xy(inset, pad_y));
            let used = c.f.paragraph(inner.left, inner.top, inner.width(), title, title_role, color.label, 2);
            let mut meta = Vec::new();
            if m.flag("showSource", true)
                && let Some(source) = item["source"].as_str().filter(|s| !s.is_empty())
            {
                meta.push(source.to_owned());
            }
            if m.flag("showTime", true)
                && let Some(at) = item["published"].as_i64()
            {
                meta.push(relative_age(at, c.now));
            }
            let at = Rect::new(inner.left, inner.top + used + c.px(2.0), inner.width(), meta_line);
            c.text(at, &meta.join(" · "), TextRole::Caption, color.secondary, Align::Start);
            y += heights[i] + gap;
        }
    });
}

