use mypaper_ui::controls::{Align, Behavior, key};
use mypaper_ui::{Rect, Role};
use serde_json::{Value, json};

use super::*;
use crate::actions::Source;
use crate::app::Overlay;
use crate::docs::token;
use crate::library::Kind;

fn thumbnail(app: &App, b: &mut B, r: Rect, i: usize) {
    let wallpaper = &app.library.items[i];
    b.fill(r, b.px(8.0), b.p.fill);
    if wallpaper.error.is_some() { b.icon(r, glyph::ALERT, 22.0, b.p.ink3); return }
    let image = wallpaper.preview.as_ref().or(wallpaper.video.as_ref()).and_then(|path| app.images.borrow_mut().get(path, (r.width() as u32, r.height() as u32)));
    match image {
        Some(image) => crate::stage::image_round(b, r, &image, 1.0, b.px(8.0)),
        None => b.icon(r, glyph::WALLPAPERS, 22.0, b.p.ink3),
    }
}

pub fn list(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Wallpapers", &format!("{} wallpapers · click to apply", app.library.items.len()));
    action_button(b, c, "Add a wallpaper", Action::AddWallpaper);
    let r = Rect::new(c.x, c.y, c.w, b.px(30.0));
    b.segmented(key("panel-wallpaper-filter"), r, &["All", "Yours", "Built in"], Some(usize::from(app.nav.filter.min(2))), "Wallpapers", true, |i| Action::WallpaperFilter(i as u8));
    b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
    c.y = r.bottom + b.px(8.0);
    field(app, b, c, FieldId::WallpaperSearch, "Search wallpapers", "Name or category", "");
    let category = &app.panel.as_ref().unwrap().category;
    row(b, c, None, "Categories", category.as_deref().unwrap_or("All"), "", None,
        Action::More(key("panel-wallpaper-categories")));
    let mut categories = Vec::new();
    for wallpaper in &app.library.items { if !categories.contains(&wallpaper.category()) { categories.push(wallpaper.category()); } }
    if app.is_open(key("panel-wallpaper-categories")) {
        group(b, c, "Category", |b, c| {
            row(b, c, None, "All categories", "", "", Some(category.is_none()), Action::Panel(Command::Category(None)));
            for name in &categories { row(b, c, None, name, "", "", Some(category.as_deref() == Some(name)), Action::Panel(Command::Category(Some((*name).into())))); }
        });
    }
    let query = app.text(&FieldId::WallpaperSearch).to_lowercase();
    let mut count = 0;
    for category_name in categories {
        if category.as_deref().is_some_and(|name| name != category_name) { continue }
        let shown: Vec<_> = app.library.items.iter().enumerate().filter(|(_, w)| {
            !w.hidden && w.category() == category_name
                && (app.nav.filter != 1 || w.editable()) && (app.nav.filter != 2 || !w.editable())
                && format!("{} {} {}", w.name, w.category(), w.detail()).to_lowercase().contains(&query)
        }).collect();
        if shown.is_empty() { continue }
        c.y += b.px(18.0);
        text(b, Rect::new(c.x, c.y, c.w, height(b, 24.0)), category_name, 13.0, 600, b.p.ink1, Align::Start);
        c.y += height(b, 28.0);
        let gap = b.px(12.0);
        let width = (c.w - gap) / 2.0;
        for chunk in shown.chunks(2) {
            let row_height = width * 0.56 + height(b, 64.0);
            for (column, (i, wallpaper)) in chunk.iter().enumerate() {
                let x = c.x + column as f32 * (width + gap);
                let r = Rect::new(x, c.y, width, width * 0.56);
                thumbnail(app, b, r, *i);
                let k = key(("panel-wallpaper", i));
                let apply = if wallpaper.error.is_some() { Action::Panel(Command::Wallpaper(*i)) } else { Action::UseWallpaper(*i) };
                b.hit(k, r, Role::Button, &format!("Apply {}", wallpaper.name), apply, Behavior::Click);
                b.focus_ring(k, r, b.px(8.0));
                if wallpaper.active {
                    let check = Rect::new(r.right - b.px(22.0), r.top + b.px(6.0), b.px(16.0), b.px(16.0));
                    b.fill(check, b.px(8.0), b.p.solid); b.icon(check, glyph::CHECK, 11.0, b.p.on_solid);
                }
                text(b, Rect::new(x, r.bottom + b.px(4.0), width - b.px(24.0), height(b, 22.0)), &wallpaper.name, 13.0, 500, b.p.ink1, Align::Start);
                let detail = Rect::new(r.right - b.px(24.0), r.bottom, b.px(24.0), height(b, 28.0));
                b.icon(detail, glyph::CHEVRON_RIGHT, 15.0, b.p.ink2);
                b.hit(key(("panel-wallpaper-details", i)), Rect::new(x, r.bottom, width, height(b, 28.0)), Role::Button, &format!("{} details", wallpaper.name), Action::Panel(Command::Wallpaper(*i)), Behavior::Click);
                let caption = if wallpaper.error.is_some() { "Can't load · open for details" } else if wallpaper.active { "On your desktop" } else { wallpaper.kind.map_or("Wallpaper", Kind::label) };
                text(b, Rect::new(x, r.bottom + height(b, 26.0), width, height(b, 18.0)), caption, 12.0, 400, b.p.ink3, Align::Start);
                count += 1;
            }
            c.y += row_height;
        }
    }
    if count == 0 { notice(b, c, "No wallpapers match. Try All or another search."); }
}

