//! One frame of Studio: the window's chrome (sidebar, main panel, page bar, caption buttons),
//! the page, and whatever is over it (the palette, a sheet, the first run, a toast). Layout
//! follows plans/native/studio-redesign.md 8.2 and the mockups.

use mypaper_ui::controls::{Align, Builder, ButtonKind, Font, ScrollArea, Target, key};
use mypaper_ui::{Chrome, DisplayList, Point, Rect, Role, Text};
use serde_json::Value;

use crate::actions::{Action, Page};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Saved};
use crate::theme::glyph;
use crate::ui::B;

/// Where the window's parts are, in device pixels.
#[derive(Debug, Clone, Copy)]
pub struct Regions {
    pub sidebar: Rect,
    /// The rounded main panel (page bar and body).
    pub main: Rect,
    pub bar: Rect,
    pub body: Rect,
    /// The sidebar is an icon rail (narrow windows).
    pub rail: bool,
}

pub fn regions(app: &App) -> Regions {
    let s = app.scale;
    let (w, h) = (app.size.0 as f32, app.size.1 as f32);
    let rail = w < 1100.0 * s;
    let side = if rail { 64.0 } else { 232.0 } * s;
    let main = Rect::new(side, 8.0 * s, w - side, h - 8.0 * s);
    let bar = Rect::new(main.left, main.top, main.width(), 52.0 * s);
    let body = Rect::new(main.left, bar.bottom, main.width(), main.bottom - bar.bottom);
    Regions {
        sidebar: Rect::new(0.0, 0.0, side, h),
        main,
        bar,
        body,
        rail,
    }
}

/// The inspector's width at this window size (340, 380 from 1600 px).
pub fn inspector_width(app: &App) -> f32 {
    let w = app.size.0 as f32 / app.scale;
    (if w >= 1600.0 { 380.0 } else { 340.0 }) * app.scale
}

/// What one frame produced.
pub struct Built {
    pub list: DisplayList,
    pub semantics: Vec<(mypaper_ui::HitId, mypaper_ui::Semantics)>,
    pub targets: Vec<Target<Action>>,
    pub scrolls: Vec<ScrollArea>,
    pub chrome: Vec<(Rect, Chrome)>,
    pub regions: Vec<Rect>,
    pub overlay_from: usize,
}

pub fn build(app: &App, text: &mut Text) -> Built {
    let p = app.theme.palette;
    let ty = app.theme.ty.clone();
    let mut b = Builder::new(text, &p, &ty, app.scale, &app.input);
    let r = regions(app);
    let mut chrome = Vec::new();
    b.fill(Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32), 0.0, p.chrome);
    let first_run = matches!(app.overlay, Some(Overlay::FirstRun { .. }));
    if first_run {
        b.fill(Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32), 0.0, p.bg);
        chrome.push((Rect::new(0.0, 0.0, app.size.0 as f32, b.px(52.0)), Chrome::Caption));
        let top = Rect::new(0.0, 0.0, app.size.0 as f32, b.px(52.0));
        caption_buttons(app, &mut b, top, &mut chrome);
        let overlay_from = b.targets.len();
        crate::pages::first_run::view(app, &mut b);
        return finish(b, chrome, vec![], overlay_from);
    }
    sidebar(app, &mut b, r);
    // The main panel: rounded top-left, set apart from the sidebar by its fill.
    let m = r.main;
    let wide = Rect::new(m.left, m.top, m.width() + b.px(24.0), m.height() + b.px(24.0));
    b.fill(wide, b.px(12.0), p.bg);
    page_bar(app, &mut b, r.bar, &mut chrome);
    chrome.push((Rect::new(r.sidebar.left, 0.0, r.sidebar.width(), b.px(52.0)), Chrome::Caption));
    chrome.push((Rect::new(m.left, 0.0, m.width(), m.top), Chrome::Caption));
    let page_from = b.targets.len();
    b.list.ops.push(mypaper_ui::Op::Clip { rect: r.body });
    crate::pages::view(app, &mut b, r.body);
    b.list.ops.push(mypaper_ui::Op::Unclip);
    let _ = page_from;
    let regions = vec![r.sidebar, r.bar, r.body];
    let overlay_from = b.targets.len();
    match &app.overlay {
        Some(Overlay::PluginConsent) => crate::pages::plugins::consent(app, &mut b),
        Some(Overlay::PluginUninstall(id)) => crate::pages::plugins::uninstall_sheet(app, &mut b, id),
        Some(Overlay::Palette { selected }) => crate::search::view(app, &mut b, *selected),
        Some(Overlay::AddWallpaper { .. }) => crate::pages::wallpapers::add_sheet(app, &mut b),
        Some(Overlay::AddWidget) => crate::pages::widgets::add_sheet(app, &mut b),
        Some(Overlay::AddModule { bar }) => crate::pages::bars::add_sheet(app, &mut b, *bar),
        _ => {}
    }
    toast(app, &mut b);
    finish(b, chrome, regions, overlay_from)
}

