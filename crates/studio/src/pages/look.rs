//! Look: one editor for everything mypaper draw    let seg_w = b.segmented_width(&items); (the Desktop tab: style, material, colour,
//! text and icons, shape, "Different for") and how Windows draws its own windows (the Windows
//! tab, D2). The stage is a specimen built from real views, or the display's own layout. The
//! scope switch edits your look, or the look the wallpaper brings (your own wallpapers'; a
//! built-in's needs a copy).

use std::path::PathBuf;

use mypaper_bars::icon_pack::{self, IconPack};
use mypaper_config::look::Style;
use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Rect, Rgba};
use serde_json::{Value, json};

use crate::actions::{Action, Choices, Edit, Page};
use crate::app::App;
use crate::docs::{DocId, Op, token};
use crate::settings::Place;
use crate::theme::glyph;
use crate::ui::{B, Col, inspector, label, section, setting, swatches};
use crate::view::split;

const STYLES: [(Style, &str); 5] = [
    (Style::Liquid, "Liquid"),
    (Style::Oneui, "One UI"),
    (Style::Solid, "Solid"),
    (Style::Fluent, "Fluent"),
    (Style::Mono, "Mono"),
];

const PRESETS: [[u8; 3]; 6] = [
    [0x8b, 0x9e, 0xb7],
    [0xc8, 0x8d, 0x3c],
    [0x7f, 0xae, 0x8e],
    [0xc9, 0x6b, 0x73],
    [0x93, 0x88, 0xc9],
    [0xe6, 0xe6, 0xe6],
];

/// The user's look style (or the default).
fn style(app: &App) -> Style {
    app.get(DocId::Config, "/look/style")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn style_name(s: Style) -> &'static str {
    STYLES.iter().find(|(x, _)| *x == s).map_or("Liquid", |(_, n)| n)
}

/// Home's one line for the look ("Liquid · Dark · accent from wallpaper").
pub fn summary(app: &App) -> String {
    let appearance = match app.get(DocId::Config, "/look/appearance").and_then(Value::as_str) {
        Some("light") => "Light",
        Some("auto") => "Match Windows",
        _ => "Dark",
    };
    let accent = match app.get(DocId::Config, "/look/accent").and_then(Value::as_str) {
        Some("auto") | None => "accent from wallpaper",
        Some(_) => "your accent",
    };
    format!("{} · {appearance} · {accent}", style_name(style(app)))
}

/// The active wallpaper, when it's one of yours (its look can be edited in place).
fn own_wallpaper(app: &App) -> Option<&crate::library::Wallpaper> {
    app.library.active().map(|i| &app.library.items[i]).filter(|w| w.editable())
}

/// Look's scope switch after the title.
pub fn scope(app: &App, b: &mut B, rect: Rect) -> f32 {
    let Some(i) = app.library.active() else { return rect.left };
    let w = &app.library.items[i];
    let value = if app.nav.look_theme { format!("The look {} brings", w.name) } else { "Your look".into() };
    let width = b.select_width(&value);
    let r = Rect::new(rect.left - b.px(4.0), rect.top + (rect.height() - b.px(28.0)) / 2.0, width, b.px(28.0));
    b.select(
        key("look-scope"),
        r,
        &value,
        "Editing",
        Action::Choose(Choices {
            edit: Edit::new(DocId::Studio, "/look_scope", "Scope"),
            options: vec![(json!(false), "Your look".into()), (json!(true), format!("The look {} brings", w.name))],
            current: Some(json!(app.nav.look_theme)),
        }),
    );
    r.right + b.px(10.0)
}

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let w = b.button_width("Reset", true, false);
    let right = right - w;
    b.button(key("look-reset"), Rect::new(right, y, w, b.px(32.0)), "Reset", Some(glyph::RESET), ButtonKind::Ghost, true, Action::LookReset);
    crate::view::edit_tools(app, b, bar, right - b.px(6.0))
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let (stage_side, side) = split(app, body);
    let frame = crate::pages::stage_in(app, body);
    let show = if app.nav.stage_real { crate::stage::Show::Desktop } else { crate::stage::Show::Specimen };
    crate::stage::draw(app, b, frame, show, |_| Action::None);
    // Specimen | Display, then the Peek hint.
    let y = frame.bottom + b.px(14.0);
    let display = format!("Display {}", app.nav.display + 1);
    let items = ["Specimen", display.as_str()];
    let seg_w = b.segmented_width(&items);
    let hint_w = b.px(200.0);
    let x = stage_side.left + (stage_side.width() - seg_w - hint_w) / 2.0;
    b.segmented(key("stage-mode"), Rect::new(x, y, seg_w, b.px(30.0)), &items, Some(usize::from(app.nav.stage_real)), "Stage", true, |i| Action::StageReal(i == 1));
    let kx = x + seg_w + b.px(12.0);
    let kw = b.keys(kx, y + b.px(5.0), &["Ctrl", "D"]);
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(kx + kw + b.px(8.0), y, b.px(160.0), b.px(30.0)), "Peek at your desktop", &caption, p.ink3, Align::Start);
    if app.glass.borrow().unavailable {
        b.text_in(Rect::new(stage_side.left, y + b.px(36.0), stage_side.width(), b.px(18.0)), "Glass shows when mypaper is running.", &caption, p.ink3, Align::Center);
    }
    inspector(app, b, side, key(("look-inspector", app.nav.look_windows)), |b, col| {
        // Desktop | Windows.
        let tabs = Rect::new(col.x + b.px(16.0), col.y + b.px(12.0), col.w - b.px(32.0), b.px(32.0));
        b.segmented(key("look-tab"), tabs, &["Desktop", "Windows"], Some(usize::from(app.nav.look_windows)), "Look", true, |i| Action::LookTab(i == 1));
        col.y = tabs.bottom + b.px(4.0);
        if app.nav.look_theme {
            theme_scope(app, b, col);
        } else if app.nav.look_windows {
            windows(app, b, col);
        } else {
            desktop(app, b, col);
        }
    });
}

