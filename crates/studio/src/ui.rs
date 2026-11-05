//! Studio's building blocks over `mypaper_ui::controls` (the kit's look): the inspector column
//! and its sections, property rows (with the reset ↺ and the inherited chip), table-driven
//! setting rows, More disclosures, headers, tiles, colour swatches, banners and navigation
//! rows. Surfaces are told apart by fills and spacing, never outlines. Sizes follow
//! plans/native/studio-redesign.md, 8.2 and 8.4.

use mypaper_ui::controls::{Align, Builder, ButtonKind, Font, Glyph, Key, key};
use mypaper_ui::{Accessory, Lead, Point, Rect, Rgba, Role};
use serde_json::Value;

use crate::actions::{Action, Choices, Edit, FieldId};
use crate::app::{App, field_key, more_key};
use crate::settings::{Kind, Place, Setting};
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

/// A scrolling inspector panel at `rect`: the chrome fill (set apart from the page by it, no
/// line), and `content`.
pub fn inspector(app: &App, b: &mut B, rect: Rect, view: Key, content: impl FnOnce(&mut B, &mut Col)) {
    let p = *b.p;
    b.fill(rect, 0.0, p.chrome);
    let offset = app.nav.scroll(view);
    b.scroll(view, rect, offset, Action::None, |b, top| {
        let mut col = Col {
            x: rect.left,
            y: top + b.px(4.0),
            w: rect.width(),
            view,
        };
        content(b, &mut col);
        col.y + b.px(12.0)
    });
}

/// The padded content column of a section.
pub fn pad(b: &B, col: &Col) -> Col {
    Col {
        x: col.x + b.px(16.0),
        w: col.w - b.px(32.0),
        ..*col
    }
}

/// A section: the kit's section header, then `body` in a padded column; sections are set apart
/// by space. `right` draws in the heading's right edge (an "All 8" link, a "+ Add").
pub fn section(b: &mut B, col: &mut Col, title: &str, body: impl FnOnce(&mut B, &mut Col)) {
    let mut inner = pad(b, col);
    inner.y += b.px(14.0);
    if !title.is_empty() {
        b.section_header(Rect::new(inner.x, inner.y, inner.w, b.px(18.0)), title);
        inner.y += b.px(28.0);
    }
    body(b, &mut inner);
    col.y = inner.y + b.px(14.0);
}