fn theme_value(wallpaper: &crate::library::Wallpaper) -> Value {
    std::fs::read_to_string(wallpaper.dir.join(mypaper_theme::MANIFEST)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
}

fn theme_edit(w: &crate::library::Wallpaper, path: &str, label: &str) -> Edit {
    Edit::new(DocId::Studio, format!("/theme:{}{path}", token(&w.dir.display().to_string())), label)
}

pub fn details(app: &App, b: &mut B, c: &mut Col) {
    let Some(i) = app.nav.wallpaper.or_else(|| app.library.active()).filter(|&i| i < app.library.items.len()) else { title(b, c, "Wallpaper", "No wallpaper selected"); return };
    let w = &app.library.items[i];
    title(b, c, &w.name, &format!("{} · {}", w.detail(), w.category()));
    let pic = Rect::new(c.x, c.y + b.px(8.0), c.w, c.w * 0.56);
    thumbnail(app, b, pic, i); c.y = pic.bottom + b.px(8.0);
    if let Some(error) = &w.error {
        group(b, c, "Can't load", |b, c| { notice(b, c, error.lines().next().unwrap_or(error)); });
    } else {
        if !w.active { action_button(b, c, "Use this wallpaper", Action::UseWallpaper(i)); }
        let theme = theme_value(w);
        if w.kind.is_some_and(Kind::uses_fit) {
            group(b, c, "Fit", |b, c| {
                let current = theme.pointer("/wallpaper/fit").and_then(Value::as_str).unwrap_or("cover");
                for (value, label) in [("cover", "Fill"), ("contain", "Fit"), ("stretch", "Stretch"), ("center", "Center")] {
                    let action = if w.editable() { Action::Set(theme_edit(w, "/wallpaper/fit", "Fit"), json!(value)) } else { Action::CopyWallpaper(i) };
                    row(b, c, None, label, "", "", Some(current == value), action);
                }
                if !w.editable() { notice(b, c, "A built-in wallpaper: make a copy to change its fit."); }
            });
        }
        if let Some(on) = w.interactive {
            group(b, c, "Mouse", |b, c| toggle(b, c, "React to the mouse", "Clicks and moves reach the wallpaper", on,
                if w.editable() { Action::Toggle(theme_edit(w, "/wallpaper/interactive", "React to the mouse"), on) } else { Action::CopyWallpaper(i) }));
        }
        group(b, c, "Its look", |b, c| {
            notice(b, c, if w.brings_look { "Brings a look. Your own choices still win." } else { "No look of its own. Your look applies." });
            if w.active { action_button(b, c, "Edit the look it brings", Action::Panel(Command::Scope(true))); }
            else { notice(b, c, "Apply this wallpaper to edit the look it brings."); }
        });
        if w.kind == Some(Kind::App) && w.editable() {
            group(b, c, "App", |b, c| {
                let args = theme.pointer("/wallpaper/args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                field(app, b, c, FieldId::Text(DocId::Studio, format!("/app_args/{}", token(&w.id)).into()), "Arguments", "{hwnd} is the wallpaper window", &args);
            });
        }
        group(b, c, "Performance", |b, c| {
            let base = format!("/engine/wallpapers/{}", token(&w.id));
            select(b, c, "Graphics card", app.get(DocId::Config, &format!("{base}/gpu")).cloned().unwrap_or(Value::Null),
                Edit::new(DocId::Config, format!("{base}/gpu"), "Wallpaper graphics card"),
                vec![(Value::Null, "Same as Settings".into()), (json!("integrated"), "Integrated".into()), (json!("high-performance"), "High performance".into()), (json!("auto"), "Windows' choice".into())]);
            select(b, c, "Frame rate", app.get(DocId::Config, &format!("{base}/fps")).cloned().unwrap_or(Value::Null),
                Edit::new(DocId::Config, format!("{base}/fps"), "Wallpaper frame rate"),
                vec![(Value::Null, "Same as Settings".into()), (json!(24), "24 fps".into()), (json!(30), "30 fps".into()), (json!(60), "60 fps".into()), (json!(120), "120 fps".into())]);
        });
    }
    if let Some(credit) = &w.credit {
        group(b, c, "Credit", |b, c| {
            for line in [credit.author.as_deref(), credit.license.as_deref()].into_iter().flatten() { notice(b, c, line); }
            if let Some(source) = &credit.source { row(b, c, None, "Source", "", source, None, Action::Open(source.clone())); }
            if credit.noncommercial { notice(b, c, "Free for personal use, under the listed license."); }
        });
    }
    group(b, c, "", |b, c| {
        action_button(b, c, "Show in folder", Action::Reveal(w.dir.clone()));
        if w.editable() && !w.active { action_button(b, c, "Remove", Action::RemoveWallpaper(i)); }
        else if w.editable() { notice(b, c, "Choose another wallpaper before removing this one."); }
    });
}

pub fn can_add(app: &App) -> bool {
    matches!(&app.overlay, Some(Overlay::AddWallpaper { source: Some(Source::Web), .. }) if !app.text(&FieldId::AddUrl).trim().is_empty())
        || matches!(&app.overlay, Some(Overlay::AddWallpaper { picked: Some(path), .. }) if !path.is_empty())
}

pub fn add(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Add a wallpaper", "Pictures, live sources and your own apps");
    let (source, picked) = match &app.overlay {
        Some(Overlay::AddWallpaper { source, picked }) => (*source, picked.as_deref()),
        _ => (None, None),
    };
    group(b, c, "From", |b, c| {
        for (kind, icon, label, caption) in [
            (Source::File, glyph::FILE, "A file", "Picture, video, shader or 3D scene"),
            (Source::Folder, glyph::FOLDER, "A folder", "A slideshow of pictures"),
            (Source::Web, glyph::WALLPAPERS, "A web page", "A link or an HTML file"),
            (Source::App, glyph::APP, "An app", "A program drawn as your wallpaper"),
        ] { row(b, c, Some(icon), label, "", caption, Some(source == Some(kind)), Action::AddSource(kind)); }
    });
    if source == Some(Source::Web) {
        group(b, c, "Web page", |b, c| {
            field(app, b, c, FieldId::AddUrl, "Address", "https://example.com", "");
            action_button(b, c, "Choose an HTML file", Action::AddChoose(Source::Web));
        });
    }
    if let Some(path) = picked { group(b, c, "Selected source", |b, c| notice(b, c, path)); }
    if source.is_some() {
        group(b, c, if source == Some(Source::App) { "The app" } else { "Details" }, |b, c| {
            field(app, b, c, FieldId::AddName, "Name", "My wallpaper", "");
            if source == Some(Source::App) {
                field(app, b, c, FieldId::AddArgs, "Arguments", "--embed {hwnd}", "");
                notice(b, c, "{hwnd} becomes the wallpaper window. The app must support being embedded.");
                select(b, c, "Presets", Value::Null, Edit::new(DocId::Studio, "/add_args_preset", "Arguments"),
                    vec![(Value::Null, "Choose a preset".into()), (json!(crate::library::UNITY_ARGS), "Unity player".into()), (json!("--embed {hwnd}"), "Embed window".into()), (json!("-parentHWND {hwnd}"), "Wallpaper Engine".into())]);
            } else if matches!(source, Some(Source::File | Source::Folder)) {
                let value = app.text(&FieldId::Text(DocId::Studio, "/add_fit".into()));
                select(b, c, "Fit", json!(if value.is_empty() { "cover" } else { value }), Edit::new(DocId::Studio, "/add_fit", "Fit"),
                    [("cover", "Fill"), ("contain", "Fit"), ("stretch", "Stretch"), ("center", "Center")].into_iter().map(|(v, l)| (json!(v), l.into())).collect());
            }
        });
    }
}
