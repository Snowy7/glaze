use super::{Command, active, edit, raw};
use crate::{
    actions::{Action, FieldId},
    app::App,
    panel::{
        Page,
        controls::{self, button, group, notice, text, title},
        editors,
    },
    theme::glyph,
    ui::{B, Col},
};
use glaze_ui::controls::{Align, Behavior, key};
use glaze_ui::{Rect, Role, TextStyle};
use serde_json::{Value, json};

pub fn action(command: Command) -> Action {
    Action::Panel(crate::panel::Command::Builder(command))
}
fn line(b: &mut B, c: &mut Col, label: &str, detail: &str, action: Action) {
    controls::row(b, c, None, label, detail, "", None, action);
}
fn text_field(app: &App, b: &mut B, c: &mut Col, path: String, label: &str, value: &str) {
    editors::field(
        app,
        b,
        c,
        FieldId::Text(active(app).unwrap().doc, path.into()),
        label,
        "",
        value,
    );
}
fn slider(
    app: &App,
    b: &mut B,
    c: &mut Col,
    path: String,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
) {
    let h = b.px(22.0);
    text(
        b,
        Rect::new(c.x, c.y, c.w, h),
        label,
        12.0,
        500,
        b.p.ink2,
        Align::Start,
    );
    c.y += h;
    let r = Rect::new(c.x, c.y, c.w, b.px(32.0));
    let shown = if step >= 1.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    b.slider(
        key(("builder-slider", &path)),
        r,
        value,
        (min, max, step),
        &shown,
        b.px(45.0),
        label,
        Action::Slider(edit(app, path, label)),
    );
    c.y = r.bottom + b.px(7.0);
}
fn options(
    app: &App,
    b: &mut B,
    c: &mut Col,
    path: String,
    label: &str,
    value: Value,
    values: &[&str],
) {
    editors::select(
        b,
        c,
        label,
        value,
        edit(app, path, label),
        values.iter().map(|v| (json!(v), v.to_string())).collect(),
    );
}
fn color(app: &App, b: &mut B, c: &mut Col, path: String, label: &str, value: &Value) {
    let hex = value.as_str().unwrap_or("#ffffff");
    let r = Rect::new(c.x, c.y, c.w, b.px(38.0));
    text(
        b,
        Rect::new(r.left, r.top, r.width() - b.px(40.0), r.height()),
        label,
        13.0,
        500,
        b.p.ink1,
        Align::Start,
    );
    let swatch = Rect::new(
        r.right - b.px(30.0),
        r.top + b.px(6.0),
        b.px(26.0),
        b.px(26.0),
    );
    let fill = glaze_config::Color::parse(hex)
        .map(|v| glaze_ui::Rgba::rgb8(v.0, 1.0))
        .unwrap_or(b.p.ink1);
    b.fill(swatch, b.px(8.0), fill);
    b.stroke(swatch, b.px(8.0), b.px(1.0), b.p.edge);
    b.hit(
        key(("builder-color", &path)),
        r,
        Role::Button,
        &format!("{label} {hex}"),
        Action::Panel(crate::panel::Command::PickColor(edit(app, path, label))),
        Behavior::Click,
    );
    c.y = r.bottom;
}
fn pair(b: &mut B, c: &mut Col, left: (&str, Action), right: (&str, Action)) {
    let gap = b.px(8.0);
    let w = (c.w - gap) / 2.0;
    let h = b.px(34.0);
    button(
        b,
        Rect::new(c.x, c.y, w, h),
        left.0,
        None,
        false,
        true,
        left.1,
    );
    button(
        b,
        Rect::new(c.x + w + gap, c.y, w, h),
        right.0,
        None,
        false,
        true,
        right.1,
    );
    c.y += h + b.px(10.0);
}

/// The gallery card's hit key; the editor's tick compares it with the hovered key.
pub fn effect_key(id: &str) -> glaze_ui::controls::Key {
    key(("builder-effect", id))
}

const CATEGORIES: [(&str, &str); 6] = [
    ("weather", "Weather"),
    ("camera", "Camera"),
    ("light", "Light"),
    ("color", "Color"),
    ("motion", "Motion"),
    ("stylize", "Stylize"),
];

/// The editor's one panel: header, background, layers, cost. The effect gallery replaces it
/// as a sheet; the shader page edits code.
pub fn page(app: &App, b: &mut B, c: &mut Col, page: Page) {
    let Some(editor) = active(app) else {
        title(b, c, "Create wallpaper", "");
        line(b, c, "Start creating", "", action(Command::New));
        return;
    };
    let value = raw(app);
    if page == Page::Effects {
        gallery(app, b, c);
        return;
    }
    if page == Page::Shader {
        shader_panel(app, b, c, &value);
        return;
    }
    header(app, b, c, &value);
    if editor.fresh {
        start(app, b, c);
        return;
    }
    background(app, b, c, &value["wallpaper"]["base"]);
    layers(app, b, c, &value);
    footer(app, b, c, &value);
}

fn small_text(b: &mut B, r: Rect, label: &str, ink: glaze_ui::Rgba) {
    text(b, r, label, 12.0, 400, ink, Align::Start);
}

/// A 28 DIP icon button with a hover plate.
fn icon_button(b: &mut B, r: Rect, k: glaze_ui::controls::Key, glyph: glaze_ui::controls::Glyph, ink: glaze_ui::Rgba, label: &str, act: Action) {
    if b.hovered(k) {
        b.fill(r, b.px(8.0), b.p.hover);
    }
    b.icon(r, glyph, 14.0, ink);
    b.hit(k, r, Role::Button, label, act, Behavior::Click);
    b.focus_ring(k, r, b.px(8.0));
}

fn more_glyph() -> glaze_ui::controls::Glyph {
    glaze_ui::icons::Icon::More.into()
}

/// A full-width menu row (overflow menus and pickers).
fn menu_row(b: &mut B, c: &mut Col, k: glaze_ui::controls::Key, glyph: Option<glaze_ui::controls::Glyph>, label: &str, detail: &str, act: Action) {
    let r = Rect::new(c.x, c.y, c.w, b.px(34.0));
    if b.hovered(k) {
        b.fill(r, b.px(8.0), b.p.hover);
    }
    let mut x = r.left + b.px(10.0);
    if let Some(glyph) = glyph {
        b.icon(Rect::new(x, r.top, b.px(18.0), r.height()), glyph, 14.0, b.p.ink2);
        x += b.px(28.0);
    }
    text(b, Rect::new(x, r.top, r.right - x - b.px(10.0), r.height()), label, 13.0, 500, b.p.ink1, Align::Start);
    if !detail.is_empty() {
        text(b, Rect::new(x, r.top, r.right - x - b.px(10.0), r.height()), detail, 12.0, 400, b.p.ink3, Align::End);
    }
    b.hit(k, r, Role::Button, label, act, Behavior::Click);
    b.focus_ring(k, r, b.px(8.0));
    c.y = r.bottom;
}

