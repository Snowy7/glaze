//! Home: the desktop as it is now (click anything to open its editor), the wallpapers strip,
//! mypaper's status and cost, and "Your desktop" — a row per thing, one click from its editor.
//! No inspector: Home duplicates no editor (spec 4.1).

use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Rect, Role};
use serde_json::Value;

use crate::actions::{Action, Bar, Page, Section};
use crate::app::App;
use crate::docs::DocId;
use crate::stage::{self, Show};
use crate::theme::glyph;
use crate::ui::{B, Col, card, nav_row};

pub fn title(app: &App) -> (String, String) {
    let wallpaper = app.library.active().map_or("No wallpaper".into(), |i| app.library.items[i].name.clone());
    let displays = app.monitors.len();
    (
        "Home".into(),
        format!("{wallpaper} · {displays} display{}", if displays == 1 { "" } else { "s" }),
    )
}

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let mut right = right - b.px(32.0);
    b.icon_button(key("peek"), Rect::new(right, y, b.px(32.0), b.px(32.0)), glyph::EYE, "Peek at your desktop (Ctrl+D)", false, true, Action::Peek);
    let paused = app.get(DocId::Config, "/paused").and_then(Value::as_bool).unwrap_or(false);
    let (label, g) = if paused { ("Resume", glyph::PLAY) } else { ("Pause", glyph::PAUSE) };
    if app.system.running() {
        let w = b.button_width(label, true, false);
        right -= w + b.px(16.0);
        b.button(key("home-pause"), Rect::new(right, y, w, b.px(32.0)), label, Some(g), ButtonKind::Secondary, true, Action::Pause);
    }
    right - b.px(8.0)
}

/// The stage's area on Home: the left column above the chips and strip.
pub fn stage_area(app: &App, body: Rect) -> Rect {
    let s = app.scale;
    let right = 316.0 * s + 20.0 * s;
    let w = body.width() - 48.0 * s - right;
    let h = (body.height() - 48.0 * s - 190.0 * s).max(120.0 * s);
    Rect::new(body.left + 24.0 * s, body.top + 24.0 * s, w, h)
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let area = stage_area(app, body);
    let frame = stage::frame(app, area);
    let frame = Rect::new(area.left, frame.top.min(area.top), frame.width(), frame.height());
    let running = app.system.running();
    stage::draw(app, b, frame, Show::Desktop, stage::home_pick);
    if !running {
        // Off: the preview dims to 40% with a card that starts mypaper.
        b.fill(frame, b.px(12.0), p.bg.with_alpha(0.6));
        let c = Rect::new(frame.left + (frame.width() - b.px(300.0)) / 2.0, frame.top + (frame.height() - b.px(140.0)) / 2.0, b.px(300.0), b.px(140.0));
        card(b, c);
        let display = b.font(Font::Title1);
        b.text_in(Rect::new(c.left, c.top + b.px(22.0), c.width(), b.px(28.0)), "mypaper is off", &display, p.ink1, Align::Center);
        let caption = b.font(Font::Caption);
        b.text_in(Rect::new(c.left, c.top + b.px(52.0), c.width(), b.px(18.0)), "Windows shows its own desktop.", &caption, p.ink3, Align::Center);
        let w = b.button_width("Start mypaper", true, false);
        b.button(key("home-start"), Rect::new(c.left + (c.width() - w) / 2.0, c.top + b.px(84.0), w, b.px(32.0)), "Start mypaper", Some(glyph::PLAY), ButtonKind::Primary, true, Action::StartMypaper);
    }
    // Display chips and the hint.
    let chips_y = frame.bottom + b.px(12.0);
    crate::pages::display_chips(app, b, frame.left, chips_y);
    let caption = b.font(Font::Caption);
    let hint = if app.glass.borrow().unavailable && running { "Glass shows when mypaper is running" } else { "Click anything to change it" };
    b.text_in(Rect::new(frame.left, chips_y, frame.width(), b.px(28.0)), hint, &caption, p.ink3, Align::End);
    strip(app, b, Rect::new(frame.left, chips_y + b.px(44.0), frame.width(), b.px(120.0)));
    // The right column.
    let rx = area.right + b.px(20.0);
    let rw = body.right - b.px(24.0) - rx;
    let mut y = body.top + b.px(24.0);
    y = status_card(app, b, Rect::new(rx, y, rw, 0.0)) + b.px(14.0);
    y = desktop_card(app, b, Rect::new(rx, y, rw, 0.0)) + b.px(14.0);
    keys_card(b, Rect::new(rx, y, rw, 0.0));
}

