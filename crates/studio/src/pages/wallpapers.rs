//! Wallpapers (D1): the library grouped by category, with search and the All / Yours / Built in
//! filter. A click selects a card (D4); Use, a double-click or Enter applies it, with Undo. The
//! inspector holds a wallpaper's performance, its fit and mouse, an app's arguments, the look it
//! brings, its credit, and Show in folder or Remove. Add wallpaper is a sheet: four sources,
//! then the details.

use mypaper_ui::controls::Glyph;
use mypaper_theme::Fit;
use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Rect, Rgba, Role};
use serde_json::{Value, json};

use crate::actions::{Action, Choices, Edit, FieldId, Page, Source};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op, token};
use crate::library::{CATEGORIES, Kind, NewWallpaper, Origin, Wallpaper};
use crate::stage::image_cover;
use crate::theme::glyph;
use crate::ui::{B, Col, inspector, section, text_field};
use crate::view::split;

pub fn title(app: &App) -> (String, String) {
    ("Wallpapers".into(), app.library.items.len().to_string())
}

const FILTERS: [&str; 3] = ["All", "Yours", "Built in"];

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let w = b.button_width("Add wallpaper", true, false);
    let mut right = right - w;
    b.button(key("add-wallpaper"), Rect::new(right, y, w, b.px(32.0)), "Add wallpaper", Some(glyph::ADD), ButtonKind::Primary, true, Action::AddWallpaper);
    right -= b.px(8.0);
    let seg = b.segmented(
        key("wallpaper-filter"),
        Rect::new(right - b.px(400.0), y, b.px(400.0), b.px(32.0)),
        &FILTERS,
        Some(usize::from(app.nav.filter.min(2))),
        "Show",
        false,
        |i| Action::WallpaperFilter(i as u8),
    );
    right = seg.left - b.px(8.0);
    let field = Rect::new(right - b.px(200.0), y, b.px(200.0), b.px(32.0));
    text_field(app, b, field, FieldId::WallpaperSearch, Some(glyph::SEARCH), "Search wallpapers", "", "Search wallpapers");
    field.left - b.px(8.0)
}

/// The wallpapers the filter and search let through, by index.
fn shown(app: &App) -> Vec<usize> {
    let needle = app.text(&FieldId::WallpaperSearch).trim().to_lowercase();
    let own = |w: &Wallpaper| {
        app.get(DocId::Config, &format!("/engine/wallpapers/{}", token(&w.id))).is_some()
    };
    app.library
        .items
        .iter()
        .enumerate()
        .filter(|(_, w)| match app.nav.filter {
            1 => w.origin != Origin::Bundled,
            2 => w.origin == Origin::Bundled,
            3 => own(w),
            _ => true,
        })
        .filter(|(_, w)| needle.is_empty() || needle.split_whitespace().all(|n| w.haystack().contains(n)))
        .map(|(i, _)| i)
        .collect()
}

/// The selected wallpaper: the user's pick, else the one on the desktop.
fn selected(app: &App) -> Option<usize> {
    app.nav
        .wallpaper
        .filter(|&i| i < app.library.items.len())
        .or(app.library.active())
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let (grid, side) = split(app, body);
    let list = shown(app);
    let view_key = key("wallpaper-grid");
    let offset = app.nav.scroll(view_key);
    let selected = selected(app);
    b.scroll(view_key, grid, offset, Action::None, |b, top| {
        let pad = b.px(24.0);
        let mut y = top + b.px(20.0);
        let x = grid.left + pad;
        let w = grid.width() - pad * 2.0;
        if list.is_empty() {
            let needle = app.text(&FieldId::WallpaperSearch).trim().to_owned();
            if needle.is_empty() {
                crate::ui::empty(b, Rect::new(grid.left, top, grid.width(), grid.height()), glyph::WALLPAPERS, "Your own wallpapers show here.", "Pictures, videos, web pages and apps.", Some(("Add wallpaper", Action::AddWallpaper)));
            } else {
                crate::ui::empty(b, Rect::new(grid.left, top, grid.width(), grid.height()), glyph::SEARCH, &format!("Nothing called '{needle}'."), "Search everything finds settings and widgets too.", Some(("Search everything", Action::Palette)));
            }
            return top + grid.height();
        }
        let gap = b.px(16.0);
        let columns = (((w + gap) / (b.px(200.0) + gap)).floor() as usize).max(1);
        let card_w = (w - gap * (columns as f32 - 1.0)) / columns as f32;
        let card_h = card_w * 10.0 / 16.0;
        for category in CATEGORIES {
            let items: Vec<usize> = list.iter().copied().filter(|&i| app.library.items[i].category() == category).collect();
            if items.is_empty() {
                continue;
            }
            let strong = b.font(Font::Heading);
            let (tw, _) = b.measure(category, &strong);
            b.text_at(x, y + b.px(4.0), category, &strong, p.ink1);
            let caption = b.font(Font::Caption);
            b.text_at(x + tw + b.px(8.0), y + b.px(5.0), &items.len().to_string(), &caption, p.ink4);
            y += b.px(32.0);
            b.group(key(("wallpaper-group", category)), |b| {
                for (n, &i) in items.iter().enumerate() {
                    let cx = x + (n % columns) as f32 * (card_w + gap);
                    if n > 0 && n % columns == 0 {
                        y += card_h + b.px(58.0);
                    }
                    card(app, b, i, Rect::new(cx, y, card_w, card_h), selected == Some(i));
                }
            });
            y += card_h + b.px(58.0) + b.px(8.0);
        }
        y + b.px(8.0)
    });
    if let Some(i) = selected {
        inspector(app, b, side, key("wallpaper-inspector"), |b, col| details(app, b, col, i));
    }
}