/// Name (edited in place), save state, Apply / Save / overflow.
fn header(app: &App, b: &mut B, c: &mut Col, value: &Value) {
    let editor = active(app).unwrap();
    let name = value["name"].as_str().unwrap_or("");
    let r = Rect::new(c.x, c.y, c.w, b.px(40.0));
    crate::ui::text_field(
        app,
        b,
        r,
        FieldId::Text(editor.doc, "/name".into()),
        None,
        "Untitled wallpaper",
        name,
        "Wallpaper name",
    );
    c.y = r.bottom + b.px(4.0);
    let state = match &editor.saved {
        Some(saved) if *saved == *value => "Saved",
        Some(_) => "Unsaved changes",
        None if editor.origin.is_none() && !editor.fresh => "New wallpaper · not saved yet",
        None => "New wallpaper",
    };
    let dot = Rect::new(c.x, c.y + b.px(8.0), b.px(6.0), b.px(6.0));
    b.fill(dot, b.px(3.0), if state == "Saved" { b.p.ink3 } else { b.p.ink1 });
    small_text(b, Rect::new(c.x + b.px(12.0), c.y, c.w - b.px(12.0), b.px(22.0)), state, b.p.ink3);
    c.y += b.px(32.0);
    let h = b.px(34.0);
    let more = Rect::new(c.right() - h, c.y, h, h);
    let gap = b.px(8.0);
    let save_w = b.px(84.0);
    let save = Rect::new(more.left - gap - save_w, c.y, save_w, h);
    let apply = Rect::new(c.x, c.y, save.left - gap - c.x, h);
    let ready = !editor.fresh;
    button(b, apply, "Apply", None, true, ready, action(Command::Save(true)));
    button(b, save, "Save", None, false, ready, action(Command::Save(false)));
    let mk = key("builder-more");
    b.fill(more, b.control_radius(more), if b.hovered(mk) || app.is_open(mk) { b.p.hover } else { b.p.fill });
    b.stroke(more, b.control_radius(more), b.px(1.0), b.p.line);
    b.icon(more, more_glyph(), 15.0, b.p.ink1);
    b.hit(mk, more, Role::Button, "More actions", Action::More(mk), Behavior::Click);
    b.focus_ring(mk, more, b.control_radius(more));
    c.y = more.bottom + b.px(6.0);
    if app.is_open(mk) {
        let top = c.y;
        let at = b.list.ops.len();
        c.y += b.px(4.0);
        let mut inner = Col { x: c.x + b.px(4.0), w: c.w - b.px(8.0), ..*c };
        menu_row(b, &mut inner, key("builder-more-copy"), Some(glyph::COPY), "Save as copy", "", action(Command::SaveCopy));
        menu_row(b, &mut inner, key("builder-more-revert"), Some(glyph::UNDO), "Revert to original", "", action(Command::Revert));
        menu_row(b, &mut inner, key("builder-more-discard"), Some(glyph::DELETE), "Discard draft", "", action(Command::Discard));
        c.y = inner.y + b.px(4.0);
        b.list.ops.insert(at, glaze_ui::Op::Fill { rect: Rect::new(c.x, top, c.w, c.y - top), radius: b.px(12.0), color: b.p.panel });
        c.y += b.px(4.0);
    }
}

/// A new draft: what to start from.
fn start(app: &App, b: &mut B, c: &mut Col) {
    c.y += b.px(18.0);
    text(b, Rect::new(c.x, c.y, c.w, b.px(24.0)), "Start from", 16.0, 600, b.p.ink1, Align::Start);
    c.y += b.px(28.0);
    small_text(b, Rect::new(c.x, c.y, c.w, b.px(20.0)), "A photo, a video, a colour, or a wallpaper you have.", b.p.ink3);
    c.y += b.px(30.0);
    let gap = b.px(10.0);
    let w = (c.w - gap) / 2.0;
    let h = b.px(112.0);
    let tiles = [
        (glyph::IMAGE, "Photo", "JPG, PNG, WebP", action(Command::Pick("photo"))),
        (glyph::VIDEO, "Video", "MP4, WebM", action(Command::Pick("video"))),
        (crate::theme::glyph::LOOK, "Colour", "Solid or gradient", action(Command::Base("color"))),
        (glyph::DESKTOP, "Wallpaper", "One you have", Action::More(key("builder-change"))),
    ];
    for (i, (icon, label, detail, act)) in tiles.into_iter().enumerate() {
        let r = Rect::new(c.x + (i % 2) as f32 * (w + gap), c.y + (i / 2) as f32 * (h + gap), w, h);
        let k = key(("builder-start", i));
        // An icon in place of a picture, the label under it.
        let pic = Rect::new(r.left, r.top, r.width(), r.height() - b.px(42.0));
        controls::card_picture(b, pic, b.hovered(k), |b, pic| b.icon(pic, icon, 20.0, b.p.ink1));
        text(b, Rect::new(r.left, pic.bottom + b.px(6.0), r.width(), b.px(20.0)), label, 13.0, 600, b.p.ink1, Align::Start);
        small_text(b, Rect::new(r.left, pic.bottom + b.px(24.0), r.width(), b.px(18.0)), detail, b.p.ink3);
        b.hit(k, r, Role::Button, &format!("Start from {label}"), act, Behavior::Click);
        b.focus_ring(k, r, b.px(12.0));
    }
    c.y += 2.0 * h + gap + b.px(14.0);
    line(b, c, "Gradient, shader or 3D scene", "", Action::More(key("builder-change")));
    if app.is_open(key("builder-change")) {
        picker(app, b, c);
    }
}

/// What the base is, in words a person uses.
fn base_label(app: &App, base: &Value) -> (glaze_ui::controls::Glyph, String, String) {
    let file = base["source"].as_str().and_then(|s| s.rsplit('/').next()).unwrap_or("").to_owned();
    match base["kind"].as_str().unwrap_or("color") {
        "image" => (glyph::IMAGE, "Photo".into(), file),
        "video" => (glyph::VIDEO, "Video".into(), file),
        "gradient" => (crate::theme::glyph::LOOK, "Gradient".into(), String::new()),
        "shader" => (glyph::SHADER, "Shader".into(), file),
        "glsl" => (glyph::SHADER, "Shader".into(), "Shadertoy".into()),
        "scene" => (glyph::SCENE, "3D scene".into(), file),
        "theme" => {
            let wallpaper = theme_item(app, base);
            (
                glyph::WALLPAPERS,
                wallpaper.map_or_else(|| base["theme"].as_str().unwrap_or("Wallpaper").to_owned(), |w| w.name.clone()),
                wallpaper.and_then(|w| w.kind).map_or("Wallpaper", crate::library::Kind::label).to_owned(),
            )
        }
        _ => (crate::theme::glyph::LOOK, "Colour".into(), base["color"].as_str().unwrap_or("").to_uppercase()),
    }
}