fn desktop(app: &App, b: &mut B, col: &mut Col) {
    let current = style(app);
    section(b, col, "Style", |b, c| {
        let items: Vec<(&str, bool, Action)> = STYLES
            .iter()
            .map(|(s, name)| (*name, *s == current, Action::SetStyle(Some(*s))))
            .collect();
        crate::ui::tiles(b, c, key("style-tiles"), 3, false, &items, |b, i, pic| style_tile(app, b, STYLES[i].0, pic));
        crate::ui::anchor(app, "style", c.view, c.y);
    });
    let uses = crate::look::uses(current);
    section(b, col, "Material", |b, c| {
        let n = uses.iter().filter(|u| **u).count() + 1;
        let chip = if n == 5 { format!("{} uses all five", style_name(current)) } else { format!("{} uses {}", style_name(current), if n == 1 { "only tint".into() } else { format!("{n}") }) };
        let micro = b.font(Font::Micro);
        let (cw, _) = b.measure(&chip, &micro);
        let r = Rect::new(c.x + c.w - cw - b.px(12.0), c.y - b.px(29.0), cw + b.px(12.0), b.px(20.0));
        b.fill(r, b.px(5.0), b.p.fill);
        b.text_in(r, &chip, &micro, b.p.ink2, Align::Center);
        for (id, on) in ["clarity", "blur", "refraction", "highlight"].iter().zip(uses) {
            if on {
                setting(app, b, c, id);
            }
        }
        let caption = b.font(Font::Body);
        b.text_at(c.x, c.y + b.px(6.0), "Tint", &caption, b.p.ink1);
        c.y += b.px(30.0);
        crate::ui::anchor(app, "tint", c.view, c.y);
        let edit = Edit::new(DocId::Config, "/look/material/tint", "Tint");
        swatches(app, b, c, &edit, app.get(DocId::Config, "/look/material/tint"), Some(wallpaper_accent(app)), &PRESETS);
    });
    section(b, col, "Color", |b, c| {
        setting(app, b, c, "appearance");
        let accent = app.get(DocId::Config, "/look/accent");
        let row = Rect::new(c.x, c.y, c.w, b.px(28.0));
        let chip = if accent.and_then(Value::as_str).is_none_or(|a| a == "auto") { Some("From wallpaper") } else { None };
        label(b, row, "Accent", "", chip);
        c.y += b.px(32.0);
        crate::ui::anchor(app, "accent", c.view, c.y);
        let edit = Edit::new(DocId::Config, "/look/accent", "Accent");
        swatches(app, b, c, &edit, accent, Some(wallpaper_accent(app)), &PRESETS);
    });
    let place = Place::Look { windows: false };
    if crate::ui::more(app, b, col, place, "Text and icons", "") {
        section(b, col, "", |b, c| {
            for id in ["font", "text_size", "text_shadow", "text_tone", "icon_style"] {
                setting(app, b, c, id);
            }
            icon_pack(app, b, c);
        });
    }
    if crate::ui::more(app, b, col, place, "Shape", "") {
        section(b, col, "", |b, c| {
            setting(app, b, c, "corners");
            setting(app, b, c, "edge");
        });
    }
    if crate::ui::more(app, b, col, place, "Different for", "Bars, widgets, menus") {
        section(b, col, "", |b, c| {
            crate::ui::anchor(app, "different", c.view, c.y);
            for (family, name) in [("bars", "Bars"), ("widgets", "Widgets"), ("menus", "Menus and panels")] {
                let path = format!("/look/{family}/style");
                let own: Option<Style> = app.get(DocId::Config, &path).and_then(|v| serde_json::from_value(v.clone()).ok());
                let mut options = vec![(Value::Null, "Follows Look".to_owned())];
                options.extend(STYLES.iter().map(|(s, n)| (json!(s), (*n).to_owned())));
                let shown = own.map_or("Follows Look", style_name);
                let h = b.px(40.0);
                let w = b.select_width(shown).max(b.px(120.0));
                label(b, Rect::new(c.x, c.y, c.w - w, h), name, "", None);
                b.select(key(("different", family)), Rect::new(c.x + c.w - w, c.y + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), shown, name, Action::Choose(Choices {
                    edit: Edit::new(DocId::Config, path, format!("{name} style")),
                    options,
                    current: Some(own.map_or(Value::Null, |s| json!(s))),
                }));
                c.y += h + b.px(4.0);
            }
            let caption = b.font(Font::Caption);
            c.y += b.paragraph(c.x, c.y + b.px(4.0), c.w, "A bar's own look is under Bars › a bar › More › Look; a widget's under Widgets.", &caption, b.px(16.0), b.p.ink3) + b.px(4.0);
        });
    }
}