fn card(app: &App, b: &mut B, i: usize, pic: Rect, selected: bool) {
    let p = *b.p;
    let w = &app.library.items[i];
    let k = key(("wallpaper", i));
    let hot = b.hovered(k) || b.input.pointer.is_some_and(|pt| pic.contains(pt));
    let size = ((pic.width() * 1.5) as u32, (pic.height() * 1.5) as u32);
    match w.preview.as_ref().or(w.video.as_ref()).and_then(|path| app.images.borrow_mut().get(path, size)) {
        Some(image) => {
            b.fill(pic, b.px(10.0), p.panel);
            image_cover(b, pic, &image, 1.0);
            b.stroke(pic.inflate(b.px(3.0)), b.px(13.0), b.px(6.0), p.bg);
        }
        None => {
            b.fill(pic, b.px(10.0), p.panel);
            b.icon(pic, kind_glyph(w.kind), 22.0, p.ink4);
        }
    }
    if w.error.is_some() {
        b.fill(pic, b.px(10.0), Rgba::new(0.0, 0.0, 0.0, 0.55));
        b.icon(Rect::new(pic.left, pic.top, pic.width(), pic.height() - b.px(16.0)), glyph::ALERT, 18.0, Rgba::new(1.0, 1.0, 1.0, 0.9));
        let caption = b.font(Font::Caption);
        b.text_in(Rect::new(pic.left, pic.top + pic.height() / 2.0 + b.px(4.0), pic.width(), b.px(18.0)), "Can't load", &caption, Rgba::new(1.0, 1.0, 1.0, 0.9), Align::Center);
    }
    if selected {
        b.stroke(pic.inflate(b.px(4.0)), b.px(14.0), b.px(2.0), p.ink1);
    } else if hot {
        b.stroke(pic.inflate(b.px(4.0)), b.px(14.0), b.px(2.0), p.selected);
    }
    if w.active {
        b.badge(pic.left + b.px(8.0), pic.top + b.px(8.0), "On desktop", None, true);
    }
    let mut bx = pic.left + b.px(8.0);
    let by = pic.bottom - b.px(28.0);
    if w.credit.as_ref().is_some_and(|c| c.noncommercial) {
        bx += b.overlay_badge(bx, by, "Personal use", None) + b.px(4.0);
    }
    if w.brings_look {
        b.overlay_badge(bx, by, "Brings a look", Some(glyph::PALETTE));
    }
    let strong = b.font(Font::Heading);
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(pic.left, pic.bottom + b.px(8.0), pic.width(), b.px(18.0)), &w.name, &strong, p.ink1, Align::Start);
    let detail = match &w.credit {
        Some(c) if w.origin == Origin::Bundled && w.kind == Some(Kind::Video) => {
            format!("{} · {}", w.detail(), c.author.as_deref().unwrap_or("").split([';', ',']).next().unwrap_or(""))
        }
        _ if w.origin != Origin::Bundled => format!("{} · Yours", w.detail()),
        _ => w.detail(),
    };
    b.text_in(Rect::new(pic.left, pic.bottom + b.px(27.0), pic.width(), b.px(16.0)), &detail, &caption, p.ink3, Align::Start);
    b.hit_checked(k, Rect::new(pic.left, pic.top, pic.width(), pic.height() + b.px(44.0)), Role::RadioButton, &w.name, Some(selected), Action::SelectWallpaper(i), mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, pic, b.px(10.0));
    // Use, on hover (D4).
    if hot && !w.active && w.error.is_none() {
        let bw = b.button_width("Use", false, true);
        let r = Rect::new(pic.right - bw - b.px(8.0), pic.top + b.px(8.0), bw, b.px(28.0));
        b.button(key(("use", i)), r, "Use", None, ButtonKind::Primary, true, Action::UseWallpaper(i));
    }
}