/// The library entry a `theme` base names.
fn theme_item<'a>(app: &'a App, base: &Value) -> Option<&'a crate::library::Wallpaper> {
    let id = base["theme"].as_str()?;
    app.library.items.iter().find(|w| w.apply_as == id || w.id == id)
}

/// The base's picture: its image, its video's still, or the wallpaper's preview.
fn base_picture(app: &App, base: &Value) -> Option<std::path::PathBuf> {
    let editor = active(app)?;
    match base["kind"].as_str()? {
        "image" | "video" => Some(editor.dir.join(base["source"].as_str()?)),
        "theme" => theme_item(app, base).and_then(|w| w.preview.clone().or(w.video.clone())),
        _ => None,
    }
}

fn swatch(base: &Value, fallback: glaze_ui::Rgba) -> glaze_ui::Rgba {
    base["color"]
        .as_str()
        .or_else(|| base["stops"][0]["color"].as_str())
        .and_then(glaze_config::Color::parse)
        .map(|v| glaze_ui::Rgba::rgb8(v.0, 1.0))
        .unwrap_or(fallback)
}

fn background(app: &App, b: &mut B, c: &mut Col, base: &Value) {
    let kind = base["kind"].as_str().unwrap_or("color").to_owned();
    group(b, c, "Background", |b, c| {
        let (icon, label, detail) = base_label(app, base);
        let row = Rect::new(c.x, c.y, c.w, b.px(52.0));
        let thumb = Rect::new(row.left, row.top + b.px(2.0), b.px(80.0), b.px(48.0));
        b.fill(thumb, b.px(8.0), swatch(base, b.p.well));
        match base_picture(app, base).and_then(|p| app.images.borrow_mut().get(&p, (thumb.width() as u32, thumb.height() as u32))) {
            Some(image) => crate::stage::image_round(b, thumb, &image, 1.0, b.px(8.0)),
            None if !matches!(kind.as_str(), "color" | "gradient") => b.icon(thumb, icon, 18.0, b.p.ink2),
            None => {}
        }
        b.stroke(thumb, b.px(8.0), b.px(1.0), b.p.line);
        let tx = thumb.right + b.px(12.0);
        let change = Rect::new(row.right - b.px(76.0), row.top + b.px(11.0), b.px(76.0), b.px(30.0));
        text(b, Rect::new(tx, row.top + b.px(6.0), change.left - tx - b.px(8.0), b.px(20.0)), &label, 13.0, 600, b.p.ink1, Align::Start);
        small_text(b, Rect::new(tx, row.top + b.px(26.0), change.left - tx - b.px(8.0), b.px(18.0)), &detail, b.p.ink3);
        let ck = key("builder-change");
        b.button(ck, change, if app.is_open(ck) { "Done" } else { "Change" }, None, glaze_ui::controls::ButtonKind::Secondary, true, Action::More(ck));
        c.y = row.bottom + b.px(10.0);
        if app.is_open(ck) {
            picker(app, b, c);
            return;
        }
        match kind.as_str() {
            "image" | "video" => {
                fit(b, c, base["fit"].as_str().unwrap_or("cover"));
                if kind == "image" {
                    anchor(b, c, &base["focus"]);
                }
            }
            "color" => color(app, b, c, "/wallpaper/base/color".into(), "Colour", &base["color"]),
            "gradient" => gradient(app, b, c, base),
            "shader" | "glsl" => {
                let source = active(app).unwrap().dir.join(base["source"].as_str().unwrap_or(""));
                if let Ok(source) = std::fs::read_to_string(source) {
                    params(app, b, c, &raw(app), "/wallpaper/base/params", &super::shader_params(&source));
                }
                line(b, c, "Edit shader code", "", action(Command::Shader));
            }
            "scene" => scene(app, b, c, base),
            "theme" => small_text(b, Rect::new(c.x, c.y, c.w, b.px(20.0)), "Shown exactly as the original. Add layers on top.", b.p.ink3),
            _ => {}
        }
        if kind == "theme" {
            c.y += b.px(22.0);
        }
    });
}

/// One clear list of what a background can be.
fn picker(app: &App, b: &mut B, c: &mut Col) {
    for (i, (icon, label, act)) in [
        (glyph::IMAGE, "Photo", action(Command::Pick("photo"))),
        (glyph::VIDEO, "Video", action(Command::Pick("video"))),
        (crate::theme::glyph::LOOK, "Colour", action(Command::Base("color"))),
        (crate::theme::glyph::LOOK, "Gradient", action(Command::Base("gradient"))),
        (glyph::SHADER, "Shader", action(Command::Base("shader"))),
        (glyph::SCENE, "3D scene", action(Command::Pick("scene"))),
    ]
    .into_iter()
    .enumerate()
    {
        menu_row(b, c, key(("builder-pick", i)), Some(icon), label, "", act);
    }
    c.y += b.px(10.0);
    small_text(b, Rect::new(c.x, c.y, c.w, b.px(20.0)), "A wallpaper you have", b.p.ink3);
    c.y += b.px(24.0);
    for index in super::base_choices(app) {
        let w = &app.library.items[index];
        menu_row(b, c, key(("builder-pick-theme", index)), Some(glyph::WALLPAPERS), &w.name, w.kind.map_or("", crate::library::Kind::label), action(Command::BaseTheme(index)));
    }
}