/// The icon packs: none (the style's), the bundled ones and your own folders.
pub(crate) fn icon_pack(app: &App, b: &mut B, c: &mut Col) {
    let current = app.get(DocId::Config, "/look/text/icon_pack").and_then(Value::as_str).map(str::to_owned);
    let mut options = vec![(Value::Null, "The style's icons".to_owned())];
    if let Ok(dir) = mypaper_config::dir() {
        for entry in std::fs::read_dir(dir.join("icons")).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                options.push((json!(name), name.clone()));
            }
        }
    }
    let shown = current.clone().unwrap_or_else(|| "The style's icons".into());
    let h = b.px(40.0);
    let w = b.select_width(&shown).max(b.px(120.0));
    label(b, Rect::new(c.x, c.y, c.w - w, h), "Icon pack", "", None);
    crate::ui::anchor(app, "icon_pack", c.view, c.y);
    b.select(key("icon-pack"), Rect::new(c.x + c.w - w, c.y + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), &shown, "Icon pack", Action::Choose(Choices {
        edit: Edit::new(DocId::Config, "/look/text/icon_pack", "Icon pack"),
        options,
        current: Some(current.map_or(Value::Null, Value::String)),
    }));
    c.y += h + b.px(4.0);
    // Your own pack: the apps it gives an icon, and adding one.
    if let Some((dir, pack)) = own_pack(app) {
        let body = b.font(Font::Body);
        for (i, entry) in pack.entries.iter().enumerate() {
            let r = Rect::new(c.x, c.y, c.w, b.px(36.0));
            let icon = Rect::new(r.left + b.px(2.0), r.top + b.px(8.0), b.px(20.0), b.px(20.0));
            match app.images.borrow_mut().get(&dir.join(&entry.icon), (64, 64)) {
                Some(image) => crate::stage::image_cover(b, icon, &image, 1.0),
                None => b.icon(icon, glyph::APP, 14.0, b.p.ink3),
            }
            let what = [&entry.path, &entry.umid, &entry.extension, &entry.key].into_iter().flatten().next().map_or("", String::as_str);
            b.text_in(Rect::new(r.left + b.px(32.0), r.top, r.width() - b.px(68.0), r.height()), what, &body, b.p.ink1, Align::Start);
            let remove = Rect::new(r.right - b.px(30.0), r.top + b.px(4.0), b.px(28.0), b.px(28.0));
            b.icon_button(key(("pack-remove", i)), remove, glyph::DELETE, &format!("Remove {what}"), false, true, Action::IconPackRemove(i));
            c.y += b.px(36.0);
        }
        let w = b.button_width("An app's icon", true, true);
        b.button(key("pack-add"), Rect::new(c.x, c.y + b.px(4.0), w, b.px(28.0)), "An app's icon", Some(glyph::ADD), ButtonKind::Secondary, true, Action::IconPackAdd);
        c.y += b.px(40.0);
    } else {
        let w = b.button_width("Start your own", true, true);
        b.button(key("pack-new"), Rect::new(c.x, c.y + b.px(4.0), w, b.px(28.0)), "Start your own", Some(glyph::ADD), ButtonKind::Secondary, true, Action::IconPackNew);
        c.y += b.px(40.0);
    }
}

/// Where your icon packs are (`%APPDATA%\mypaper\icons`).
fn packs_dir() -> Option<PathBuf> {
    mypaper_config::dir().ok().map(|d| d.join("icons"))
}

/// The pack in use, when it's one of your own folders.
fn own_pack(app: &App) -> Option<(PathBuf, IconPack)> {
    let name = app.get(DocId::Config, "/look/text/icon_pack")?.as_str()?;
    let dir = packs_dir()?.join(name);
    IconPack::load(&dir).ok().map(|pack| (dir, pack))
}