fn finish(b: Builder<'_, Action>, chrome: Vec<(Rect, Chrome)>, regions: Vec<Rect>, overlay_from: usize) -> Built {
    Built {
        list: b.list,
        semantics: b.semantics,
        targets: b.targets,
        scrolls: b.scrolls,
        chrome,
        regions,
        overlay_from,
    }
}

fn sidebar(app: &App, b: &mut B, r: Regions) {
    let p = *b.p;
    let s = r.sidebar;
    let pad = b.px(12.0);
    let x = s.left + pad;
    let w = s.width() - pad * 2.0;
    // The mark and name.
    let mark = Rect::new(if r.rail { s.left + (s.width() - b.px(28.0)) / 2.0 } else { x + b.px(6.0) }, b.px(18.0), b.px(28.0), b.px(28.0));
    b.fill(mark, b.px(7.0), p.ink1);
    b.icon(mark, glyph::WALLPAPERS, 15.0, p.chrome);
    if !r.rail {
        let title = b.font(Font::Heading);
        b.text_at(mark.right + b.px(10.0), mark.top - b.px(1.0), "mypaper", &title, p.ink1);
        let micro = b.font(Font::Micro);
        b.text_at(mark.right + b.px(10.0), mark.top + b.px(15.0), "Studio", &micro, p.ink3);
    }
    // Search: opens the palette.
    let search = Rect::new(x, b.px(64.0), w, b.px(34.0));
    let k = key("sidebar-search");
    b.fill(search, b.px(8.0), p.well);
    b.wash(k, search, b.px(8.0));
    if r.rail {
        b.icon(search, glyph::SEARCH, 15.0, p.ink3);
    } else {
        b.icon(Rect::new(search.left + b.px(10.0), search.top, b.px(16.0), search.height()), glyph::SEARCH, 14.0, p.ink3);
        let body = b.font(Font::Body);
        b.text_in(Rect::new(search.left + b.px(34.0), search.top, b.px(80.0), search.height()), "Search", &body, p.ink3, Align::Start);
        let kw = b.px(30.0 + 4.0 + 20.0);
        b.keys(search.right - kw - b.px(6.0), search.top + b.px(7.0), &["Ctrl", "K"]);
    }
    b.hit(k, search, Role::SearchInput, "Search everything (Ctrl+K)", Action::Palette, mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, search, b.px(8.0));
    // The six places.
    let mut y = b.px(112.0);
    b.group(key("sidebar-nav"), |b| {
        for page in &Page::ALL[..6] {
            nav_item(app, b, Rect::new(x, y, w, b.px(36.0)), *page, r.rail);
            y += b.px(38.0);
        }
    });
    // Settings and the status block at the bottom.
    let status_h = b.px(46.0);
    let status = Rect::new(x, s.bottom - pad - status_h, w, status_h);
    let settings = Rect::new(x, status.top - b.px(10.0) - b.px(36.0), w, b.px(36.0));
    nav_item(app, b, settings, Page::Settings, r.rail);
    status_block(app, b, status, r.rail);
}