/// Fill / Fit / Stretch / Center, each drawn as a tiny screen and picture.
fn fit(b: &mut B, c: &mut Col, current: &str) {
    text(b, Rect::new(c.x, c.y, c.w, b.px(20.0)), "Fit", 12.0, 500, b.p.ink2, Align::Start);
    c.y += b.px(24.0);
    let values = [("cover", "Fill"), ("contain", "Fit"), ("stretch", "Stretch"), ("center", "Center")];
    let r = Rect::new(c.x, c.y, c.w, b.px(62.0));
    b.fill(r, b.px(10.0), b.p.fill);
    let w = r.width() / 4.0;
    for (i, (fit, label)) in values.into_iter().enumerate() {
        let cell = Rect::new(r.left + i as f32 * w, r.top, w, r.height()).inflate(-b.px(3.0));
        let k = key(("builder-fit", i));
        let on = current == fit;
        if on {
            b.fill(cell, b.px(8.0), b.p.panel);
            b.stroke(cell, b.px(8.0), b.px(1.0), b.p.line);
        } else if b.hovered(k) {
            b.fill(cell, b.px(8.0), b.p.hover);
        }
        let screen = Rect::new(cell.left + (cell.width() - b.px(30.0)) / 2.0, cell.top + b.px(9.0), b.px(30.0), b.px(19.0));
        let ink = if on { b.p.ink1 } else { b.p.ink3 };
        let picture = match fit {
            "cover" => screen.inflate(b.px(3.0)),
            "contain" => Rect::new(screen.left + b.px(7.0), screen.top, screen.width() - b.px(14.0), screen.height()),
            "stretch" => screen,
            _ => Rect::new(screen.left + b.px(9.0), screen.top + b.px(5.0), screen.width() - b.px(18.0), screen.height() - b.px(10.0)),
        };
        let clip = Rect::new(picture.left.max(screen.left), picture.top.max(screen.top), picture.right.min(screen.right) - picture.left.max(screen.left), picture.bottom.min(screen.bottom) - picture.top.max(screen.top));
        b.fill(clip, b.px(2.0), glaze_ui::Rgba { a: 0.45, ..ink });
        b.stroke(screen, b.px(3.0), b.px(1.2), ink);
        text(b, Rect::new(cell.left, cell.bottom - b.px(24.0), cell.width(), b.px(20.0)), label, 12.0, if on { 600 } else { 500 }, if on { b.p.ink1 } else { b.p.ink2 }, Align::Center);
        b.hit(k, cell, Role::RadioButton, label, action(Command::Fit(fit)), Behavior::Click);
        b.focus_ring(k, cell, b.px(8.0));
    }
    c.y = r.bottom + b.px(14.0);
}

/// Which part of the photo stays in view: a 3x3 anchor.
fn anchor(b: &mut B, c: &mut Col, focus: &Value) {
    let (fx, fy) = (focus[0].as_f64().unwrap_or(0.5), focus[1].as_f64().unwrap_or(0.5));
    let size = b.px(66.0);
    let r = Rect::new(c.right() - size, c.y, size, size * 0.66);
    text(b, Rect::new(c.x, c.y, c.w - size - b.px(12.0), b.px(20.0)), "Keep in view", 12.0, 500, b.p.ink2, Align::Start);
    small_text(b, Rect::new(c.x, c.y + b.px(20.0), c.w - size - b.px(12.0), b.px(20.0)), "The part of the photo that stays when it's cropped.", b.p.ink3);
    b.fill(r, b.px(8.0), b.p.fill);
    let (cw, ch) = (r.width() / 3.0, r.height() / 3.0);
    for y in 0..3u8 {
        for x in 0..3u8 {
            let cell = Rect::new(r.left + f32::from(x) * cw, r.top + f32::from(y) * ch, cw, ch);
            let on = (fx * 2.0).round() as u8 == x && (fy * 2.0).round() as u8 == y;
            let k = key(("builder-anchor", x, y));
            let dot = b.px(if on { 8.0 } else { 4.0 });
            let d = Rect::new(cell.left + (cw - dot) / 2.0, cell.top + (ch - dot) / 2.0, dot, dot);
            b.fill(d, dot / 2.0, if on { b.p.ink1 } else if b.hovered(k) { b.p.ink2 } else { b.p.ink3 });
            b.hit(k, cell, Role::RadioButton, "Keep this part in view", action(Command::Focus(x, y)), Behavior::Click);
        }
    }
    c.y = r.bottom.max(c.y + b.px(44.0)) + b.px(8.0);
}

fn gradient(app: &App, b: &mut B, c: &mut Col, base: &Value) {
    let shape = usize::from(base["type"] == "radial");
    let r = Rect::new(c.x, c.y, c.w, b.px(30.0));
    b.segmented(key("builder-gradient-shape"), r, &["Linear", "Radial"], Some(shape), "Shape", true, |i| {
        Action::Set(edit(app, "/wallpaper/base/type", "Shape"), json!(if i == 0 { "linear" } else { "radial" }))
    });
    c.y = r.bottom + b.px(12.0);
    if let Some(stops) = base["stops"].as_array() {
        for (i, stop) in stops.iter().enumerate() {
            color(app, b, c, format!("/wallpaper/base/stops/{i}/color"), &format!("Colour {}", i + 1), &stop["color"]);
            slider(app, b, c, format!("/wallpaper/base/stops/{i}/at"), "Where it sits", stop["at"].as_f64().unwrap_or(0.0), 0.0, 1.0, 0.01);
        }
    }
    if base["type"] != "radial" {
        slider(app, b, c, "/wallpaper/base/angle".into(), "Angle", base["angle"].as_f64().unwrap_or(120.0), 0.0, 360.0, 1.0);
    }
    pair(b, c, ("Add colour", action(Command::GradientStop(true))), ("Remove colour", action(Command::GradientStop(false))));
}

fn scene(app: &App, b: &mut B, c: &mut Col, base: &Value) {
    let on = base["interactive"].as_bool().unwrap_or(false);
    editors::toggle(b, c, "Follow the mouse", "", on, Action::Toggle(edit(app, "/wallpaper/base/interactive", "Follow the mouse"), on));
    slider(app, b, c, "/wallpaper/base/orbit".into(), "Camera turn speed", base["orbit"].as_f64().unwrap_or(0.0), 0.0, 30.0, 0.1);
    slider(app, b, c, "/wallpaper/base/parallax".into(), "Depth with the mouse", base["parallax"].as_f64().unwrap_or(0.3), 0.0, 1.0, 0.01);
    slider(app, b, c, "/wallpaper/base/exposure".into(), "Brightness", base["exposure"].as_f64().unwrap_or(1.0), 0.0, 5.0, 0.05);
    options(app, b, c, "/wallpaper/base/environment".into(), "Backdrop", base["environment"].clone(), &["sky", "color", "transparent"]);
}

fn effect_of<'a>(app: &'a App, layer: &Value) -> Option<&'a super::Effect> {
    active(app)?.effects.iter().find(|e| layer["effect"].as_str() == Some(&e.id))
}