/// Writes a pack's manifest (checked first), then has the bars reload it.
fn save_pack(app: &mut App, dir: &std::path::Path, pack: &IconPack) {
    let written = serde_json::to_string_pretty(pack)
        .map_err(anyhow::Error::from)
        .and_then(|text| IconPack::parse(&text).map(|_| text))
        .and_then(|text| {
            std::fs::create_dir_all(dir)?;
            crate::docs::write_atomic(&dir.join(icon_pack::MANIFEST), text.as_bytes())
        })
        .and_then(|()| app.docs.touch(DocId::Bars));
    if let Err(e) = written {
        app.alert(format!("{e:#}"));
    }
    app.dirty = true;
}

/// The wallpaper's accent (its palette), for "From wallpaper" swatches.
fn wallpaper_accent(_app: &App) -> Rgba {
    mypaper_tweaks_palette().unwrap_or(Rgba::rgb8([0x5b, 0x8d, 0xef], 1.0))
}

fn mypaper_tweaks_palette() -> Option<Rgba> {
    let dir = std::env::var_os("LOCALAPPDATA")?;
    let text = std::fs::read_to_string(std::path::PathBuf::from(dir).join("mypaper").join("palette.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let hex = v.pointer("/palette/accent").and_then(Value::as_str)?;
    mypaper_config::Color::parse(hex).map(|c| Rgba::rgb8(c.0, 1.0))
}

/// A style tile: the wallpaper, a tiny bar and a card drawn in that style.
fn style_tile(app: &App, b: &mut B, s: Style, pic: Rect) {
    if let Some(still) = app.library.active().and_then(|i| app.library.items[i].preview.clone())
        && let Some(image) = app.images.borrow_mut().get(&still, (pic.width() as u32 * 2, pic.height() as u32 * 2))
    {
        crate::stage::image_round(b, pic, &image, 1.0, b.px(8.0));
    } else {
        b.fill(pic, b.px(8.0), Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0));
    }
    let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
    let (fill, rim, radius, weight) = match s {
        Style::Liquid => (Rgba::new(1.0, 1.0, 1.0, 0.08), Rgba::new(1.0, 1.0, 1.0, 0.45), 9.0, 300),
        Style::Oneui => (Rgba::rgb8([40, 44, 56], 0.62), Rgba::TRANSPARENT, 12.0, 700),
        Style::Solid => (Rgba::rgb8([32, 32, 34], 1.0), Rgba::rgb8([58, 58, 60], 1.0), 5.0, 400),
        Style::Fluent => (Rgba::rgb8([36, 36, 40], 0.78), Rgba::new(1.0, 1.0, 1.0, 0.1), 5.0, 400),
        Style::Mono => (Rgba::new(0.0, 0.0, 0.0, 0.62), Rgba::new(1.0, 1.0, 1.0, 0.16), 6.0, 400),
    };
    let s_ = b.s;
    let bar = Rect::new(pic.left + pic.width() * 0.06, pic.top + pic.height() * 0.07, pic.width() * 0.88, pic.height() * 0.11);
    b.fill(bar, 4.0 * s_, fill);
    if rim.a > 0.0 {
        b.stroke(bar, 4.0 * s_, 1.0, rim);
    }
    let card = Rect::new(pic.left + pic.width() * 0.16, pic.top + pic.height() * 0.32, pic.width() * 0.68, pic.height() * 0.54);
    b.fill(card, radius * s_, fill);
    if rim.a > 0.0 {
        b.stroke(card, radius * s_, 1.0, rim);
    }
    let mut font = b.t.style(Font::Body, s_).scaled(15.0 / 14.0);
    font.weight = weight;
    b.text_in(Rect::new(card.left + card.width() * 0.1, card.top + b.px(4.0), card.width(), card.height() * 0.55), "10:42", &font, white, Align::Start);
    b.fill(Rect::new(card.left + card.width() * 0.1, card.bottom - card.height() * 0.3, card.width() * 0.6 * 0.8, b.px(2.0)), 1.0, Rgba::new(1.0, 1.0, 1.0, 0.7));
}

/// The four Windows looks, as presets of the Windows layer.
const WINDOWS_LOOKS: [(&str, &str); 4] = [
    ("As is", r#"{ "accent": null, "border": null, "caption": null, "caption_text": null, "corners": null, "backdrop": null, "apps": "as-is", "cursors": null }"#),
    ("Match mypaper", r#"{ "accent": "auto", "border": "auto", "corners": "round", "backdrop": "mica", "apps": "match" }"#),
    ("Glass", r#"{ "accent": "auto", "border": null, "corners": "round", "backdrop": "acrylic", "apps": "match" }"#),
    ("Sharp", r#"{ "accent": "auto", "border": "auto", "corners": "square", "backdrop": "none", "apps": "match" }"#),
];

fn windows(app: &App, b: &mut B, col: &mut Col) {
    let layer = app.get(DocId::Config, "/look/windows").cloned().unwrap_or(json!({}));
    let current = WINDOWS_LOOKS.iter().position(|(_, json)| {
        let preset: Value = serde_json::from_str(json).unwrap_or_default();
        preset.as_object().is_some_and(|m| m.iter().all(|(k, v)| layer.get(k) == Some(v) || (v.is_null() && layer.get(k).is_some_and(Value::is_null))))
    });
    section(b, col, "Windows look", |b, c| {
        let items: Vec<(&str, bool, Action)> = WINDOWS_LOOKS
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (*name, current == Some(i), Action::WindowsLook(i)))
            .collect();
        crate::ui::anchor(app, "windows_look", c.view, c.y);
        crate::ui::tiles(b, c, key("windows-looks"), 4, false, &items, |b, i, pic| window_tile(app, b, i, pic));
    });
    section(b, col, "", |b, c| {
        // Accent: same as mypaper's, a colour of your own, or as Windows has it.
        let accent = layer.get("accent");
        let at = match accent {
            None => None,
            Some(Value::Null) => Some(2),
            Some(Value::String(s)) if s == "auto" => Some(0),
            Some(_) => Some(1),
        };
        let edit = Edit::new(DocId::Config, "/look/windows/accent", "Windows accent");
        let label_style = b.font(Font::Body);
        b.text_at(c.x, c.y, "Accent", &label_style, b.p.ink1);
        crate::ui::anchor(app, "windows_accent", c.view, c.y);
        c.y += b.px(24.0);
        let e2 = edit.clone();
        b.segmented(key("windows-accent"), Rect::new(c.x, c.y, c.w, b.px(32.0)), &["Same as mypaper", "Pick", "As is"], at, "Windows accent", true, |i| match i {
            0 => Action::Set(e2.clone(), json!("auto")),
            1 => Action::Set(e2.clone(), json!("#5b8def")),
            _ => Action::Set(Edit::new(DocId::Studio, "/windows_null/accent", "Windows accent"), Value::Null),
        });
        c.y += b.px(40.0);
        if at == Some(1) {
            swatches(app, b, c, &edit, accent, None, &PRESETS);
        }
        for id in ["windows_apps", "windows_corners", "windows_material"] {
            setting(app, b, c, id);
        }
    });
    section(b, col, "Pointer", |b, c| {
        let cursors = layer.get("cursors");
        let at = match cursors {
            Some(Value::Null) | None => 0,
            Some(Value::String(s)) if s == "mypaper-dark" => 1,
            Some(Value::String(s)) if s == "mypaper-light" => 2,
            Some(_) => 3,
        };
        let edit = Edit::new(DocId::Config, "/look/windows/cursors", "Pointer");
        let items = [
            ("As is", at == 0, Action::Set(Edit::new(DocId::Studio, "/windows_null/cursors", "Pointer"), Value::Null)),
            ("Dark", at == 1, Action::Set(edit.clone(), json!("mypaper-dark"))),
            ("Light", at == 2, Action::Set(edit.clone(), json!("mypaper-light"))),
            ("Other", at == 3, Action::PickFile("cursors".into(), "/look/windows/cursors".into())),
        ];
        crate::ui::anchor(app, "pointer", c.view, c.y);
        crate::ui::tiles(b, c, key("pointer-tiles"), 4, true, &items, pointer_tile);
    });
    let place = Place::Look { windows: true };
    let edge_detail = match layer.get("border").and_then(Value::as_str) {
        Some("auto") => "Edge from wallpaper",
        Some(_) => "Your edge",
        None => "",
    };
    if crate::ui::more(app, b, col, place, "Edges and title bars", edge_detail) {
        section(b, col, "", |b, c| {
            for (id, name, path, auto) in [
                ("window_edge", "Window edge", "/look/windows/border", true),
                ("title_bar", "Title bar", "/look/windows/caption", false),
                ("title_text", "Title text", "/look/windows/caption_text", false),
            ] {
                let v = app.get(DocId::Config, path);
                let at = match v {
                    None => None,
                    Some(Value::Null) => Some(if auto { 2 } else { 1 }),
                    Some(Value::String(s)) if s == "auto" => Some(0),
                    Some(_) => Some(if auto { 1 } else { 0 }),
                };
                let body = b.font(Font::Body);
                b.text_at(c.x, c.y, name, &body, b.p.ink1);
                crate::ui::anchor(app, id, c.view, c.y);
                c.y += b.px(24.0);
                let edit = Edit::new(DocId::Config, path, name);
                let null = Edit::new(DocId::Studio, format!("/windows_null/{}", path.rsplit('/').next().unwrap_or("")), name);
                let items: &[&str] = if auto { &["From wallpaper", "Pick", "As is"] } else { &["Pick", "As is"] };
                let e2 = edit.clone();
                b.segmented(key(("edge", id)), Rect::new(c.x, c.y, c.w, b.px(32.0)), items, at, name, true, |i| match (auto, i) {
                    (true, 0) => Action::Set(e2.clone(), json!("auto")),
                    (true, 1) | (false, 0) => Action::Set(e2.clone(), json!("#3a3a3c")),
                    _ => Action::Set(null.clone(), Value::Null),
                });
                c.y += b.px(40.0);
                if v.and_then(Value::as_str).is_some_and(|s| s.starts_with('#')) {
                    swatches(app, b, c, &edit, v, None, &PRESETS);
                }
            }
        });
    }
    let caption = b.font(Font::Caption);
    col.y += b.px(14.0);
    col.y += b.paragraph(col.x + b.px(16.0), col.y, col.w - b.px(32.0), "Everything here is undone when mypaper stops or you press Ctrl Alt Shift Backspace.", &caption, b.px(16.0), b.p.ink3) + b.px(16.0);
}

fn window_tile(app: &App, b: &mut B, i: usize, pic: Rect) {
    if let Some(still) = app.library.active().and_then(|i| app.library.items[i].preview.clone())
        && let Some(image) = app.images.borrow_mut().get(&still, (pic.width() as u32 * 2, pic.height() as u32 * 2))
    {
        crate::stage::image_round(b, pic, &image, 1.0, b.px(8.0));
    } else {
        b.fill(pic, b.px(8.0), Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0));
    }
    let win = Rect::new(pic.left + pic.width() * 0.18, pic.top + pic.height() * 0.22, pic.width() * 0.64, pic.height() * 0.6);
    let s = b.s;
    let (fill, edge, radius) = match i {
        0 => (Rgba::rgb8([0xf3, 0xf3, 0xf3], 1.0), Rgba::rgb8([0xcc, 0xcc, 0xcc], 1.0), 3.0),
        1 => (Rgba::rgb8([0x20, 0x22, 0x2a], 0.92), Rgba::rgb8([0x7d, 0x9b, 0xc8], 1.0), 3.0),
        2 => (Rgba::rgb8([0x30, 0x34, 0x40], 0.6), Rgba::new(1.0, 1.0, 1.0, 0.3), 3.0),
        _ => (Rgba::rgb8([0x1a, 0x1a, 0x1c], 1.0), Rgba::rgb8([0x7d, 0x9b, 0xc8], 1.0), 0.0),
    };
    b.fill(win, radius * s, fill);
    b.stroke(win, radius * s, 1.0, edge);
    let ink = if i == 0 { Rgba::rgb8([0x66, 0x66, 0x66], 1.0) } else { Rgba::new(1.0, 1.0, 1.0, 0.7) };
    b.fill(Rect::new(win.left + win.width() * 0.12, win.top + win.height() * 0.3, win.width() * 0.4, win.height() * 0.16), 1.0, ink);
}

fn pointer_tile(b: &mut B, i: usize, pic: Rect) {
    let p = *b.p;
    let (bg, fg) = match i {
        1 => (Rgba::rgb8([0xcf, 0xcf, 0xcf], 1.0), Rgba::rgb8([0x11, 0x11, 0x11], 1.0)),
        2 => (Rgba::rgb8([0x2a, 0x2a, 0x2a], 1.0), Rgba::rgb8([0xf5, 0xf5, 0xf5], 1.0)),
        3 => (p.panel, p.ink4),
        _ => (p.hover, p.ink1),
    };
    b.fill(pic, b.px(8.0), bg);
    b.icon(pic, glyph::POINTER, 22.0, fg);
}

/// Look scoped to the wallpaper: the look the active wallpaper brings, edited in its own file.
pub(crate) fn theme_scope(app: &App, b: &mut B, col: &mut Col) {
    let Some(w) = app.library.active().map(|i| app.library.items[i].clone()) else { return };
    if !w.editable() {
        section(b, col, "", |b, c| {
            let caption = b.font(Font::Body);
            c.y += b.paragraph(c.x, c.y, c.w, &format!("{} is built in, so its look can't change. A copy is yours to change, and it brings this look with it.", w.name), &caption, b.px(20.0), b.p.ink2);
            c.y += b.px(10.0);
            let bw = b.button_width("Make a copy", true, false);
            if let Some(i) = app.library.active() {
                b.button(key("scope-copy"), Rect::new(c.x, c.y, bw, b.px(32.0)), "Make a copy", Some(glyph::COPY), ButtonKind::Primary, true, Action::CopyWallpaper(i));
            }
            c.y += b.px(36.0);
        });
        return;
    }
    let dir = token(&w.dir.display().to_string());
    let theme: Value = std::fs::read_to_string(w.dir.join(mypaper_theme::MANIFEST))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}));
    let edit = |path: &str, label: &str| Edit::new(DocId::Studio, format!("/theme:{dir}{path}"), label);
    let caption = b.font(Font::Caption);
    col.y += b.px(12.0);
    col.y += b.paragraph(col.x + b.px(16.0), col.y, col.w - b.px(32.0), &format!("Editing the look {} brings. Your own look still wins where you set it.", w.name), &caption, b.px(16.0), b.p.ink3);
    if app.nav.look_windows {
        section(b, col, "Windows", |b, c| {
            for (name, path, options) in [
                ("Corners", "/windows/corners", &[("null", "As is"), (r#""round""#, "Round"), (r#""small""#, "Small"), (r#""square""#, "Square")][..]),
                ("Window material", "/windows/backdrop", &[("null", "As is"), (r#""mica""#, "Mica"), (r#""acrylic""#, "Acrylic"), (r#""none""#, "None")][..]),
                ("Apps", "/windows/dark_mode", &[("null", "As is"), ("false", "Light"), ("true", "Dark")][..]),
                ("Accent", "/windows/accent", &[("null", "As is"), (r#""auto""#, "From wallpaper")][..]),
            ] {
                theme_choice(b, c, &theme, path, name, options, edit(path, name));
            }
        });
        return;
    }
    section(b, col, "Material", |b, c| {
        let clarity = theme.pointer("/widgets/glass/tint").and_then(Value::as_f64).map_or(0.94, |t| 1.0 - t);
        theme_slider(b, c, "Clarity", clarity, (0.0, 1.0, 0.01), 100.0, "%", edit("/@both/glass/clarity", "Clarity"));
        let blur = theme.pointer("/widgets/glass/blur").and_then(Value::as_f64).unwrap_or(0.0);
        theme_slider(b, c, "Blur", blur, (0.0, 32.0, 1.0), 1.0, "px", edit("/@both/glass/blur", "Blur"));
        let refraction = theme.pointer("/widgets/glass/refraction").and_then(Value::as_f64).unwrap_or(0.6);
        theme_slider(b, c, "Refraction", refraction, (0.0, 1.0, 0.01), 100.0, "%", edit("/@both/glass/refraction", "Refraction"));
        let highlight = theme.pointer("/widgets/glass/highlight").and_then(Value::as_f64).unwrap_or(0.55);
        theme_slider(b, c, "Highlight", highlight, (0.0, 1.0, 0.01), 100.0, "%", edit("/@both/glass/highlight", "Highlight"));
    });
    section(b, col, "Color", |b, c| {
        theme_choice(b, c, &theme, "/widgets/appearance/text", "Text", &[("null", "The style's"), (r#""light""#, "Light"), (r#""dark""#, "Dark")], edit("/@both/appearance/text", "Text"));
        let body = b.font(Font::Body);
        b.text_at(c.x, c.y, "Accent", &body, b.p.ink1);
        c.y += b.px(28.0);
        swatches(app, b, c, &edit("/@both/appearance/accent", "Accent"), theme.pointer("/widgets/appearance/accent"), Some(wallpaper_accent(app)), &PRESETS);
        b.text_at(c.x, c.y, "Tint", &body, b.p.ink1);
        c.y += b.px(28.0);
        swatches(app, b, c, &edit("/@both/appearance/tint", "Tint"), theme.pointer("/widgets/appearance/tint"), None, &PRESETS);
    });
}

fn theme_choice(b: &mut B, c: &mut Col, theme: &Value, path: &str, name: &str, options: &[(&str, &str)], edit: Edit) {
    let value = theme.pointer(path).cloned().unwrap_or(Value::Null);
    let values: Vec<Value> = options.iter().map(|(v, _)| serde_json::from_str(v).unwrap_or(Value::Null)).collect();
    let labels: Vec<&str> = options.iter().map(|(_, l)| *l).collect();
    let at = values.iter().position(|v| *v == value);
    let body = b.font(Font::Body);
    b.text_at(c.x, c.y, name, &body, b.p.ink1);
    c.y += b.px(24.0);
    b.segmented(key(("theme-choice", path)), Rect::new(c.x, c.y, c.w, b.px(32.0)), &labels, at, name, true, |i| Action::Set(edit.clone(), values[i].clone()));
    c.y += b.px(40.0);
}

#[allow(clippy::too_many_arguments)]
fn theme_slider(b: &mut B, c: &mut Col, name: &str, value: f64, range: (f64, f64, f64), factor: f64, unit: &str, edit: Edit) {
    let lw = b.px(100.0);
    label(b, Rect::new(c.x, c.y, lw, b.px(40.0)), name, "", None);
    let text = if unit == "%" { format!("{:.0}%", value * factor) } else { format!("{:.0} {unit}", value * factor) };
    b.slider(key(("theme-slider", name)), Rect::new(c.x + lw, c.y, c.w - lw, b.px(40.0)), value, range, &text, b.px(44.0), name, Action::Slider(edit));
    c.y += b.px(40.0);
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::LookTab(windows) => {
            app.go(Page::Look);
            app.nav.look_windows = windows;
        }
        Action::IconPackNew => {
            let Some(root) = packs_dir() else { return };
            let Some(name) = (1..100).map(|n| if n == 1 { "My icons".to_owned() } else { format!("My icons {n}") }).find(|n| !root.join(n).exists()) else { return };
            save_pack(app, &root.join(&name), &IconPack { name: name.clone(), mono: false, missing: None, entries: Vec::new() });
            app.set(&Edit::new(DocId::Config, "/look/text/icon_pack", "Icon pack"), json!(name));
        }
        Action::IconPackAdd => {
            let Some((dir, mut pack)) = own_pack(app) else { return };
            let Some(exe) = crate::dialogs::pick(app.window, "Choose the app", false, crate::dialogs::PROGRAMS) else { return };
            let Some(picture) = crate::dialogs::pick(app.window, "Choose its icon", false, crate::dialogs::ICONS) else { return };
            let (Some(stem), Some(ext), Some(exe_name)) = (
                exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()),
                picture.extension().map(|s| s.to_string_lossy().to_lowercase()),
                exe.file_name().map(|s| s.to_string_lossy().to_lowercase()),
            ) else {
                return;
            };
            let icon = format!("{stem}.{ext}");
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(&picture, dir.join(&icon))) {
                app.alert(format!("Couldn't copy the picture: {e}"));
                return;
            }
            // By file name, so it matches the app wherever it's installed.
            pack.entries.retain(|e| e.path.as_deref() != Some(exe_name.as_str()));
            pack.entries.push(icon_pack::Entry { path: Some(exe_name), icon, ..Default::default() });
            save_pack(app, &dir, &pack);
        }
        Action::IconPackRemove(i) => {
            let Some((dir, mut pack)) = own_pack(app) else { return };
            if i < pack.entries.len() {
                pack.entries.remove(i);
                save_pack(app, &dir, &pack);
            }
        }
        Action::LookScope(theme) => {
            app.go(Page::Look);
            app.nav.look_theme = theme && own_wallpaper(app).is_some();
            if theme && own_wallpaper(app).is_none() {
                app.nav.look_theme = true;
            }
        }
        Action::StageReal(real) => app.nav.stage_real = real,
        Action::SetStyle(style) => {
            // A new style starts from its own material: the sliders' values reset.
            let mut ops = vec![Op { doc: DocId::Config, path: "/look/style".into(), value: style.map(|s| json!(s)) }];
            for field in ["clarity", "blur", "refraction", "highlight"] {
                ops.push(Op { doc: DocId::Config, path: format!("/look/material/{field}"), value: None });
            }
            app.apply(&format!("{} style", style.map_or("Default", style_name)), ops);
        }
        Action::WindowsLook(i) => {
            let preset: Value = serde_json::from_str(WINDOWS_LOOKS[i].1).unwrap_or_default();
            app.apply(&format!("Windows look: {}", WINDOWS_LOOKS[i].0), vec![Op { doc: DocId::Config, path: "/look/windows".into(), value: Some(preset) }]);
        }
        Action::LookReset => {
            let path = if app.nav.look_windows { "/look/windows" } else { "/look" };
            let mut ops = vec![Op { doc: DocId::Config, path: path.into(), value: None }];
            if !app.nav.look_windows {
                // Keep the Windows layer: Reset on the Desktop tab is the desktop's.
                if let Some(windows) = app.get(DocId::Config, "/look/windows").cloned() {
                    ops.push(Op { doc: DocId::Config, path: "/look/windows".into(), value: Some(windows) });
                }
            }
            app.apply(if app.nav.look_windows { "Reset Windows styling" } else { "Reset the look" }, ops);
            app.toast_undo("Back to the wallpaper's look");
        }
        _ => {}
    }
}

/// `null` for a Windows field means "As is" (keep Windows' own), which a plain Set can't say
/// (it removes the key): these pseudo-paths write an explicit null.
pub fn intercept_null(app: &mut App, path: &str) -> bool {
    let Some(field) = path.strip_prefix("/windows_null/") else { return false };
    app.apply(&format!("Windows {field}: as is"), vec![Op { doc: DocId::Config, path: format!("/look/windows/{field}"), value: Some(Value::Null) }]);
    true
}

