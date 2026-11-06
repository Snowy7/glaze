//! Shared Studio columns, manifest fields, picture tiles and list rows.

use mypaper_ui::controls::{Align, Builder, Font, Glyph, Key, key};
use mypaper_ui::{Accessory, Lead, Rect, Rgba, Role};
use serde_json::Value;

use crate::actions::{Action, Edit, FieldId};
use crate::app::{App, field_key};
use crate::theme::glyph;

pub type B<'a> = Builder<'a, Action>;

/// A column being filled top to bottom, in device pixels.
#[derive(Debug, Clone, Copy)]
pub struct Col {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    /// The scroll view it's in (for deep links to its rows).
    pub view: Key,
}

impl Col {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
}

/// A property row's label (and hint) at the left of `rect`; returns where the label ends.
pub fn label(b: &mut B, rect: Rect, label: &str, hint: &str, chip: Option<&str>) -> f32 {
    let p = *b.p;
    let body = b.font(Font::Body);
    let caption = b.font(Font::Caption);
    let (lw, lh) = b.measure(label, &body);
    let hint_h = if hint.is_empty() { 0.0 } else { b.px(17.0) };
    let top = rect.top + (rect.height() - lh.max(b.px(20.0)) - hint_h) / 2.0;
    b.text_at(rect.left, top, label, &body, p.ink1);
    let mut end = rect.left + lw;
    if let Some(chip) = chip {
        let style = b.font(Font::Micro);
        let (cw, _) = b.measure(chip, &style);
        let r = Rect::new(end + b.px(8.0), top + b.px(1.0), cw + b.px(12.0), b.px(18.0));
        b.fill(r, b.px(5.0), p.fill);
        b.text_in(r, chip, &style, p.ink2, Align::Center);
        end = r.right;
    }
    if !hint.is_empty() {
        b.text_in(
            Rect::new(rect.left, top + b.px(20.0), rect.width(), b.px(16.0)),
            hint,
            &caption,
            p.ink3,
            Align::Start,
        );
    }
    end
}

/// Remembers where a setting's row is (the palette scrolls there).
pub fn anchor(app: &App, id: &'static str, view: Key, y: f32) {
    app.anchors.borrow_mut().insert(id, (view, y));
}

/// A text field: the live RichEdit when it has focus, else its text (or a placeholder).
#[allow(clippy::too_many_arguments)]
pub fn text_field(
    app: &App,
    b: &mut B,
    rect: Rect,
    id: FieldId,
    glyph: Option<Glyph>,
    placeholder: &str,
    shown: &str,
    label: &str,
) {
    let live = app.field.as_ref().filter(|f| f.id == id);
    let k = field_key(&id);
    let shown = if shown.is_empty() { app.text(&id) } else { shown };
    b.field(k, rect, glyph, placeholder, shown.is_empty() && live.is_none(), live.is_some(), label, Action::Field(id.clone()));
    let value = live.and_then(|field| field.field.text().ok()).unwrap_or_else(|| shown.to_owned());
    b.semantics(k, mypaper_ui::Semantics::Text(value.into()));
    let inner = b.field_inner(rect, glyph.is_some());
    match live {
        Some(f) => {
            f.field.set_rect(inner);
            b.list.ops.push(mypaper_ui::Op::TextField {
                rect: inner,
                field: f.field.clone(),
                revision: f.field.revision(),
            });
        }
        None if !shown.is_empty() => {
            let style = b.font(Font::Body);
            b.text_in(inner, shown, &style, b.p.ink1, Align::Start);
        }
        None => {}
    }
}