/// Figma-style rows; the selected one opens its settings underneath.
fn layers(app: &App, b: &mut B, c: &mut Col, value: &Value) {
    let editor = active(app).unwrap();
    c.y += b.px(20.0);
    let head = Rect::new(c.x, c.y, c.w, b.px(28.0));
    text(b, head, "Layers", 16.0, 600, b.p.ink1, Align::Start);
    let add = Rect::new(head.right - b.px(72.0), head.top, b.px(72.0), head.height());
    b.button(key("builder-add-layer"), add, "Add", Some(glyph::ADD), glaze_ui::controls::ButtonKind::Secondary, true, action(Command::Gallery));
    c.y = head.bottom + b.px(8.0);
    let list = super::layers(app);
    let top = c.y;
    let at = b.list.ops.len();
    let mut inner = Col { x: c.x + b.px(6.0), y: c.y + b.px(6.0), w: c.w - b.px(12.0), view: c.view };
    if list.is_empty() {
        let r = Rect::new(inner.x, inner.y, inner.w, b.px(64.0));
        text(b, Rect::new(r.left, r.top + b.px(12.0), r.width(), b.px(20.0)), "Nothing on top yet", 13.0, 600, b.p.ink1, Align::Center);
        small_text(b, Rect::new(r.left, r.top + b.px(32.0), r.width(), b.px(20.0)), "", b.p.ink3);
        text(b, Rect::new(r.left, r.top + b.px(32.0), r.width(), b.px(20.0)), "Add rain, light, colour or motion.", 12.0, 400, b.p.ink3, Align::Center);
        inner.y = r.bottom;
    }
    // Top of the list is drawn last, like every layer panel: the newest layer is on top.
    for i in (0..list.len()).rev() {
        let layer = &list[i];
        let selected = editor.selected == Some(i);
        layer_row(app, b, &mut inner, i, layer, selected);
        if selected {
            layer_settings(app, b, &mut inner, i, layer);
        }
    }
    c.y = inner.y + b.px(6.0);
    b.list.ops.insert(at, glaze_ui::Op::Fill { rect: Rect::new(c.x, top, c.w, c.y - top), radius: b.px(b.kit.radius.card), color: b.p.panel });
    let _ = value;
}

fn layer_row(app: &App, b: &mut B, c: &mut Col, i: usize, layer: &Value, selected: bool) {
    let r = Rect::new(c.x, c.y, c.w, b.px(48.0));
    let row_key = key(("builder-layer", i));
    if selected {
        b.fill(r, b.px(10.0), b.p.fill);
    } else if b.hovered(row_key) {
        b.fill(r, b.px(10.0), b.p.hover);
    }
    let on = layer["enabled"].as_bool().unwrap_or(true);
    let handle = Rect::new(r.left, r.top, b.px(22.0), r.height());
    b.icon(handle, glyph::GRIP, 12.0, b.p.ink3);
    let thumb = Rect::new(handle.right + b.px(2.0), r.top + b.px(8.0), b.px(32.0), b.px(32.0));
    b.fill(thumb, b.px(7.0), b.p.well);
    let effect = effect_of(app, layer);
    let picture = match layer["kind"].as_str() {
        Some("effect") => effect.map(|e| e.dir.join("preview.jpg")),
        Some("image") => layer["source"].as_str().map(|s| active(app).unwrap().dir.join(s)),
        _ => None,
    };
    match picture.and_then(|p| app.images.borrow_mut().get(&p, (thumb.width() as u32, thumb.height() as u32))) {
        Some(image) => crate::stage::image_round(b, thumb, &image, if on { 1.0 } else { 0.4 }, b.px(7.0)),
        None => b.icon(thumb, if layer["kind"] == "model" { glyph::SCENE } else { glyph::LAYERS }, 14.0, b.p.ink2),
    }
    let name = layer_name(app, layer);
    let eye = Rect::new(r.right - b.px(66.0), r.top + b.px(10.0), b.px(28.0), b.px(28.0));
    let menu = Rect::new(r.right - b.px(34.0), r.top + b.px(10.0), b.px(28.0), b.px(28.0));
    let tx = thumb.right + b.px(10.0);
    text(b, Rect::new(tx, r.top + b.px(6.0), eye.left - tx - b.px(4.0), b.px(20.0)), &name, 13.0, 600, if on { b.p.ink1 } else { b.p.ink3 }, Align::Start);
    let caption = match (layer["kind"].as_str(), effect) {
        (Some("effect"), Some(e)) => format!("{} · {}", capitalize(e.category()), if e.animated { "Moving" } else { "Still" }),
        (Some("effect"), None) => "Effect missing".into(),
        (Some("image"), _) => "Picture".into(),
        _ => "3D model".into(),
    };
    small_text(b, Rect::new(tx, r.top + b.px(25.0), eye.left - tx - b.px(4.0), b.px(18.0)), &if on { caption } else { "Hidden".into() }, b.p.ink3);
    b.hit(row_key, Rect::new(handle.right, r.top, eye.left - handle.right, r.height()), Role::Button, &format!("{name} settings"), action(Command::Select(if selected { None } else { Some(i) })), Behavior::Click);
    b.focus_ring(row_key, r, b.px(10.0));
    b.hit(key(("builder-drag", i)), handle, Role::Button, "Drag to reorder", action(Command::Drag(i)), Behavior::Drag);
    icon_button(b, eye, key(("builder-eye", i)), glyph::EYE, if on { b.p.ink1 } else { b.p.ink3 }, if on { "Hide layer" } else { "Show layer" },
        Action::Toggle(edit(app, format!("/wallpaper/layers/{i}/enabled"), if on { "Hide layer" } else { "Show layer" }), on));
    if !on {
        b.fill(Rect::new(eye.left + b.px(7.0), eye.top + b.px(13.5), b.px(14.0), b.px(1.5)), 0.0, b.p.ink3);
    }
    let mk = key(("builder-layer-menu", i));
    icon_button(b, menu, mk, more_glyph(), b.p.ink2, "Layer actions", Action::More(mk));
    c.y = r.bottom + b.px(2.0);
    if app.is_open(mk) {
        let top = c.y;
        let at = b.list.ops.len();
        let mut inner = Col { x: c.x + b.px(28.0), y: c.y + b.px(4.0), w: c.w - b.px(28.0), view: c.view };
        let last = super::layers(app).len().saturating_sub(1);
        // The panel lists the top layer first, so "up" moves a layer later in the stack.
        menu_row(b, &mut inner, key(("builder-lm-dup", i)), Some(glyph::COPY), "Duplicate", "", action(Command::DuplicateAt(i)));
        if i < last {
            menu_row(b, &mut inner, key(("builder-lm-up", i)), Some(crate::theme::glyph::CHEVRON_RIGHT), "Move up", "", action(Command::MoveAt(i, 1)));
        }
        if i > 0 {
            menu_row(b, &mut inner, key(("builder-lm-down", i)), Some(crate::theme::glyph::CHEVRON_DOWN), "Move down", "", action(Command::MoveAt(i, -1)));
        }
        menu_row(b, &mut inner, key(("builder-lm-del", i)), Some(glyph::DELETE), "Remove", "Del", action(Command::RemoveAt(i)));
        c.y = inner.y + b.px(4.0);
        b.list.ops.insert(at, glaze_ui::Op::Fill { rect: Rect::new(c.x + b.px(24.0), top, c.w - b.px(24.0), c.y - top), radius: b.px(10.0), color: b.p.fill });
        c.y += b.px(4.0);
    }
}