/// A heading with an action at its right ("Modules … + Add").
pub fn heading_action(b: &mut B, col: &Col, y: f32, label: &str, glyph: Option<Glyph>, action: Action) {
    let w = b.button_width(label, glyph.is_some(), true);
    let rect = Rect::new(col.right() - w + b.px(8.0), y - b.px(5.0), w, b.px(28.0));
    b.button(key(("heading", label, y as i32)), rect, label, glyph, ButtonKind::Ghost, true, action);
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

/// The ↺ after a label, while the row is hovered or focused and its value is the user's own.
fn reset_mark(b: &mut B, x: f32, row: Rect, edit: &Edit) {
    let k = key(("reset", &*edit.path));
    let hot = b.input.pointer.is_some_and(|p| row.contains(p)) || b.input.hover == Some(k);
    if !hot {
        return;
    }
    let r = Rect::new(x + b.px(4.0), row.top + (row.height() - b.px(20.0)) / 2.0, b.px(20.0), b.px(20.0));
    b.icon_button(k, r, glyph::RESET, &format!("Reset {}", edit.label), false, true, Action::Unset(edit.clone()));
}

/// The height of a row with or without a hint.
pub fn row_height(b: &B, hint: &str) -> f32 {
    if hint.is_empty() { b.px(38.0) } else { b.px(44.0) }
}

/// A row from the settings table. Unknown ids draw nothing.
pub fn setting(app: &App, b: &mut B, col: &mut Col, id: &'static str) {
    let Some(s) = Setting::get(id) else {
        debug_assert!(false, "no setting {id}");
        return;
    };
    setting_with(app, b, col, s, s.label, s.hint);
}

/// A table setting under another label or hint (a page's wording).
pub fn setting_with(app: &App, b: &mut B, col: &mut Col, s: &'static Setting, title: &str, hint: &str) {
    let edit = Edit::new(s.doc, s.path, s.label);
    let value = app.value(s);
    let set = app.docs.is_set(s.doc, s.path);
    let flash = app.flash.is_some_and(|(f, _)| f == s.id);
    let top = col.y;
    let h = row_height(b, hint);
    let k = key(("setting", s.id));
    match s.kind {
        Kind::Switch { .. } => {
            let row = Rect::new(col.x, top, col.w, h);
            highlight(b, row, flash);
            let end = label(b, row, title, hint, None);
            if set {
                reset_mark(b, end, row, &edit);
            }
            let on = value.as_bool().unwrap_or(false);
            let at = Point::new(col.right() - b.px(36.0), top + (h - b.px(20.0)) / 2.0);
            b.switch(k, at, on, title, Action::Toggle(edit, on));
            col.y += h + b.px(4.0);
        }
        Kind::Choice { options, .. } => {
            let labels: Vec<&str> = options.iter().map(|(_, l)| *l).collect();
            let values: Vec<Value> = options.iter().map(|(v, _)| serde_json::from_str(v).unwrap_or(Value::Null)).collect();
            let selected = values.iter().position(|v| *v == value);
            let seg_w = b.segmented_width(&labels);
            let body = b.font(Font::Body);
            let label_w = b.measure(title, &body).0;
            let stacked = label_w + seg_w + b.px(24.0) > col.w;
            let row = Rect::new(col.x, top, col.w, if stacked { b.px(28.0) } else { h });
            highlight(b, Rect::new(col.x, top, col.w, if stacked { b.px(64.0) } else { h }), flash);
            let end = label(b, row, title, if stacked { "" } else { hint }, None);
            if set {
                reset_mark(b, end, row, &edit);
            }
            let seg = if stacked {
                Rect::new(col.x, top + b.px(30.0), col.w, b.px(32.0))
            } else {
                Rect::new(col.right() - seg_w, top + (h - b.px(30.0)) / 2.0, seg_w, b.px(30.0))
            };
            let edit2 = edit.clone();
            b.segmented(k, seg, &labels, selected, title, stacked, |i| Action::Set(edit2.clone(), values[i].clone()));
            col.y += if stacked { b.px(68.0) } else { h + b.px(4.0) };
        }
        Kind::Select { options, .. } => {
            let row = Rect::new(col.x, top, col.w, h);
            highlight(b, row, flash);
            let end = label(b, row, title, hint, None);
            if set {
                reset_mark(b, end, row, &edit);
            }
            let options: Vec<(Value, String)> = options
                .iter()
                .map(|(v, l)| (serde_json::from_str(v).unwrap_or(Value::Null), (*l).to_owned()))
                .collect();
            let shown = options
                .iter()
                .find(|(v, _)| *v == value)
                .map_or_else(|| value.to_string(), |(_, l)| l.clone());
            let w = b.select_width(&shown).max(b.px(96.0));
            let rect = Rect::new(col.right() - w, top + (h - b.px(30.0)) / 2.0, w, b.px(30.0));
            b.select(
                k,
                rect,
                &shown,
                title,
                Action::Choose(Choices {
                    edit,
                    options,
                    current: Some(value),
                }),
            );
            col.y += h + b.px(4.0);
        }
        Kind::Slider {
            min,
            max,
            step,
            unit,
            factor,
            ..
        } => {
            let row = Rect::new(col.x, top, col.w, b.px(40.0));
            highlight(b, row, flash);
            let lw = b.px(100.0);
            let end = label(b, Rect::new(col.x, top, lw, row.height()), title, "", None);
            if set {
                reset_mark(b, end.min(col.x + lw - b.px(24.0)), row, &edit);
            }
            let v = value.as_f64().unwrap_or(min);
            let shown = v * factor;
            let text = if unit == "%" {
                format!("{:.0}%", shown)
            } else if step < 1.0 && factor == 1.0 {
                format!("{shown:.1}{}{unit}", if unit.is_empty() { "" } else { " " })
            } else {
                format!("{shown:.0}{}{unit}", if unit.is_empty() { "" } else { " " })
            };
            let rect = Rect::new(col.x + lw, top, col.w - lw, row.height());
            b.slider(k, rect, v, (min, max, step), &text, b.px(44.0), title, Action::Slider(edit));
            col.y += row.height();
        }
        Kind::Text { placeholder } => {
            let row = Rect::new(col.x, top, col.w, b.px(24.0));
            highlight(b, Rect::new(col.x, top, col.w, b.px(64.0)), flash);
            let end = label(b, row, title, "", None);
            if set {
                reset_mark(b, end, row, &edit);
            }
            let rect = Rect::new(col.x, top + b.px(26.0), col.w, b.px(32.0));
            text_field(app, b, rect, FieldId::Text(s.doc, s.path.into()), None, placeholder, value.as_str().unwrap_or(""), title);
            col.y += b.px(66.0);
        }
        Kind::Custom => {}
    }
    anchor(app, s.id, col.view, top);
}

/// Remembers where a setting's row is (the palette scrolls there).
pub fn anchor(app: &App, id: &'static str, view: Key, y: f32) {
    app.anchors.borrow_mut().insert(id, (view, y));
}

/// The one-time flash a deep link gives its row.
fn highlight(b: &mut B, rect: Rect, on: bool) {
    if on {
        let r = rect.inflate(b.px(6.0));
        b.fill(Rect::new(r.left, r.top, r.width(), r.height() - b.px(8.0)), b.px(8.0), b.p.selected);
    }
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

/// A More disclosure: a 38-high row with a chevron; returns whether it's open. `detail` sits at
/// its right ("Edge from wallpaper").
pub fn more(app: &App, b: &mut B, col: &mut Col, place: Place, group: &str, detail: &str) -> bool {
    let k = more_key(place, group);
    let open = app.is_open(k);
    let p = *b.p;
    let row = Rect::new(col.x + b.px(8.0), col.y, col.w - b.px(16.0), b.px(38.0));
    b.wash(k, row, b.px(8.0));
    let chevron = if open { glyph::CHEVRON_DOWN } else { glyph::CHEVRON_RIGHT };
    b.icon(Rect::new(row.left + b.px(6.0), row.top, b.px(16.0), row.height()), chevron, 12.0, p.ink3);
    let style = b.font(Font::Strong);
    b.text_in(Rect::new(row.left + b.px(30.0), row.top, row.width() - b.px(40.0), row.height()), group, &style, p.ink2, Align::Start);
    if !detail.is_empty() {
        let caption = b.font(Font::Caption);
        b.text_in(Rect::new(row.left, row.top, row.width() - b.px(10.0), row.height()), detail, &caption, p.ink3, Align::End);
    }
    b.hit_checked(k, row, Role::Button, group, Some(open), Action::More(k), mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, row, b.px(8.0));
    col.y += row.height() + b.px(2.0);
    open
}

/// The inspector's header: an optional back link, an icon tile, the title and a detail line,
/// then icon actions at the right.
pub fn header(
    b: &mut B,
    col: &mut Col,
    back: Option<(&str, Action)>,
    glyph: Glyph,
    title: &str,
    detail: &str,
    actions: &[(Glyph, &str, Action)],
) {
    let p = *b.p;
    let x = col.x + b.px(16.0);
    if let Some((label, action)) = back {
        let style = b.font(Font::Caption);
        let (w, _) = b.measure(label, &style);
        let r = Rect::new(col.x + b.px(10.0), col.y + b.px(10.0), w + b.px(26.0), b.px(24.0));
        let k = key(("back", label));
        b.wash(k, r, b.px(6.0));
        let ink = if b.hovered(k) { p.ink1 } else { p.ink3 };
        b.icon(Rect::new(r.left + b.px(4.0), r.top, b.px(14.0), r.height()), glyph::CHEVRON_LEFT, 12.0, ink);
        b.text_in(Rect::new(r.left + b.px(20.0), r.top, w + b.px(4.0), r.height()), label, &style, ink, Align::Start);
        b.hit(k, r, Role::Link, &format!("Back to {label}"), action, mypaper_ui::controls::Behavior::Click);
        b.focus_ring(k, r, b.px(6.0));
        col.y += b.px(32.0);
    }
    let top = col.y + b.px(10.0);
    let tile = Rect::new(x, top, b.px(36.0), b.px(36.0));
    b.fill(tile, b.px(9.0), p.fill);
    b.icon(tile, glyph, 17.0, p.ink1);
    let title_style = b.font(Font::Title2);
    let caption = b.font(Font::Caption);
    let actions_w = actions.len() as f32 * b.px(32.0);
    let tw = col.w - b.px(36.0 + 12.0 + 16.0) - actions_w;
    b.text_in(Rect::new(tile.right + b.px(12.0), top - b.px(1.0), tw, b.px(22.0)), title, &title_style, p.ink1, Align::Start);
    b.text_in(Rect::new(tile.right + b.px(12.0), top + b.px(21.0), tw, b.px(16.0)), detail, &caption, p.ink3, Align::Start);
    let mut ax = col.right() - b.px(16.0) - actions_w;
    for (g, label, action) in actions {
        let r = Rect::new(ax, top + b.px(4.0), b.px(30.0), b.px(30.0));
        b.icon_button(key(("header", *label)), r, *g, label, false, true, action.clone());
        ax += b.px(32.0);
    }
    col.y = top + b.px(36.0 + 10.0);
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

/// A banner in the page: alert icon, a sentence, up to two buttons, on the card fill.
pub fn banner(b: &mut B, rect: Rect, text: &str, buttons: &[(&str, Action)]) -> f32 {
    let p = *b.p;
    let h = b.px(52.0);
    let r = Rect::new(rect.left, rect.top, rect.width(), h);
    b.fill(r, b.px(12.0), p.panel);
    b.icon(Rect::new(r.left + b.px(14.0), r.top, b.px(18.0), h), glyph::ALERT, 15.0, p.ink1);
    let mut x = r.right - b.px(12.0);
    for (label, action) in buttons.iter().rev() {
        let w = b.button_width(label, false, true);
        x -= w;
        b.button(key(("banner", *label)), Rect::new(x, r.top + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), label, None, ButtonKind::Secondary, true, action.clone());
        x -= b.px(8.0);
    }
    let style = b.font(Font::Body);
    b.text_in(Rect::new(r.left + b.px(44.0), r.top, x - r.left - b.px(52.0), h), text, &style, p.ink1, Align::Start);
    h
}

/// A file's banner, when it has one. Returns the height used.
pub fn file_banner(app: &App, b: &mut B, rect: Rect, doc: crate::docs::DocId) -> f32 {
    let Some((text, buttons)) = banner_for(&app.docs, doc) else {
        return 0.0;
    };
    banner(b, rect, &text, &buttons) + b.px(12.0)
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

/// A card (panel fill, radius 12, no edge) at `rect`.
pub fn card(b: &mut B, rect: Rect) {
    b.card(rect, b.px(12.0));
}

/// A centered empty state: icon tile, title, line, and an optional button.
pub fn empty(b: &mut B, rect: Rect, glyph: Glyph, title: &str, line: &str, button: Option<(&str, Action)>) {
    let p = *b.p;
    let cx = (rect.left + rect.right) / 2.0;
    let mut y = rect.top + rect.height() / 2.0 - b.px(70.0);
    let tile = Rect::new(cx - b.px(22.0), y, b.px(44.0), b.px(44.0));
    b.fill(tile, b.px(12.0), p.fill);
    b.icon(tile, glyph, 20.0, p.ink1);
    y += b.px(56.0);
    let strong = b.font(Font::Heading);
    b.text_in(Rect::new(rect.left, y, rect.width(), b.px(22.0)), title, &strong, p.ink1, Align::Center);
    y += b.px(24.0);
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(rect.left, y, rect.width(), b.px(18.0)), line, &caption, p.ink3, Align::Center);
    y += b.px(30.0);
    if let Some((label, action)) = button {
        let w = b.button_width(label, true, false);
        b.button(key(("empty", label)), Rect::new(cx - w / 2.0, y, w, b.px(32.0)), label, Some(glyph::ADD), ButtonKind::Primary, true, action);
    }
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