/// A grid of picture tiles: each draws its picture through `draw`, gets its frame and label.
/// `columns` across `col.w`, 16:10 (or square) pictures.
#[allow(clippy::too_many_arguments)]
pub fn tiles(
    b: &mut B,
    col: &mut Col,
    group: Key,
    columns: usize,
    square: bool,
    items: &[(&str, bool, Action)],
    mut draw: impl FnMut(&mut B, usize, Rect),
) {
    let p = *b.p;
    let gap = b.px(10.0);
    let w = (col.w - gap * (columns as f32 - 1.0)) / columns as f32;
    let h = if square { w } else { w * 10.0 / 16.0 };
    let label_style = b.font(Font::Caption);
    let rows = items.len().div_ceil(columns);
    b.group(group, |b| {
        for (i, (label, selected, action)) in items.iter().enumerate() {
            let (r, c) = (i / columns, i % columns);
            let x = col.x + c as f32 * (w + gap);
            let y = col.y + r as f32 * (h + b.px(32.0));
            let pic = Rect::new(x, y, w, h);
            let k = key((group, i));
            let pad = b.px(4.0);
            b.plate(k, Rect::new(x - pad, y - pad, w + pad * 2.0, h + b.px(24.0) + pad * 2.0), b.px(8.0) + pad, *selected);
            draw(b, i, pic);
            b.text_in(
                Rect::new(x, y + h + b.px(6.0), w, b.px(18.0)),
                label,
                &label_style,
                if *selected { p.ink1 } else { p.ink3 },
                Align::Center,
            );
            b.hit_checked(k, Rect::new(x, y, w, h + b.px(24.0)), Role::RadioButton, label, Some(*selected), action.clone(), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, pic, b.px(8.0));
        }
    });
    col.y += rows as f32 * (h + b.px(32.0));
}

/// A row of colour swatches: "From wallpaper" first (when offered), then presets, then the
/// custom colour (a field for hex). `current` is the stored value.
#[allow(clippy::too_many_arguments)]
pub fn swatches(
    app: &App,
    b: &mut B,
    col: &mut Col,
    edit: &Edit,
    current: Option<&Value>,
    from_wallpaper: Option<Rgba>,
    presets: &[[u8; 3]],
) {
    if app.panel.is_some() {
        crate::panel::swatches(app, b, col, edit, current, from_wallpaper.is_some(), false, presets);
        return;
    }
    let p = *b.p;
    let d = b.px(24.0);
    let gap = b.px(8.0);
    let mut x = col.x + b.px(2.0);
    let y = col.y + b.px(4.0);
    let group = key(("swatches", &*edit.path));
    let current_hex = current.and_then(Value::as_str).unwrap_or("");
    b.group(group, |b| {
        if let Some(auto) = from_wallpaper {
            let r = Rect::new(x, y, d, d);
            let on = current_hex == "auto";
            b.fill(r, d / 2.0, auto);
            swatch_frame(b, r, auto, on, p);
            let k = key((group, "auto"));
            b.hit_checked(k, r, Role::RadioButton, "From wallpaper", Some(on), Action::Set(edit.clone(), Value::String("auto".into())), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, d / 2.0);
            x += d + gap;
        }
        for c in presets {
            let r = Rect::new(x, y, d, d);
            let hex = mypaper_config::Color(*c).hex();
            let on = current_hex.eq_ignore_ascii_case(&hex);
            let color = Rgba::rgb8(*c, 1.0);
            b.fill(r, d / 2.0, color);
            swatch_frame(b, r, color, on, p);
            let k = key((group, hex.as_str()));
            b.hit_checked(k, r, Role::RadioButton, &hex, Some(on), Action::Set(edit.clone(), Value::String(hex.clone())), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, d / 2.0);
            x += d + gap;
        }
    });
    // The custom colour: a hex field.
    let custom = current_hex.starts_with('#')
        && !presets.iter().any(|c| mypaper_config::Color(*c).hex().eq_ignore_ascii_case(current_hex));
    let fw = (col.right() - x).max(b.px(84.0)).min(b.px(110.0));
    let rect = Rect::new(col.right() - fw, y - b.px(4.0), fw, b.px(32.0));
    let id = FieldId::Hex(edit.path.clone());
    let shown = if custom { current_hex } else { "" };
    text_field(app, b, rect, id, None, "Custom", shown, &format!("{} hex", edit.label));
    col.y += b.px(34.0);
}