/// The selected layer's settings, in plain words, directly under its row.
fn layer_settings(app: &App, b: &mut B, c: &mut Col, i: usize, layer: &Value) {
    let prefix = format!("/wallpaper/layers/{i}");
    let mut inner = Col { x: c.x + b.px(12.0), y: c.y + b.px(6.0), w: c.w - b.px(24.0), view: c.view };
    let c2 = &mut inner;
    if let Some(effect) = effect_of(app, layer) {
        if !effect.description.is_empty() {
            let r = Rect::new(c2.x, c2.y, c2.w, b.px(34.0));
            b.text_in(r, &effect.description, &controls::font(b, 12.0, 400), b.p.ink3, Align::Start);
            c2.y = r.bottom + b.px(6.0);
        }
        params(app, b, c2, &raw(app), &format!("{prefix}/params"), &effect.schemas());
    } else if layer["kind"] == "effect" {
        notice(b, c2, "This effect isn't installed. The layer is kept as it is.");
    } else {
        for (j, label) in ["Left", "Top", "Width", "Height"].into_iter().enumerate() {
            slider(app, b, c2, format!("{prefix}/rect/{j}"), label, layer["rect"][j].as_f64().unwrap_or(0.5), 0.0, 1.0, 0.01);
        }
        if layer["kind"] == "model" {
            slider(app, b, c2, format!("{prefix}/orbit"), "Turn speed", layer["orbit"].as_f64().unwrap_or(0.0), 0.0, 30.0, 0.1);
            slider(app, b, c2, format!("{prefix}/scale"), "Size", layer["scale"].as_f64().unwrap_or(1.0), 0.0, 5.0, 0.01);
        }
    }
    slider(app, b, c2, format!("{prefix}/mix"), "Strength", layer["mix"].as_f64().unwrap_or(1.0), 0.0, 1.0, 0.01);
    let more = key(("builder-layer-more", i));
    let r = Rect::new(c2.x, c2.y, c2.w, b.px(30.0));
    b.icon(Rect::new(r.left, r.top, b.px(14.0), r.height()), if app.is_open(more) { glyph::CHEVRON_DOWN } else { crate::theme::glyph::CHEVRON_RIGHT }, 11.0, b.p.ink3);
    text(b, Rect::new(r.left + b.px(20.0), r.top, r.width() - b.px(20.0), r.height()), "More", 12.0, 500, b.p.ink2, Align::Start);
    b.hit(more, r, Role::Button, "More settings", Action::More(more), Behavior::Click);
    c2.y = r.bottom + b.px(4.0);
    if app.is_open(more) {
        editors::select(b, c2, "Blend", layer.get("blend").cloned().unwrap_or(json!("normal")), edit(app, format!("{prefix}/blend"), "Blend"),
            [("normal", "Normal"), ("add", "Add (brighter)"), ("screen", "Screen (lighter)"), ("multiply", "Multiply (darker)")].iter().map(|(v, l)| (json!(v), (*l).to_owned())).collect());
        text_field(app, b, c2, format!("{prefix}/id"), "Layer name", layer["id"].as_str().unwrap_or(""));
        if layer["kind"] == "effect" {
            line(b, c2, "Edit effect code", "WGSL", action(Command::Shader));
        }
    }
    c.y = inner.y + b.px(10.0);
    b.hline(c.x + b.px(10.0), c.right() - b.px(10.0), c.y - b.px(4.0), b.p.line);
}

/// What it costs, in words; frame rate and quality live under it.
fn footer(app: &App, b: &mut B, c: &mut Col, value: &Value) {
    c.y += b.px(18.0);
    let cost = super::cost_line(app);
    let r = Rect::new(c.x, c.y, c.w, b.px(24.0));
    b.icon(Rect::new(r.left, r.top, b.px(16.0), r.height()), glyph::GAUGE, 13.0, b.p.ink2);
    text(b, Rect::new(r.left + b.px(24.0), r.top, r.width() - b.px(24.0), r.height()), &cost, 12.0, 500, b.p.ink2, Align::Start);
    c.y = r.bottom + b.px(4.0);
    let k = key("builder-performance");
    let p = Rect::new(c.x, c.y, c.w, b.px(28.0));
    text(b, Rect::new(p.left + b.px(24.0), p.top, p.width(), p.height()), if app.is_open(k) { "Hide frame rate and quality" } else { "Frame rate and quality" }, 12.0, 400, b.p.ink3, Align::Start);
    b.hit(k, p, Role::Button, "Frame rate and quality", Action::More(k), Behavior::Click);
    c.y = p.bottom;
    if app.is_open(k) {
        let mut inner = Col { x: c.x + b.px(24.0), w: c.w - b.px(24.0), ..*c };
        slider(app, b, &mut inner, "/wallpaper/fps".into(), "Frames per second while moving", value["wallpaper"]["fps"].as_f64().unwrap_or(30.0), 1.0, 120.0, 1.0);
        slider(app, b, &mut inner, "/wallpaper/scale".into(), "Quality while moving", value["wallpaper"]["scale"].as_f64().unwrap_or(1.0), 0.25, 1.0, 0.05);
        line(b, &mut inner, "Credits for imported files", "Edit", action(Command::Credits));
        c.y = inner.y;
    }
}