fn nav_item(app: &App, b: &mut B, rect: Rect, page: Page, rail: bool) {
    let p = *b.p;
    let k = key(("nav-page", page.key()));
    let current = app.nav.page() == page;
    let hot = b.hovered(k);
    if current {
        b.fill(rect, b.px(8.0), p.selected);
    } else {
        b.wash(k, rect, b.px(8.0));
    }
    let ink = if current || hot { p.ink1 } else { p.ink2 };
    if rail {
        b.icon(rect, page.glyph(), 16.0, ink);
    } else {
        b.icon(Rect::new(rect.left + b.px(10.0), rect.top, b.px(16.0), rect.height()), page.glyph(), 15.0, ink);
        let style = b.font(Font::Strong);
        b.text_in(Rect::new(rect.left + b.px(38.0), rect.top, rect.width() - b.px(40.0), rect.height()), page.title(), &style, ink, Align::Start);
    }
    b.hit_checked(k, rect, Role::Tab, page.title(), Some(current), Action::Nav(page), mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, rect, b.px(8.0));
}

/// Playing / Paused / mypaper is off / a part stopped, with pause or the fix.
fn status_block(app: &App, b: &mut B, rect: Rect, rail: bool) {
    let p = *b.p;
    let paused = app.get(DocId::Config, "/paused").and_then(Value::as_bool).unwrap_or(false);
    let running = app.system.running();
    let stopped_part = crate::system::PARTS
        .iter()
        .zip(app.system.parts)
        .skip(1)
        .find(|(_, part)| running && !part.running)
        .map(|((name, _), _)| *name);
    let wallpaper = app.library.active().map_or("No wallpaper", |i| app.library.items[i].name.as_str());
    let (title, detail, action, button) = if !running {
        ("mypaper is off", "Windows' own desktop", Action::StartMypaper, glyph::PLAY)
    } else if let Some(part) = stopped_part {
        (if part == "shell" { "Shell stopped" } else { "Engine stopped" }, "Restart it in Troubleshoot", Action::Restart(part), glyph::RESET)
    } else if paused {
        ("Paused", wallpaper, Action::Pause, glyph::PLAY)
    } else {
        ("Playing", wallpaper, Action::Pause, glyph::PAUSE)
    };
    b.fill(rect, b.px(10.0), if stopped_part.is_some() { p.selected } else { p.fill });
    let button_rect = Rect::new(rect.right - b.px(34.0), rect.top + (rect.height() - b.px(28.0)) / 2.0, b.px(28.0), b.px(28.0));
    if !rail {
        let mark = Rect::new(rect.left + b.px(12.0), rect.top, b.px(10.0), rect.height());
        if !running {
            b.dot(Point::new(mark.left + b.px(4.0), mark.top + mark.height() / 2.0), false);
        } else if stopped_part.is_some() {
            b.icon(mark, glyph::ALERT, 11.0, p.ink1);
        } else if paused {
            b.icon(mark, glyph::PAUSE, 10.0, p.ink1);
        } else {
            b.dot(Point::new(mark.left + b.px(4.0), mark.top + mark.height() / 2.0), true);
        }
        let strong = b.font(Font::Heading);
        let caption = b.font(Font::Caption);
        let tw = button_rect.left - rect.left - b.px(34.0);
        b.text_in(Rect::new(rect.left + b.px(30.0), rect.top + b.px(6.0), tw, b.px(16.0)), title, &strong, p.ink1, Align::Start);
        b.text_in(Rect::new(rect.left + b.px(30.0), rect.top + b.px(23.0), tw, b.px(16.0)), detail, &caption, p.ink3, Align::Start);
    }
    let button_rect = if rail { Rect::new(rect.left + (rect.width() - b.px(28.0)) / 2.0, button_rect.top, b.px(28.0), b.px(28.0)) } else { button_rect };
    let label = match action {
        Action::StartMypaper => "Start mypaper",
        Action::Restart(_) => "Restart",
        _ if paused => "Resume",
        _ => "Pause",
    };
    b.icon_button(key("status-action"), button_rect, button, label, false, true, action);
}