/// A swatch's selection: a check inside it in whichever of black or white reads on the
/// colour (macOS's accent picker), never a ring. A swatch too close to the inspector behind it
/// (a near-white on light) gets a faint edge so it doesn't vanish.
fn swatch_frame(b: &mut B, r: Rect, color: Rgba, on: bool, p: mypaper_ui::controls::Palette) {
    if mypaper_ui::theme::contrast(color, p.chrome) < 1.35 {
        b.stroke(r, r.width() / 2.0, 1.0, p.edge);
    }
    if on {
        let ink = if mypaper_ui::theme::luminance(color) > 0.4 { Rgba::new(0.0, 0.0, 0.0, 0.85) } else { Rgba::new(1.0, 1.0, 1.0, 1.0) };
        b.icon(r, mypaper_ui::icons::Icon::Check, 13.0, ink);
    }
}

/// What a file's banner says and offers. A layout saved by a newer mypaper offers only Restart:
/// nothing destructive. A file that can't be read offers Show file and Start over (which asks
/// first and keeps a timestamped backup).
pub fn banner_for(docs: &crate::docs::Docs, doc: crate::docs::DocId) -> Option<(String, Vec<(&'static str, Action)>)> {
    let error = docs.error(doc)?;
    if docs.newer(doc) {
        return Some((crate::docs::NEWER.to_owned(), vec![("Restart", Action::RestartStudio)]));
    }
    let first = error.lines().next().unwrap_or(error);
    Some((
        format!("{} can't be read: {first}", doc.label()),
        vec![("Show file", Action::ShowFile(doc)), ("Start over", Action::StartOver(doc))],
    ))
}

/// A navigation row: the kit's list row (icon badge, title, detail, chevron); the whole row is
/// one button.
pub fn nav_row(b: &mut B, col: &mut Col, glyph: Glyph, title: &str, detail: &str, action: Action) {
    let k = key(("nav", title));
    let row = Rect::new(col.x - b.px(8.0), col.y, col.w + b.px(16.0), b.px(48.0));
    let lead = match glyph {
        Glyph::Icon(icon) => Lead::Badge(icon, false),
        Glyph::Font(_) => Lead::None,
    };
    b.row(k, row, lead, title, Some(detail), Accessory::Chevron, false, action);
    col.y += row.height() + b.px(2.0);
}

/// A sortable list row: grip, icon, label, trailing remove on hover; Alt+arrows move it. Rows
/// sit on their list's fill; an inset hairline parts them.
#[allow(clippy::too_many_arguments)]
pub fn list_row(
    b: &mut B,
    rect: Rect,
    k: Key,
    glyph: Option<Glyph>,
    label: &str,
    select: Action,
    remove: Option<Action>,
    last: bool,
) {
    let p = *b.p;
    let hot = b.hovered(k) || b.input.pointer.is_some_and(|pt| rect.contains(pt));
    if hot {
        b.fill(rect.inflate(-b.px(3.0)), b.px(7.0), p.hover);
    }
    b.icon(Rect::new(rect.left + b.px(4.0), rect.top, b.px(16.0), rect.height()), glyph::GRIP, 12.0, if hot { p.ink3 } else { p.ink4 });
    let mut x = rect.left + b.px(24.0);
    if let Some(g) = glyph {
        b.icon(Rect::new(x, rect.top, b.px(18.0), rect.height()), g, 14.0, p.ink2);
        x += b.px(26.0);
    }
    if !last {
        b.hline(x, rect.right - b.px(10.0), rect.bottom - 1.0, p.line);
    }
    let style = b.font(Font::Body);
    b.text_in(Rect::new(x, rect.top, rect.right - x - b.px(36.0), rect.height()), label, &style, p.ink1, Align::Start);
    b.hit(k, Rect::new(rect.left, rect.top, rect.width() - b.px(34.0), rect.height()), Role::ListItem, label, select, mypaper_ui::controls::Behavior::Drag);
    b.focus_ring(k, rect.inflate(-b.px(2.0)), b.px(6.0));
    if let Some(remove) = remove
        && hot
    {
        let r = Rect::new(rect.right - b.px(32.0), rect.top + (rect.height() - b.px(28.0)) / 2.0, b.px(28.0), b.px(28.0));
        b.icon_button(key((k, "remove")), r, glyph::DELETE, &format!("Remove {label}"), false, true, remove);
    }
}