/// The effect gallery: a sheet over the panel. Hovering a card previews it on the canvas.
fn gallery(app: &App, b: &mut B, c: &mut Col) {
    let editor = active(app).unwrap();
    let head = Rect::new(c.x, c.y, c.w, b.px(36.0));
    text(b, head, "Add a layer", 20.0, 700, b.p.ink1, Align::Start);
    icon_button(b, Rect::new(head.right - b.px(32.0), head.top + b.px(4.0), b.px(28.0), b.px(28.0)), key("builder-gallery-close"), glyph::CLOSE, b.p.ink1, "Close (Esc)", action(Command::CloseGallery));
    c.y = head.bottom;
    small_text(b, Rect::new(c.x, c.y, c.w, b.px(20.0)), "Point at one to try it on your wallpaper.", b.p.ink3);
    c.y += b.px(30.0);
    let search = Rect::new(c.x, c.y, c.w, b.px(32.0));
    crate::ui::text_field(app, b, search, FieldId::EffectSearch, Some(glyph::SEARCH), "Search effects", "", "Search effects");
    b.stroke(search, b.control_radius(search), b.px(1.0), b.p.edge);
    c.y = search.bottom + b.px(10.0);
    // Category chips wrap to the panel's width.
    let font = controls::font(b, 12.0, 500);
    let (mut x, mut y) = (c.x, c.y);
    let chips: Vec<(Option<&str>, &str)> = std::iter::once((None, "All")).chain(CATEGORIES.iter().map(|(id, label)| (Some(*id), *label))).collect();
    for (id, label) in chips {
        let w = b.measure(label, &font).0 + b.px(22.0);
        if x + w > c.right() {
            x = c.x;
            y += b.px(34.0);
        }
        let r = Rect::new(x, y, w, b.px(28.0));
        let on = editor.category.as_deref() == id;
        let k = key(("builder-chip", label));
        b.fill(r, r.height() / 2.0, if on { b.p.solid } else if b.hovered(k) { b.p.hover } else { b.p.fill });
        text(b, r, label, 12.0, 500, if on { b.p.on_solid } else { b.p.ink1 }, Align::Center);
        b.hit(k, r, Role::RadioButton, label, action(Command::Category(id.map(str::to_owned))), Behavior::Click);
        b.focus_ring(k, r, r.height() / 2.0);
        x = r.right + b.px(6.0);
    }
    c.y = y + b.px(42.0);
    let query = app.text(&FieldId::EffectSearch).to_lowercase();
    let shown: Vec<_> = editor
        .effects
        .iter()
        .filter(|e| editor.category.as_deref().is_none_or(|cat| e.category() == cat))
        .filter(|e| query.is_empty() || format!("{} {} {}", e.name, e.category(), e.description).to_lowercase().contains(&query))
        .collect();
    let gap = b.px(10.0);
    let w = (c.w - gap) / 2.0;
    let pic_h = (w * 0.62).round();
    let card_h = pic_h + b.px(52.0);
    for (n, effect) in shown.iter().enumerate() {
        let r = Rect::new(c.x + (n % 2) as f32 * (w + gap), c.y + (n / 2) as f32 * (card_h + gap), w, card_h);
        let k = effect_key(&effect.id);
        let previewing = editor.hover.as_deref() == Some(effect.id.as_str());
        // Off-screen cards ask for no pictures: the decoded cache stays a screenful.
        let visible = b.clip_rect().is_none_or(|clip| r.bottom >= clip.top && r.top <= clip.bottom);
        let pic = Rect::new(r.left, r.top, w, pic_h);
        controls::card_picture(b, pic, b.hovered(k), |b, pic| {
            match visible.then(|| app.images.borrow_mut().get(&effect.dir.join("preview.jpg"), (pic.width() as u32, pic.height() as u32))).flatten() {
                Some(image) => crate::stage::image_round(b, pic, &image, 1.0, 0.0),
                None => b.icon(pic, glyph::LAYERS, 20.0, b.p.ink3),
            }
        });
        text(b, Rect::new(r.left, pic.bottom + b.px(8.0), w, b.px(20.0)), &effect.name, 13.0, 600, b.p.ink1, Align::Start);
        small_text(b, Rect::new(r.left, pic.bottom + b.px(28.0), w, b.px(18.0)), if effect.animated { "Moving" } else { "Still" }, b.p.ink3);
        b.hit(k, r, Role::Button, &format!("Add {}", effect.name), action(Command::AddEffect(effect.id.clone())), Behavior::Click);
        b.focus_ring(k, r, b.px(12.0));
        if previewing {
            controls::card_selection(b, pic);
        }
    }
    c.y += shown.len().div_ceil(2) as f32 * (card_h + gap);
    if shown.is_empty() {
        notice(b, c, "No effects match. Try another word or All.");
    }
    group(b, c, "Your own", |b, c| {
        line(b, c, "Picture or 3D model", "Import", action(Command::Import(true)));
        line(b, c, "Write an effect", "WGSL", action(Command::CustomEffect));
    });
}