fn page_bar(app: &App, b: &mut B, bar: Rect, chrome: &mut Vec<(Rect, Chrome)>) {
    let p = *b.p;
    chrome.push((bar, Chrome::Caption));
    // Caption buttons, right.
    let caps = caption_buttons(app, b, bar, chrome);
    // The page's own cluster, then undo/redo/peek/saved, right to left.
    let mut right = caps - b.px(20.0);
    right = crate::pages::bar_actions(app, b, bar, right);
    let (title, detail) = crate::pages::title(app);
    let title_style = b.font(Font::Title2);
    let (tw, _) = b.measure(&title, &title_style);
    let x = bar.left + b.px(24.0);
    b.text_in(Rect::new(x, bar.top, tw + b.px(2.0), bar.height()), &title, &title_style, p.ink1, Align::Start);
    let caption = b.font(Font::Caption);
    let after = crate::pages::title_extra(app, b, Rect::new(x + tw + b.px(10.0), bar.top, right - x - tw, bar.height()));
    b.text_in(Rect::new(after, bar.top + b.px(1.0), (right - after - b.px(12.0)).max(0.0), bar.height()), &detail, &caption, p.ink3, Align::Start);
}

/// The common page-bar tools (saved state, undo, redo, Peek) right to left from `right`;
/// returns the new right edge.
pub fn edit_tools(app: &App, b: &mut B, bar: Rect, mut right: f32) -> f32 {
    let p = *b.p;
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    right -= b.px(32.0);
    b.icon_button(key("peek"), Rect::new(right, y, b.px(32.0), b.px(32.0)), glyph::EYE, "Peek at your desktop (Ctrl+D)", false, true, Action::Peek);
    right -= b.px(36.0);
    b.icon_button(key("redo"), Rect::new(right, y, b.px(32.0), b.px(32.0)), glyph::REDO, "Redo (Ctrl+Y)", false, app.docs.can_redo(), Action::Redo);
    right -= b.px(36.0);
    b.icon_button(key("undo"), Rect::new(right, y, b.px(32.0), b.px(32.0)), glyph::UNDO, "Undo (Ctrl+Z)", false, app.docs.can_undo(), Action::Undo);
    let caption = b.font(Font::Caption);
    let (label, glyph) = match &app.docs.saved {
        Saved::Done(_) => ("Saved", Some(glyph::CHECK)),
        Saved::Failed(_) => ("Not saved", Some(glyph::ALERT)),
        Saved::Idle => ("", None),
    };
    if let Some(g) = glyph {
        let (w, _) = b.measure(label, &caption);
        right -= w + b.px(28.0);
        b.icon(Rect::new(right, bar.top, b.px(14.0), bar.height()), g, 11.0, p.ink3);
        b.text_in(Rect::new(right + b.px(18.0), bar.top, w + b.px(2.0), bar.height()), label, &caption, p.ink3, Align::Start);
        // Announced as a status; not a stop for Tab (it does nothing).
        b.hit(key("save-state"), Rect::new(right, bar.top, w + b.px(20.0), bar.height()), Role::Status, label, Action::None, mypaper_ui::controls::Behavior::Click);
    }
    right - b.px(6.0)
}

/// Minimize, maximize (or restore) and close; returns their left edge.
fn caption_buttons(app: &App, b: &mut B, bar: Rect, chrome: &mut Vec<(Rect, Chrome)>) -> f32 {
    let p = *b.p;
    let (w, h) = (b.px(32.0), b.px(30.0));
    let y = bar.top + (bar.height() - h) / 2.0;
    let maximized = app.window.is_some_and(|hwnd| {
        // SAFETY: a read-only query.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd) }.as_bool()
    });
    let mut x = bar.right - b.px(8.0) - w * 3.0 - b.px(4.0);
    let left = x;
    for (g, label, action, part) in [
        (glyph::MINIMIZE, "Minimize", Action::Minimize, Chrome::Minimize),
        (if maximized { glyph::RESTORE } else { glyph::MAXIMIZE }, if maximized { "Restore" } else { "Maximize" }, Action::Maximize, Chrome::Maximize),
        (glyph::CAPTION_CLOSE, "Close", Action::Close, Chrome::Close),
    ] {
        let r = Rect::new(x, y, w, h);
        let k = key(("caption", label));
        b.wash(k, r, b.px(6.0));
        b.icon(r, g, 10.0, if b.hovered(k) { p.ink1 } else { p.ink2 });
        b.hit(k, r, Role::Button, label, action, mypaper_ui::controls::Behavior::Click);
        b.focus_ring(k, r, b.px(6.0));
        chrome.push((r, part));
        x += w + b.px(2.0);
    }
    left
}