pub fn kind_glyph(kind: Option<Kind>) -> Glyph {
    match kind {
        Some(Kind::Image) => glyph::IMAGE,
        Some(Kind::Slideshow) => glyph::SLIDESHOW,
        Some(Kind::Video) => glyph::VIDEO,
        Some(Kind::Shader) => glyph::SHADER,
        Some(Kind::Scene) => glyph::SCENE,
        Some(Kind::Web) => glyph::GLOBE,
        Some(Kind::App) => glyph::APP,
        None => glyph::ALERT,
    }
}

/// The inspector of wallpaper `i`.
fn details(app: &App, b: &mut B, col: &mut Col, i: usize) {
    let p = *b.p;
    let w = &app.library.items[i];
    let x = col.x + b.px(16.0);
    let cw = col.w - b.px(32.0);
    let mut y = col.y + b.px(16.0);
    let pic = Rect::new(x, y, cw, cw * 10.0 / 16.0);
    match w.preview.as_ref().or(w.video.as_ref()).and_then(|path| app.images.borrow_mut().get(path, ((pic.width() * 1.5) as u32, (pic.height() * 1.5) as u32))) {
        Some(image) => {
            image_cover(b, pic, &image, 1.0);
            b.stroke(pic.inflate(b.px(3.0)), b.px(11.0), b.px(6.0), p.chrome);
        }
        None => b.fill(pic, b.px(8.0), p.panel),
    }
    y = pic.bottom + b.px(14.0);
    let t2 = b.font(Font::Title2);
    b.text_in(Rect::new(x, y, cw, b.px(20.0)), &w.name, &t2, p.ink1, Align::Start);
    let caption = b.font(Font::Caption);
    let detail = format!("{} · {}", w.detail(), w.category());
    b.text_in(Rect::new(x, y + b.px(22.0), cw, b.px(16.0)), &detail, &caption, p.ink3, Align::Start);
    y += b.px(50.0);
    let (label, enabled) = if w.active {
        ("On your desktop", false)
    } else if w.error.is_some() {
        ("Can't load", false)
    } else {
        ("Use this wallpaper", true)
    };
    b.button(key("use-this"), Rect::new(x, y, cw, b.px(32.0)), label, None, ButtonKind::Primary, enabled, Action::UseWallpaper(i));
    col.y = y + b.px(32.0 + 10.0);
    if let Some(error) = &w.error {
        section(b, col, "Why it can't load", |b, c| {
            let caption = b.font(Font::Caption);
            let first = error.lines().next().unwrap_or(error);
            c.y += b.paragraph(c.x, c.y, c.w, first, &caption, b.px(16.0), b.p.ink2);
            c.y += b.px(8.0);
            let bw = b.button_width("Show in folder", true, true);
            b.button(key("broken-folder"), Rect::new(c.x, c.y, bw, b.px(28.0)), "Show in folder", Some(glyph::FOLDER), ButtonKind::Secondary, true, Action::Reveal(w.dir.clone()));
            c.y += b.px(32.0);
        });
        return;
    }
    // Performance: this wallpaper's own GPU and frame rate, or the Settings ones.
    section(b, col, "Performance", |b, c| {
        let base = format!("/engine/wallpapers/{}", token(&w.id));
        let global_gpu = app.get(DocId::Config, "/engine/gpu").and_then(Value::as_str).unwrap_or("integrated");
        let gpu_name = |v: &str| match v {
            "high-performance" => "High performance",
            "auto" => "Let Windows choose",
            _ => "Integrated",
        };
        let gpu = app.get(DocId::Config, &format!("{base}/gpu")).and_then(Value::as_str).map(str::to_owned);
        select_row(app, b, c, "Graphics card", &format!("{} · {}", if gpu.is_some() { "Its own" } else { "Same as Settings" }, gpu_name(gpu.as_deref().unwrap_or(global_gpu))),
            gpu.as_deref().map_or("Same as Settings".into(), |g| gpu_name(g).to_owned()),
            Choices {
                edit: Edit::new(DocId::Config, format!("{base}/gpu"), "Graphics card"),
                options: vec![
                    (Value::Null, "Same as Settings".into()),
                    (json!("integrated"), "Integrated".into()),
                    (json!("high-performance"), "High performance".into()),
                    (json!("auto"), "Let Windows choose".into()),
                ],
                current: Some(gpu.map_or(Value::Null, Value::String)),
            });
        let fps = app.get(DocId::Config, &format!("{base}/fps")).and_then(Value::as_u64);
        let own = w.engine.fps.map_or("the wallpaper's own rate".into(), |f| format!("its own {f} fps"));
        select_row(app, b, c, "Frame rate", &format!("The wallpaper asks for {own}"),
            fps.map_or("Same as Settings".into(), |f| format!("{f} fps")),
            Choices {
                edit: Edit::new(DocId::Config, format!("{base}/fps"), "Frame rate"),
                options: [Value::Null, json!(24), json!(30), json!(60), json!(120)]
                    .into_iter()
                    .map(|v| {
                        let label = v.as_u64().map_or("Same as Settings".into(), |f| format!("{f} fps"));
                        (v, label)
                    })
                    .collect(),
                current: Some(fps.map_or(Value::Null, |f| json!(f))),
            });
    });
    // On the screen: fit, for pictures, slideshows and videos of your own.
    if w.kind.is_some_and(Kind::uses_fit) {
        section(b, col, "On the screen", |b, c| {
            let fit = theme_value(w, "/wallpaper/fit").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "cover".into());
            let items = ["Fill", "Fit", "Stretch", "Center"];
            let values = ["cover", "contain", "stretch", "center"];
            let at = values.iter().position(|v| *v == fit);
            let dir = w.dir.clone();
            b.segmented(key("fit"), Rect::new(c.x, c.y, c.w, b.px(32.0)), &items, at, "Fit", true, |n| {
                if w.editable() { Action::Set(theme_edit(&dir, "/wallpaper/fit", "Fit"), json!(values[n])) } else { Action::CopyWallpaper(i) }
            });
            c.y += b.px(36.0);
            if !w.editable() {
                hint(b, c, "A built-in wallpaper: changing it makes your own copy.");
            }
        });
    }
    // Mouse: web pages and 3D scenes can follow it.
    if let Some(on) = w.interactive {
        section(b, col, "Mouse", |b, c| {
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            crate::ui::label(b, row, "React to the mouse", "Clicks and moves reach the wallpaper", None);
            let action = if w.editable() { Action::Toggle(theme_edit(&w.dir, "/wallpaper/interactive", "React to the mouse"), on) } else { Action::CopyWallpaper(i) };
            b.switch(key("interactive"), mypaper_ui::Point::new(c.x + c.w - b.px(36.0), c.y + b.px(10.0)), on, "React to the mouse", action);
            c.y += b.px(44.0);
        });
    }
    // App: its arguments.
    if w.kind == Some(Kind::App) {
        section(b, col, "App", |b, c| {
            let args = theme_value(w, "/wallpaper/args")
                .and_then(|v| v.as_array().cloned())
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let caption = b.font(Font::Caption);
            b.text_at(c.x, c.y, "Start it with", &caption, b.p.ink3);
            c.y += b.px(20.0);
            let field = FieldId::Text(DocId::Studio, format!("/app_args/{}", token(&w.id)).into());
            text_field(app, b, Rect::new(c.x, c.y, c.w, b.px(32.0)), field, None, "No arguments", &args, "Arguments");
            c.y += b.px(38.0);
            hint(b, c, "{hwnd} is the window it draws into. A Unity player wants -parentHWND {hwnd} delayed.");
        });
    }
    // Its look.
    section(b, col, "Its look", |b, c| {
        let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
        let (title, line) = if w.brings_look { ("Brings a look", "Your own choices still win") } else { ("No look of its own", "Your look applies as it is") };
        crate::ui::label(b, row, title, line, None);
        c.y += b.px(48.0);
        let (label, action) = if w.editable() {
            ("Edit its look", Action::LookScope(true))
        } else {
            ("Make a copy to edit", Action::CopyWallpaper(i))
        };
        let bw = b.button_width(label, true, true);
        b.button(key("its-look"), Rect::new(c.x, c.y, bw, b.px(28.0)), label, Some(if w.editable() { glyph::LOOK } else { glyph::COPY }), ButtonKind::Secondary, true, action);
        c.y += b.px(32.0);
    });
    // Credit.
    if let Some(credit) = &w.credit {
        section(b, col, "Credit", |b, c| {
            let caption = b.font(Font::Caption);
            let mono = b.font(Font::Mono);
            for (k, v, mono_text) in [
                ("By", credit.author.clone(), false),
                ("License", credit.license.clone(), false),
                ("Source", credit.source.as_ref().map(|s| s.trim_start_matches("https://").trim_start_matches("www.").to_owned()), true),
            ] {
                let Some(v) = v else { continue };
                b.text_in(Rect::new(c.x, c.y, b.px(64.0), b.px(24.0)), k, &caption, b.p.ink3, Align::Start);
                let lines = b.text.lines(&v, if mono_text { &mono } else { &caption }, c.w - b.px(64.0), 3);
                for (n, line) in lines.iter().enumerate() {
                    b.text_in(Rect::new(c.x + b.px(64.0), c.y + n as f32 * b.px(18.0), c.w - b.px(64.0), b.px(24.0)), line, if mono_text { &mono } else { &caption }, b.p.ink2, Align::Start);
                }
                if k == "Source"
                    && let Some(url) = &credit.source
                {
                    b.hit(key("credit-source"), Rect::new(c.x + b.px(64.0), c.y, c.w - b.px(64.0), b.px(24.0)), Role::Link, url, Action::Open(url.clone()), mypaper_ui::controls::Behavior::Click);
                }
                c.y += lines.len().max(1) as f32 * b.px(18.0) + b.px(8.0);
            }
            if credit.noncommercial {
                c.y += b.paragraph(c.x, c.y, c.w, "Free for personal use. Don't sell it, and share changes under the same license.", &caption, b.px(16.0), b.p.ink3);
            }
        });
    }
    // More.
    let open = crate::ui::more(app, b, col, crate::settings::Place::Page(Page::Wallpapers), "More", "");
    if open {
        section(b, col, "", |b, c| {
            let bw = b.button_width("Show in folder", true, true);
            b.button(key("reveal"), Rect::new(c.x, c.y, bw, b.px(28.0)), "Show in folder", Some(glyph::FOLDER), ButtonKind::Secondary, true, Action::Reveal(w.dir.clone()));
            if w.editable() && !w.active {
                let rw = b.button_width("Remove", true, true);
                b.button(key("remove-wallpaper"), Rect::new(c.x + bw + b.px(8.0), c.y, rw, b.px(28.0)), "Remove", Some(glyph::DELETE), ButtonKind::Secondary, true, Action::RemoveWallpaper(i));
            }
            c.y += b.px(32.0);
        });
    }
}