/// The current wallpaper and five recent ones, then Add. A click uses one (with Undo).
fn strip(app: &App, b: &mut B, rect: Rect) {
    let p = *b.p;
    let heading = b.font(Font::Heading);
    b.text_at(rect.left, rect.top, "Wallpapers", &heading, p.ink1);
    // "All wallpapers ›".
    let caption = b.font(Font::Caption);
    let label = "All wallpapers";
    let (lw, _) = b.measure(label, &caption);
    let link = Rect::new(rect.right - lw - b.px(18.0), rect.top - b.px(2.0), lw + b.px(18.0), b.px(22.0));
    let k = key("all-wallpapers");
    let ink = if b.hovered(k) { p.ink1 } else { p.ink3 };
    b.text_in(Rect::new(link.left, link.top, lw + b.px(2.0), link.height()), label, &caption, ink, Align::Start);
    b.icon(Rect::new(link.right - b.px(14.0), link.top, b.px(14.0), link.height()), glyph::CHEVRON_RIGHT, 9.0, ink);
    b.hit(k, link, Role::Link, label, Action::Nav(Page::Wallpapers), mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, link, b.px(4.0));
    let recent = recent(app);
    let (tw, th) = (b.px(110.0), b.px(69.0));
    let gap = b.px(10.0);
    let fits = ((rect.width() - b.px(70.0)) / (tw + gap)).floor().max(1.0) as usize;
    let y = rect.top + b.px(26.0);
    let mut x = rect.left;
    let small = b.font(Font::Caption);
    b.group(key("home-strip"), |b| {
        for &i in recent.iter().take(fits.min(5)) {
            let w = &app.library.items[i];
            let pic = Rect::new(x, y, tw, th);
            let k = key(("strip", i));
            match w.preview.as_ref().or(w.video.as_ref()).and_then(|path| app.images.borrow_mut().get(path, (tw as u32 * 2, th as u32 * 2))) {
                Some(image) => {
                    b.fill(pic, b.px(8.0), p.panel);
                    stage::image_cover(b, pic, &image, 1.0);
                    // Round the picture's corners with a ring in the page colour.
                    b.stroke(pic.inflate(b.px(3.0)), b.px(11.0), b.px(6.0), p.bg);
                }
                None => b.fill(pic, b.px(8.0), p.panel),
            }
            b.tile_frame(k, pic, b.px(8.0), w.active, p.bg);
            b.text_in(Rect::new(x, pic.bottom + b.px(6.0), tw, b.px(16.0)), &w.name, &small, if w.active { p.ink1 } else { p.ink3 }, Align::Start);
            b.hit_checked(k, Rect::new(x, y, tw, th + b.px(24.0)), Role::Button, &format!("Use {}", w.name), Some(w.active), Action::UseWallpaper(i), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, pic, b.px(8.0));
            x += tw + gap;
        }
        let add = Rect::new(x, y, b.px(60.0), th);
        let k = key("strip-add");
        b.fill(add, b.px(8.0), if b.hovered(k) { p.hover } else { p.panel });
        b.stroke(add, b.px(8.0), 1.0, p.edge);
        b.icon(add, glyph::ADD, 14.0, p.ink3);
        b.text_in(Rect::new(x, add.bottom + b.px(6.0), add.width(), b.px(16.0)), "Add", &small, p.ink3, Align::Start);
        b.hit(k, add, Role::Button, "Add a wallpaper", Action::AddWallpaper, mypaper_ui::controls::Behavior::Click);
        b.focus_ring(k, add, b.px(8.0));
    });
}