fn toast(app: &App, b: &mut B) {
    let Some(t) = &app.toast else { return };
    let p = *b.p;
    let body = b.font(Font::Body);
    let (tw, _) = b.measure(&t.text, &body);
    let action_w = t.action.as_ref().map_or(0.0, |(l, _)| b.button_width(l, false, true) + b.px(8.0));
    let w = (tw + action_w + b.px(14.0 + 22.0 + 10.0 + 40.0)).min(app.size.0 as f32 - b.px(48.0));
    let h = b.px(42.0);
    let r = regions(app);
    let rect = Rect::new(r.main.left + (r.main.width() - w) / 2.0, app.size.1 as f32 - h - b.px(24.0), w, h);
    b.elevated(rect, b.px(10.0));
    // The toast as a status for screen readers; it also keeps clicks off the page beneath.
    // Its buttons come after, so they win the pointer where they are.
    b.hit(key("toast"), rect, Role::Status, &t.text, Action::None, mypaper_ui::controls::Behavior::Click);
    b.icon(Rect::new(rect.left + b.px(12.0), rect.top, b.px(16.0), h), if t.alert { glyph::ALERT } else { glyph::CHECK }, 12.0, p.ink1);
    b.text_in(Rect::new(rect.left + b.px(36.0), rect.top, tw + b.px(2.0), h), &t.text, &body, p.ink1, Align::Start);
    let mut x = rect.right - b.px(36.0);
    b.icon_button(key("toast-close"), Rect::new(x, rect.top + b.px(7.0), b.px(28.0), b.px(28.0)), glyph::CLOSE, "Close", false, true, Action::Dismiss);
    if let Some((label, _)) = &t.action {
        let bw = b.button_width(label, false, true);
        x -= bw + b.px(4.0);
        b.button(key("toast-action"), Rect::new(x, rect.top + b.px(7.0), bw, b.px(28.0)), label, None, ButtonKind::Secondary, true, Action::ToastButton);
    }
}

/// A sheet over the page: scrim, the centered panel with its title and close button. Returns
/// the content rect.
pub fn sheet(app: &App, b: &mut B, title: &str, width: f32, height: f32) -> Rect {
    let r = regions(app);
    let full = Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32);
    b.scrim(key("sheet-scrim"), full, Action::Dismiss);
    let w = width.min(r.main.width() - b.px(48.0));
    let h = height.min(app.size.1 as f32 - b.px(96.0));
    let rect = Rect::new(r.main.left + (r.main.width() - w) / 2.0, b.px(96.0).min((app.size.1 as f32 - h) / 2.0), w, h);
    b.elevated(rect, b.px(12.0));
    // Clicks inside the sheet don't reach the scrim.
    b.hit(key("sheet-body"), rect, Role::Dialog, title, Action::None, mypaper_ui::controls::Behavior::Click);
    let style = b.font(Font::Title1);
    b.text_in(Rect::new(rect.left + b.px(24.0), rect.top + b.px(18.0), w - b.px(80.0), b.px(28.0)), title, &style, b.p.ink1, Align::Start);
    b.icon_button(key("sheet-close"), Rect::new(rect.right - b.px(48.0), rect.top + b.px(16.0), b.px(32.0), b.px(32.0)), glyph::CLOSE, "Close", false, true, Action::Dismiss);
    Rect::new(rect.left + b.px(24.0), rect.top + b.px(60.0), w - b.px(48.0), h - b.px(60.0))
}

/// The page body split into a stage (left) and an inspector (right).
pub fn split(app: &App, body: Rect) -> (Rect, Rect) {
    let iw = inspector_width(app);
    let narrow = app.size.0 as f32 / app.scale < 960.0;
    if narrow {
        return (body, Rect::new(body.right - iw, body.top, iw, body.height()));
    }
    (
        Rect::new(body.left, body.top, body.width() - iw, body.height()),
        Rect::new(body.right - iw, body.top, iw, body.height()),
    )
}