/// A row with a select at its right.
fn select_row(_app: &App, b: &mut B, c: &mut Col, label: &str, hint: &str, value: String, choices: Choices) {
    let h = b.px(44.0);
    let row = Rect::new(c.x, c.y, c.w, h);
    let w = b.select_width(&value).max(b.px(120.0));
    crate::ui::label(b, Rect::new(c.x, c.y, c.w - w - b.px(8.0), h), label, hint, None);
    b.select(key(("select", label)), Rect::new(row.right - w, c.y + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), &value, label, Action::Choose(choices));
    c.y += h + b.px(4.0);
}

fn hint(b: &mut B, c: &mut Col, text: &str) {
    let caption = b.font(Font::Caption);
    c.y += b.paragraph(c.x, c.y, c.w, text, &caption, b.px(16.0), b.p.ink3) + b.px(4.0);
}

/// A value in a wallpaper's own `theme.json`.
fn theme_value(w: &Wallpaper, path: &str) -> Option<Value> {
    let text = std::fs::read_to_string(w.dir.join(mypaper_theme::MANIFEST)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.pointer(path).cloned()
}

/// An edit of one of your wallpapers' `theme.json` (Studio's document for theme files is
/// addressed by folder: `Studio` doc ops whose path starts with `/theme:`).
fn theme_edit(dir: &std::path::Path, path: &str, label: &str) -> Edit {
    Edit::new(DocId::Studio, format!("/theme:{}{path}", token(&dir.display().to_string())), label)
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::SelectWallpaper(i) => {
            app.go(Page::Wallpapers);
            app.nav.wallpaper = Some(i);
        }
        Action::UseWallpaper(i) => use_wallpaper(app, i),
        Action::WallpaperFilter(f) => app.nav.filter = f,
        Action::WallpapersOwn => {
            app.go(Page::Wallpapers);
            app.nav.filter = 3;
        }
        Action::AddWallpaper => {
            app.go(Page::Wallpapers);
            app.overlay = Some(Overlay::AddWallpaper { source: None, picked: None });
        }
        Action::AddSource(source) => {
            if source == Source::Web {
                app.overlay = Some(Overlay::AddWallpaper { source: Some(source), picked: None });
                app.focus_field(FieldId::AddUrl);
                return;
            }
            let (title, folder, filters) = match source {
                Source::File => ("Choose a picture, video, shader or 3D scene", false, crate::dialogs::WALLPAPER_FILES),
                Source::Folder => ("Choose a folder of pictures", true, &[][..]),
                Source::App => ("Choose a program", false, crate::dialogs::PROGRAMS),
                Source::Web => unreachable!(),
            };
            if let Some(path) = crate::dialogs::pick(app.window, title, folder, filters) {
                let picked = path.display().to_string();
                app.texts.insert(FieldId::AddName, crate::library::suggested_name(&picked));
                app.overlay = Some(Overlay::AddWallpaper { source: Some(source), picked: Some(picked) });
            }
        }
        Action::AddChoose(_) => {
            if let Some(path) = crate::dialogs::pick(app.window, "Choose a web page", false, crate::dialogs::PAGES) {
                let picked = path.display().to_string();
                app.texts.insert(FieldId::AddName, crate::library::suggested_name(&picked));
                app.overlay = Some(Overlay::AddWallpaper { source: Some(Source::Web), picked: Some(picked) });
            }
        }
        Action::AddBack => app.overlay = Some(Overlay::AddWallpaper { source: None, picked: None }),
        Action::AddCommit(use_it) => add(app, use_it),
        Action::CopyWallpaper(i) => copy(app, i),
        Action::RemoveWallpaper(i) => remove(app, i),
        _ => {}
    }
}