/// The active wallpaper, then the ones used recently (Studio remembers), then others.
fn recent(app: &App) -> Vec<usize> {
    let mut out: Vec<usize> = app.library.active().into_iter().collect();
    if let Some(list) = app.get(DocId::Studio, "/recent_wallpapers").and_then(Value::as_array) {
        for name in list.iter().filter_map(Value::as_str) {
            if let Some(i) = app.library.items.iter().position(|w| w.apply_as == name)
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
    }
    for (i, w) in app.library.items.iter().enumerate() {
        if out.len() >= 6 {
            break;
        }
        if !out.contains(&i) && w.error.is_none() {
            out.push(i);
        }
    }
    out
}

fn status_card(app: &App, b: &mut B, rect: Rect) -> f32 {
    let p = *b.p;
    let h = b.px(168.0);
    let r = Rect::new(rect.left, rect.top, rect.width(), h);
    card(b, r);
    let paused = app.get(DocId::Config, "/paused").and_then(Value::as_bool).unwrap_or(false);
    let running = app.system.running();
    let title = if !running { "Off" } else if paused { "Paused" } else { "Playing" };
    let x = r.left + b.px(16.0);
    let mid = r.top + b.px(28.0);
    if paused && running {
        b.icon(Rect::new(x, mid - b.px(7.0), b.px(10.0), b.px(14.0)), glyph::PAUSE, 10.0, p.ink1);
    } else {
        b.dot(mypaper_ui::Point::new(x + b.px(4.0), mid), running);
    }
    let t2 = b.font(Font::Title2);
    b.text_in(Rect::new(x + b.px(17.0), mid - b.px(10.0), b.px(160.0), b.px(20.0)), title, &t2, p.ink1, Align::Start);
    let caption = b.font(Font::Caption);
    let name = app.library.active().map_or("No wallpaper", |i| app.library.items[i].name.as_str());
    b.text_in(Rect::new(x + b.px(17.0), mid + b.px(12.0), r.width() - b.px(140.0), b.px(16.0)), name, &caption, p.ink3, Align::Start);
    if running {
        let label = if paused { "Resume" } else { "Pause" };
        let w = b.button_width(label, true, true);
        b.button(key("status-pause"), Rect::new(r.right - b.px(16.0) - w, r.top + b.px(14.0), w, b.px(28.0)), label, Some(if paused { glyph::PLAY } else { glyph::PAUSE }), ButtonKind::Secondary, true, Action::Pause);
    } else {
        let w = b.button_width("Start", true, true);
        b.button(key("status-start"), Rect::new(r.right - b.px(16.0) - w, r.top + b.px(14.0), w, b.px(28.0)), "Start", Some(glyph::PLAY), ButtonKind::Primary, true, Action::StartMypaper);
    }
    b.hline(x, r.right - b.px(16.0), r.top + b.px(76.0), p.line);
    let (cpu, mem) = app.system.total();
    let gpu = match app.get(DocId::Config, "/engine/gpu").and_then(Value::as_str).unwrap_or("integrated") {
        "high-performance" => "High performance",
        "auto" => "Windows decides",
        _ => "Integrated",
    };
    let fps = app.get(DocId::Config, "/engine/fps").and_then(Value::as_u64).map_or("its own fps".into(), |f| format!("{f} fps"));
    let display = app.display().map_or(String::new(), |m| format!("{} · {}×{}", app.monitors.len(), m.rect.width, m.rect.height));
    let mut y = r.top + b.px(84.0);
    for (k, v) in [
        ("Graphics", format!("{gpu} · {fps}")),
        ("Using", if running { format!("{cpu:.1}% CPU · {mem:.0} MB") } else { "Nothing".into() }),
        ("Displays", display),
    ] {
        b.text_in(Rect::new(x, y, b.px(100.0), b.px(24.0)), k, &caption, p.ink3, Align::Start);
        b.text_in(Rect::new(x + b.px(100.0), y, r.width() - b.px(132.0), b.px(24.0)), &v, &caption, p.ink1, Align::End);
        y += b.px(24.0);
    }
    r.bottom
}

fn desktop_card(app: &App, b: &mut B, rect: Rect) -> f32 {
    let p = *b.p;
    let rows = 6.0;
    let h = b.px(48.0) + rows * b.px(50.0) + b.px(8.0);
    let r = Rect::new(rect.left, rect.top, rect.width(), h);
    card(b, r);
    let heading = b.font(Font::Heading);
    b.text_at(r.left + b.px(16.0), r.top + b.px(14.0), "Your desktop", &heading, p.ink1);
    let mut col = Col {
        x: r.left + b.px(16.0),
        y: r.top + b.px(42.0),
        w: r.width() - b.px(32.0),
        view: 0,
    };
    let look = crate::pages::look::summary(app);
    let widgets = crate::pages::widgets::count_here(app);
    let display = app.nav.display + 1;
    let top = crate::pages::bars::design_name(app, Bar::Top);
    let dock = crate::pages::bars::design_name(app, Bar::Dock);
    let menu = if app.get(DocId::Config, "/desktop_menu/enabled").and_then(Value::as_bool).unwrap_or(true) { "mypaper's menu" } else { "Windows' menu" };
    nav_row(b, &mut col, glyph::LOOK, "Look", &look, Action::Nav(Page::Look));
    nav_row(b, &mut col, glyph::WIDGETS, "Widgets", &format!("{widgets} on Display {display}"), Action::Nav(Page::Widgets));
    nav_row(b, &mut col, glyph::BARS, "Top bar", &top, Action::SelectBar(Some(Bar::Top)));
    nav_row(b, &mut col, glyph::DESKTOP, "Dock", &dock, Action::SelectBar(Some(Bar::Dock)));
    nav_row(b, &mut col, glyph::POINTER, "Right-click menu", menu, Action::Section(Section::Desktop));
    nav_row(b, &mut col, glyph::INTEGRATIONS, "Integrations", "Media, notifications, weather", Action::Nav(Page::Integrations));
    r.bottom
}

fn keys_card(b: &mut B, rect: Rect) {
    let p = *b.p;
    let r = Rect::new(rect.left, rect.top, rect.width(), b.px(96.0));
    card(b, r);
    let heading = b.font(Font::Heading);
    b.text_at(r.left + b.px(16.0), r.top + b.px(14.0), "If anything goes wrong", &heading, p.ink1);
    b.keys(r.left + b.px(16.0), r.top + b.px(40.0), &["Ctrl", "Alt", "Shift", "Backspace"]);
    let caption = b.font(Font::Caption);
    b.text_at(r.left + b.px(16.0), r.top + b.px(68.0), "puts Windows back as it was.", &caption, p.ink3);
}