fn shader_panel(app: &App, b: &mut B, c: &mut Col, value: &Value) {
    let editor = active(app).unwrap();
    title(b, c, "Shader", "Changes appear as you type.");
    let filename = editor.shader.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    group(b, c, "Source", |b, c| {
        notice(b, c, &filename);
        line(b, c, "Edit in external editor", "Open", action(Command::External));
        line(b, c, "Back to the wallpaper", "Esc", action(Command::CloseGallery));
    });
    group(b, c, "Problems", |b, c| {
        if editor.diagnostics.is_empty() {
            notice(b, c, "No problems found");
        }
        for error in &editor.diagnostics {
            notice(b, c, &format!("Line {} · {}", error.line, error.message));
        }
    });
    let schemas = super::shader_params(&editor.source);
    if !schemas.is_empty() {
        group(b, c, "Settings", |b, c| {
            let prefix = param_prefix(editor.selected);
            params(app, b, c, value, &prefix, &schemas);
        });
    }
    if value["wallpaper"]["base"]["kind"] == "glsl" && editor.selected.is_none() {
        group(b, c, "Shader credits", |b, c| {
            for (id, label) in [("author", "Author"), ("license", "License"), ("url", "Source URL")] {
                text_field(app, b, c, format!("/wallpaper/base/{id}"), label, value["wallpaper"]["base"][id].as_str().unwrap_or(""));
            }
        });
    }
}
fn param_prefix(index: Option<usize>) -> String {
    index
        .map(|i| format!("/wallpaper/layers/{i}/params"))
        .unwrap_or_else(|| "/wallpaper/base/params".into())
}
fn layer_name(app: &App, layer: &Value) -> String {
    // The contract's id is also the editable layer label; no extra name field is written.
    layer["id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            active(app)?
                .effects
                .iter()
                .find(|e| layer["effect"].as_str() == Some(&e.id))
                .map(|e| e.name.clone())
        })
        .unwrap_or_else(|| layer["id"].as_str().unwrap_or("Layer").to_owned())
}
fn params(app: &App, b: &mut B, c: &mut Col, value: &Value, prefix: &str, schemas: &[Value]) {
    for schema in schemas {
        let Some(id) = schema["id"].as_str() else {
            continue;
        };
        let label = schema["label"].as_str().unwrap_or(id);
        let path = format!("{prefix}/{}", id.replace('~', "~0").replace('/', "~1"));
        let own = value.pointer(&path);
        let current = own.unwrap_or(&schema["default"]);
        if own.is_some() && !super::same_param(current, &schema["default"]) {
            let reset = Rect::new(c.right() - b.px(22.0), c.y, b.px(22.0), b.px(22.0));
            b.icon(reset, glyph::UNDO, 12.0, b.p.ink3);
            b.hit(
                key(("builder-reset", &path)),
                reset,
                Role::Button,
                &format!("Reset {label}"),
                Action::Unset(edit(app, path.clone(), &format!("Reset {label}"))),
                Behavior::Click,
            );
        }
        match schema["type"].as_str().unwrap_or("float") {
            "color" => color(app, b, c, path, label, current),
            "bool" => editors::toggle(
                b,
                c,
                label,
                "",
                current.as_bool().unwrap_or(false),
                Action::Toggle(edit(app, path, label), current.as_bool().unwrap_or(false)),
            ),
            "enum" => {
                let options: Vec<_> = schema["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                options_fn(app, b, c, path, label, current.clone(), &options);
            }
            _ => slider(
                app,
                b,
                c,
                path,
                label,
                current.as_f64().unwrap_or(0.0),
                schema["min"].as_f64().unwrap_or(0.0),
                schema["max"].as_f64().unwrap_or(1.0),
                schema["step"].as_f64().unwrap_or(0.01),
            ),
        }
    }
}
// A separate name avoids shadowing the generated enum options.
fn options_fn(
    app: &App,
    b: &mut B,
    c: &mut Col,
    path: String,
    label: &str,
    value: Value,
    values: &[&str],
) {
    options(app, b, c, path, label, value, values);
}

/// The canvas: the engine's live frame at the desktop's own shape, as large as the stage allows.
pub fn stage(app: &App, b: &mut B, area: Rect) -> bool {
    let page = crate::panel::current_page(app);
    if !super::is_page(page) {
        return false;
    }
    let Some(editor) = active(app) else {
        return true;
    };
    if page == Page::Shader {
        shader(app, b, area);
        return true;
    }
    let value = raw(app);
    let canvas = crate::stage::frame(
        app,
        Rect::new(
            area.left + b.px(28.0),
            area.top + b.px(24.0),
            area.width() - b.px(56.0),
            area.height() - b.px(88.0),
        ),
    );
    let radius = b.px(14.0);
    let base = &value["wallpaper"]["base"];
    if editor.fresh {
        b.fill(canvas, radius, b.p.well);
        b.stroke(canvas, radius, b.px(1.0), b.p.line);
        let mid = canvas.top + canvas.height() / 2.0;
        b.icon(Rect::new(canvas.left, mid - b.px(52.0), canvas.width(), b.px(32.0)), glyph::WALLPAPERS, 26.0, b.p.ink3);
        text(b, Rect::new(canvas.left, mid - b.px(10.0), canvas.width(), b.px(26.0)), "Your new wallpaper", 17.0, 600, b.p.ink1, Align::Center);
        text(b, Rect::new(canvas.left, mid + b.px(18.0), canvas.width(), b.px(22.0)), "Start from a photo, a video, a colour, or an existing wallpaper.", 13.0, 400, b.p.ink3, Align::Center);
        return true;
    }
    let snapshot = editor.dir.join("preview.jpg");
    let engine_frame = snapshot.is_file();
    // Until the engine's first frame: the base's own picture, then its colour.
    let path = engine_frame.then_some(snapshot).or_else(|| match base["kind"].as_str() {
        Some("image") => base["source"].as_str().map(|p| editor.dir.join(p)),
        Some("theme") => theme_item(app, base).and_then(|w| w.preview.clone()),
        _ => None,
    });
    let image = path.as_ref().and_then(|p| {
        app.images
            .borrow_mut()
            .get(p, (canvas.width() as u32, canvas.height() as u32))
    });
    b.fill(canvas, radius, swatch(base, b.p.well));
    if let Some(image) = image {
        crate::stage::image_round(b, canvas, &image, 1.0, radius);
    }
    b.stroke(canvas, radius, b.px(1.0), b.p.edge);
    let (mw, mh) = app
        .display()
        .map_or((1920, 1080), |m| (m.rect.width, m.rect.height));
    let previewing = editor
        .hover
        .as_ref()
        .and_then(|id| editor.effects.iter().find(|e| &e.id == id))
        .map(|e| e.name.clone());
    let status = if let Some(name) = previewing {
        format!("Trying {name} · click to add")
    } else if !editor.diagnostics.is_empty() {
        "Preview paused · the shader has a problem".into()
    } else if app.fixture || engine_frame {
        format!("Live preview · {mw} × {mh}")
    } else if editor.preview.error.is_some() {
        "Preview unavailable · update Glaze".into()
    } else {
        "Starting the preview…".into()
    };
    let r = Rect::new(canvas.left, canvas.bottom + b.px(14.0), canvas.width(), b.px(22.0));
    let w = b.measure(&status, &controls::font(b, 12.0, 500)).0;
    let dot = Rect::new(r.left + (r.width() - w) / 2.0 - b.px(14.0), r.top + b.px(8.0), b.px(6.0), b.px(6.0));
    b.fill(dot, b.px(3.0), if engine_frame || app.fixture { b.p.ink2 } else { b.p.ink3 });
    text(b, r, &status, 12.0, 500, b.p.ink2, Align::Center);
    true
}

fn shader(app: &App, b: &mut B, area: Rect) {
    let editor = active(app).unwrap();
    let r = Rect::new(
        area.left + b.px(8.0),
        area.top + b.px(12.0),
        area.width() - b.px(16.0),
        area.height() - b.px(24.0),
    );
    b.fill(r, b.px(20.0), b.p.panel);
    let filename = editor
        .shader
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();
    text(
        b,
        Rect::new(
            r.left + b.px(20.0),
            r.top + b.px(12.0),
            r.width() - b.px(40.0),
            b.px(30.0),
        ),
        &filename,
        13.0,
        600,
        b.p.ink1,
        Align::Start,
    );
    b.hline(
        r.left + b.px(16.0),
        r.right - b.px(16.0),
        r.top + b.px(52.0),
        b.p.line,
    );
    let code = Rect::new(
        r.left + b.px(58.0),
        r.top + b.px(69.0),
        r.width() - b.px(78.0),
        r.height() - b.px(92.0),
    );
    let mono = TextStyle::new("Cascadia Mono", b.px(12.0));
    let row = b.px(22.0);
    let max = (code.height() / row) as usize;
    for (i, source) in editor.source.lines().take(max).enumerate() {
        let y = code.top + i as f32 * row;
        let error = editor.diagnostics.iter().any(|e| e.line == i + 1);
        let number = Rect::new(r.left + b.px(12.0), y, b.px(32.0), row);
        b.text_in(
            number,
            &(i + 1).to_string(),
            &mono,
            if error { b.p.ink1 } else { b.p.ink3 },
            Align::End,
        );
        if error {
            b.fill(
                Rect::new(r.left + b.px(5.0), y + b.px(6.0), b.px(3.0), b.px(10.0)),
                b.px(1.5),
                b.p.ink1,
            );
        }
        if app
            .field
            .as_ref()
            .is_none_or(|f| f.id != FieldId::ShaderCode)
        {
            b.text_in(
                Rect::new(code.left, y, code.width(), row),
                source,
                &mono,
                if source.trim().starts_with("//") {
                    b.p.ink3
                } else {
                    b.p.ink1
                },
                Align::Start,
            );
            if error {
                b.hline(code.left, code.right, y + row - b.px(2.0), b.p.ink2);
            }
        }
    }
    let k = crate::app::field_key(&FieldId::ShaderCode);
    b.hit(
        k,
        code,
        Role::TextInput,
        "Shader source",
        Action::Field(FieldId::ShaderCode),
        Behavior::Click,
    );
    if let Some(field) = app.field.as_ref().filter(|f| f.id == FieldId::ShaderCode) {
        field.field.set_rect(code);
        field.field.set_style(&mono, b.p.ink1);
        b.list.ops.push(glaze_ui::Op::TextField {
            rect: code,
            field: field.field.clone(),
            revision: field.field.revision(),
        });
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