/// Uses wallpaper `i`: the config's theme, one undoable step, and a toast with Undo.
pub fn use_wallpaper(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else { return };
    if w.error.is_some() {
        app.alert(format!("{} can't load", w.name));
        return;
    }
    if w.active {
        return;
    }
    app.apply(&format!("Use {}", w.name), vec![Op { doc: DocId::Config, path: "/theme".into(), value: Some(json!(w.apply_as)) }]);
    let mut recent: Vec<Value> = app.get(DocId::Studio, "/recent_wallpapers").and_then(Value::as_array).cloned().unwrap_or_default();
    recent.retain(|v| v.as_str() != Some(&w.apply_as));
    recent.insert(0, json!(w.apply_as));
    recent.truncate(8);
    app.apply("Recent", vec![Op { doc: DocId::Studio, path: "/recent_wallpapers".into(), value: Some(Value::Array(recent)) }]);
    let text = if w.brings_look { format!("Using {} and its look", w.name) } else { format!("Using {}", w.name) };
    app.toast_undo(text);
}

fn add(app: &mut App, use_it: bool) {
    let Some(Overlay::AddWallpaper { source, picked }) = app.overlay.clone() else { return };
    let input = match source {
        Some(Source::Web) => picked.clone().unwrap_or_else(|| app.text(&FieldId::AddUrl).trim().to_owned()),
        _ => picked.clone().unwrap_or_default(),
    };
    if input.is_empty() {
        app.alert("Type a web address first");
        return;
    }
    if source == Some(Source::Web) && picked.is_none() && app.text(&FieldId::AddName).is_empty() {
        // The address was typed: go on to the details.
        app.texts.insert(FieldId::AddName, crate::library::suggested_name(&input));
        app.overlay = Some(Overlay::AddWallpaper { source, picked: Some(input) });
        app.blur_field();
        return;
    }
    let fit = match app.text(&FieldId::Text(DocId::Studio, "/add_fit".into())) {
        "contain" => Fit::Contain,
        "stretch" => Fit::Stretch,
        "center" => Fit::Center,
        _ => Fit::Cover,
    };
    let new = NewWallpaper {
        source: input,
        name: app.text(&FieldId::AddName).to_owned(),
        fit,
        args: app.text(&FieldId::AddArgs).to_owned(),
    };
    let root = match mypaper_config::user_themes_dir() {
        Ok(root) => root,
        Err(e) => return app.alert(format!("{e:#}")),
    };
    match crate::library::create_in(&root, &new) {
        Ok(dir) => {
            app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.overlay = None;
            app.blur_field();
            for id in [FieldId::AddUrl, FieldId::AddName, FieldId::AddArgs] {
                app.texts.remove(&id);
            }
            if let Some(i) = app.library.by_dir(&dir) {
                app.nav.wallpaper = Some(i);
                app.nav.filter = 0;
                if use_it {
                    use_wallpaper(app, i);
                } else {
                    app.toast(format!("Added {}", new.name.trim()), None);
                }
            }
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

fn copy(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else { return };
    let root = match mypaper_config::user_themes_dir() {
        Ok(root) => root,
        Err(e) => return app.alert(format!("{e:#}")),
    };
    match crate::library::duplicate_into(&root, &w.dir) {
        Ok(dir) => {
            app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.nav.wallpaper = app.library.by_dir(&dir);
            app.toast(format!("Made {} (copy): yours to change", w.name), None);
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

fn remove(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else { return };
    if !w.editable() || w.active {
        return;
    }
    match crate::library::recycle(&w.dir) {
        Ok(()) => {
            app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.nav.wallpaper = None;
            app.toast(format!("Moved {} to the Recycle Bin", w.name), Some(("Show".into(), Action::Open("shell:RecycleBinFolder".into()))));
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

/// The Add wallpaper sheet: four sources, then the details of what was picked.
pub fn add_sheet(app: &App, b: &mut B) {
    let p = *b.p;
    let Some(Overlay::AddWallpaper { source, picked }) = &app.overlay else { return };
    let rect = crate::view::sheet(app, b, "Add a wallpaper", b.px(880.0), b.px(if picked.is_some() { 440.0 } else { 482.0 }));
    let caption = b.font(Font::Body);
    let mut y = rect.top;
    if let Some(picked) = picked {
        details_step(app, b, rect, *source, picked);
        return;
    }
    b.text_at(rect.left, y, "Anything below becomes a wallpaper you can use, tune and share.", &caption, p.ink3);
    y += b.px(26.0);
    let gap = b.px(12.0);
    let tw = (rect.width() - gap) / 2.0;
    let th = b.px(88.0);
    let sources = [
        (Source::File, glyph::FILE, "A file", "A picture, a video, a shader (.wgsl) or a 3D scene (.glb, .gltf)"),
        (Source::Folder, glyph::FOLDER, "A folder", "Its pictures cross-fade as a slideshow"),
        (Source::Web, glyph::GLOBE, "A web page", "A website or an .html file, live"),
        (Source::App, glyph::APP, "An app", "A program that draws into the desktop, like a Unity player"),
    ];
    b.group(key("add-sources"), |b| {
        for (n, (s, g, title, line)) in sources.iter().enumerate() {
            let r = Rect::new(rect.left + (n % 2) as f32 * (tw + gap), y + (n / 2) as f32 * (th + gap), tw, th);
            let k = key(("source", n));
            let on = *source == Some(*s);
            b.fill(r, b.px(12.0), p.well);
            b.wash(k, r, b.px(12.0));
            if on {
                b.stroke(r, b.px(12.0), b.px(2.0), p.ink1);
            }
            let icon = Rect::new(r.left + b.px(18.0), r.top + (th - b.px(48.0)) / 2.0, b.px(48.0), b.px(48.0));
            b.fill(icon, b.px(12.0), p.fill);
            b.icon(icon, *g, 20.0, p.ink1);
            let strong = b.font(Font::Heading);
            let small = b.font(Font::Caption);
            b.text_at(icon.right + b.px(16.0), r.top + b.px(24.0), title, &strong, p.ink1);
            let lines = b.text.lines(line, &small, r.right - icon.right - b.px(32.0), 2);
            for (i, l) in lines.iter().take(2).enumerate() {
                b.text_at(icon.right + b.px(16.0), r.top + b.px(46.0) + i as f32 * b.px(16.0), l, &small, p.ink3);
            }
            b.hit_checked(k, r, Role::RadioButton, title, Some(on), Action::AddSource(*s), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, b.px(12.0));
        }
    });
    y += th * 2.0 + gap * 2.0;
    if *source == Some(Source::Web) {
        let panel = Rect::new(rect.left, y, rect.width(), b.px(60.0));
        b.fill(panel, b.px(12.0), p.well);
        let bw = b.button_width("Use this page", false, false);
        let field = Rect::new(panel.left + b.px(14.0), panel.top + b.px(14.0), panel.width() - bw - b.px(38.0), b.px(32.0));
        text_field(app, b, field, FieldId::AddUrl, None, "https://", "", "Web address");
        b.button(key("add-url"), Rect::new(field.right + b.px(10.0), field.top, bw, b.px(32.0)), "Use this page", None, ButtonKind::Primary, true, Action::AddCommit(true));
        y += b.px(72.0);
        let small = b.font(Font::Caption);
        let (w1, _) = b.measure("Or ", &small);
        b.text_at(rect.left, y, "Or ", &small, p.ink3);
        let link = "choose an .html file";
        let (lw, _) = b.measure(link, &small);
        let k = key("choose-html");
        b.text_at(rect.left + w1, y, link, &small, if b.hovered(k) { p.ink1 } else { p.ink2 });
        b.hline(rect.left + w1, rect.left + w1 + lw, y + b.px(15.0), p.control);
        b.hit(k, Rect::new(rect.left + w1, y - b.px(2.0), lw, b.px(20.0)), Role::Link, link, Action::AddChoose(Source::Web), mypaper_ui::controls::Behavior::Click);
        b.text_at(rect.left + w1 + lw, y, ". Pages pause while they're covered, and glass stays live over them.", &small, p.ink3);
        y += b.px(28.0);
    }
    let small = b.font(Font::Caption);
    let mut x = rect.left;
    for (g, label) in [(glyph::IMAGE, "Image"), (glyph::SLIDESHOW, "Slideshow"), (glyph::VIDEO, "Video"), (glyph::SHADER, "Shader"), (glyph::SCENE, "3D scene"), (glyph::GLOBE, "Web"), (glyph::APP, "App")] {
        b.icon(Rect::new(x, y + b.px(18.0), b.px(14.0), b.px(16.0)), g, 12.0, p.ink3);
        let (w, _) = b.measure(label, &small);
        b.text_at(x + b.px(18.0), y + b.px(17.0), label, &small, p.ink3);
        x += w + b.px(36.0);
    }
}

/// The details of a picked source: name, fit, an app's arguments; Add and use, or Add.
fn details_step(app: &App, b: &mut B, rect: Rect, source: Option<Source>, picked: &str) {
    let p = *b.p;
    let kind = crate::library::resolve(picked).map(|(k, _)| k).ok();
    let mut y = rect.top;
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(rect.left, y, rect.width(), b.px(18.0)), &format!("{} · {}", kind.map_or("Unknown", Kind::label), picked), &caption, p.ink3, Align::Start);
    y += b.px(30.0);
    b.text_at(rect.left, y, "Name", &caption, p.ink3);
    text_field(app, b, Rect::new(rect.left, y + b.px(20.0), rect.width(), b.px(32.0)), FieldId::AddName, None, "My wallpaper", "", "Name");
    y += b.px(66.0);
    if kind.is_some_and(Kind::uses_fit) {
        b.text_at(rect.left, y, "Fit", &caption, p.ink3);
        let current = app.text(&FieldId::Text(DocId::Studio, "/add_fit".into()));
        let values = ["cover", "contain", "stretch", "center"];
        let at = values.iter().position(|v| *v == current).or(Some(0));
        b.segmented(key("add-fit"), Rect::new(rect.left, y + b.px(20.0), b.px(360.0), b.px(32.0)), &["Fill", "Fit", "Stretch", "Center"], at, "Fit", true, |n| {
            Action::Set(Edit::new(DocId::Studio, "/add_fit", "Fit"), json!(values[n]))
        });
        y += b.px(66.0);
    }
    if kind == Some(Kind::App) {
        b.text_at(rect.left, y, "Start it with", &caption, p.ink3);
        let bw = b.button_width("Unity player", false, true);
        text_field(app, b, Rect::new(rect.left, y + b.px(20.0), rect.width() - bw - b.px(10.0), b.px(32.0)), FieldId::AddArgs, None, "No arguments ({hwnd} is the window it draws into)", "", "Arguments");
        b.button(key("unity-args"), Rect::new(rect.right - bw, y + b.px(22.0), bw, b.px(28.0)), "Unity player", None, ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Studio, "/add_args_preset", "Arguments"), json!(crate::library::UNITY_ARGS)));
        y += b.px(66.0);
    }
    let _ = (y, source);
    // Footer.
    let fy = rect.bottom - b.px(52.0);
    let back = b.button_width("Back", true, false);
    b.button(key("add-back"), Rect::new(rect.left, fy, back, b.px(32.0)), "Back", Some(glyph::CHEVRON_LEFT), ButtonKind::Ghost, true, Action::AddBack);
    let use_w = b.button_width("Add and use", false, false);
    b.button(key("add-use"), Rect::new(rect.right - use_w, fy, use_w, b.px(32.0)), "Add and use", None, ButtonKind::Primary, true, Action::AddCommit(true));
    let add_w = b.button_width("Add", false, false);
    b.button(key("add-only"), Rect::new(rect.right - use_w - add_w - b.px(8.0), fy, add_w, b.px(32.0)), "Add", None, ButtonKind::Secondary, true, Action::AddCommit(false));
}
