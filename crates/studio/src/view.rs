//! One frame of Studio: the window's chrome (sidebar, main panel, page bar, caption buttons),
//! the page, and whatever is over it (the palette, a sheet, the first run, a toast). Layout
//! follows plans/native/studio-redesign.md 8.2 and the mockups.

use mypaper_ui::controls::{Align, Builder, Font, ScrollArea, Target, key};
use mypaper_ui::{Chrome, DisplayList, Point, Rect, Role};
use serde_json::Value;

use crate::actions::Action;
use crate::app::{App, Overlay};
use crate::docs::DocId;
use crate::panel::{Command, Page};
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
    let body = Rect::new(
        main.left,
        bar.bottom,
        main.width(),
        main.bottom - bar.bottom,
    );
    Regions {
        sidebar: Rect::new(0.0, 0.0, side, h),
        main,
        bar,
        body,
        rail,
    }
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

/// A centered editor column and, on wide windows, a live desktop miniature.
pub fn content_regions(app: &App) -> (Rect, Option<Rect>) {
    let body = regions(app).body;
    let s = app.scale;
    let preview = (app.size.0 as f32 / s >= 1400.0).then(|| {
        let width = (body.width() * 0.34).clamp(320.0 * s, 480.0 * s);
        Rect::new(
            body.right - width - 24.0 * s,
            body.top + 28.0 * s,
            width,
            body.height() - 56.0 * s,
        )
    });
    let right = preview.map_or(body.right, |p| p.left - 24.0 * s);
    let width = (right - body.left - 32.0 * s).min(752.0 * s);
    let column = Rect::new(
        body.left + (right - body.left - width) / 2.0,
        body.top + 12.0 * s,
        width,
        body.height() - 12.0 * s,
    );
    (column, preview)
}

pub fn build(app: &App, ui: &mut mypaper_ui::Ui) -> Built {
    let page = crate::panel::prepare_page(app, &mut ui.text);
    let (column, preview) = content_regions(app);
    let preview_frame = preview.map(|area| {
        let aspect = app.display().map_or(9.0 / 16.0, |monitor| {
            monitor.rect.height as f32 / monitor.rect.width.max(1) as f32
        });
        crate::stage::frame(
            app,
            Rect::new(
                area.left,
                area.top + app.px(30.0),
                area.width(),
                area.width() * aspect,
            ),
        )
    });
    if let Some(frame) = preview_frame {
        crate::stage::prepare(
            app,
            ui,
            (frame.width() as u32, frame.height() as u32),
            crate::stage::Show::Desktop,
        );
    }
    let p = crate::panel::palette_for(app);
    let ty = app.theme.ty.clone();
    let mut b = Builder::new(&mut ui.text, &p, &ty, app.scale, &app.input);
    let r = regions(app);
    let mut chrome = Vec::new();
    b.fill(
        Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32),
        0.0,
        p.chrome,
    );
    sidebar(app, &mut b, r);
    let m = r.main;
    b.fill(
        Rect::new(
            m.left,
            m.top,
            m.width() + b.px(24.0),
            m.height() + b.px(24.0),
        ),
        b.px(12.0),
        p.bg,
    );
    page_bar(app, &mut b, r.bar, &mut chrome);
    chrome.push((
        Rect::new(r.sidebar.left, 0.0, r.sidebar.width(), b.px(52.0)),
        Chrome::Caption,
    ));
    chrome.push((Rect::new(m.left, 0.0, m.width(), m.top), Chrome::Caption));
    crate::panel::content(app, &mut b, column, page);
    if let (Some(area), Some(frame)) = (preview, preview_frame) {
        let heading = b.font(Font::Strong);
        b.text_at(area.left, area.top, "Your desktop", &heading, p.ink2);
        let targets = b.targets.len();
        let semantics = b.semantics.len();
        let hits = b.list.hits.len();
        crate::stage::draw(app, &mut b, frame, crate::stage::Show::Desktop, |_| {
            Action::None
        });
        b.targets.truncate(targets);
        b.semantics.truncate(semantics);
        b.list.hits.truncate(hits);
        let caption = b.font(Font::Caption);
        b.text_in(
            Rect::new(
                area.left,
                frame.bottom + b.px(16.0),
                area.width(),
                b.px(22.0),
            ),
            "Changes apply live to your desktop",
            &caption,
            p.ink3,
            Align::Start,
        );
    }
    crate::panel::pages::toast(app, &mut b, r.body);
    let overlay_from = if matches!(
        app.overlay,
        Some(Overlay::PluginConsent | Overlay::PluginUninstall(_))
    ) {
        b.targets.len()
    } else if app.overlay.is_some() {
        0
    } else {
        usize::MAX
    };
    match &app.overlay {
        Some(Overlay::PluginConsent) => crate::editing::plugins::consent(app, &mut b),
        Some(Overlay::PluginUninstall(id)) => {
            crate::editing::plugins::uninstall_sheet(app, &mut b, id)
        }
        _ => {}
    }
    crate::panel::choosers::finish(app);
    finish(b, chrome, vec![r.sidebar, r.bar, column], overlay_from)
}

fn finish(
    b: Builder<'_, Action>,
    chrome: Vec<(Rect, Chrome)>,
    regions: Vec<Rect>,
    overlay_from: usize,
) -> Built {
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
    let mark = Rect::new(
        if r.rail {
            s.left + (s.width() - b.px(28.0)) / 2.0
        } else {
            x + b.px(6.0)
        },
        b.px(18.0),
        b.px(28.0),
        b.px(28.0),
    );
    b.fill(mark, b.px(7.0), p.ink1);
    b.icon(mark, glyph::WALLPAPERS, 15.0, p.chrome);
    if !r.rail {
        let title = b.font(Font::Heading);
        b.text_at(
            mark.right + b.px(10.0),
            mark.top - b.px(1.0),
            "mypaper",
            &title,
            p.ink1,
        );
        let micro = b.font(Font::Micro);
        b.text_at(
            mark.right + b.px(10.0),
            mark.top + b.px(15.0),
            "Studio",
            &micro,
            p.ink3,
        );
    }
    // Search: opens the palette.
    let search = Rect::new(x, b.px(64.0), w, b.px(34.0));
    let k = key("sidebar-search");
    b.fill(search, b.px(8.0), p.well);
    b.wash(k, search, b.px(8.0));
    if r.rail {
        b.icon(search, glyph::SEARCH, 15.0, p.ink3);
    } else {
        b.icon(
            Rect::new(
                search.left + b.px(10.0),
                search.top,
                b.px(16.0),
                search.height(),
            ),
            glyph::SEARCH,
            14.0,
            p.ink3,
        );
        let body = b.font(Font::Body);
        b.text_in(
            Rect::new(
                search.left + b.px(34.0),
                search.top,
                b.px(80.0),
                search.height(),
            ),
            "Search",
            &body,
            p.ink3,
            Align::Start,
        );
        let kw = b.px(30.0 + 4.0 + 20.0);
        b.keys(
            search.right - kw - b.px(6.0),
            search.top + b.px(7.0),
            &["Ctrl", "K"],
        );
    }
    b.hit(
        k,
        search,
        Role::SearchInput,
        "Search everything (Ctrl+K)",
        Action::Panel(Command::Open(Page::Search)),
        mypaper_ui::controls::Behavior::Click,
    );
    b.focus_ring(k, search, b.px(8.0));
    // The six places.
    let mut y = b.px(112.0);
    b.group(key("sidebar-nav"), |b| {
        for page in [
            Page::Root,
            Page::Wallpapers,
            Page::Look,
            Page::Bars,
            Page::Widgets,
            Page::DesktopMenu,
            Page::Windows,
            Page::Connections,
        ] {
            nav_item(app, b, Rect::new(x, y, w, b.px(36.0)), page, r.rail);
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
    let k = key(("nav-page", page));
    let mut selected = app.panel.as_ref().unwrap().page;
    while ![
        Page::Root,
        Page::Wallpapers,
        Page::Look,
        Page::Bars,
        Page::Widgets,
        Page::DesktopMenu,
        Page::Windows,
        Page::Connections,
        Page::Settings,
    ]
    .contains(&selected)
    {
        selected = selected.parent().unwrap_or(Page::Root);
    }
    let current = selected == page;
    let label = window_title(page);
    let hot = b.hovered(k);
    if current {
        b.fill(rect, b.px(8.0), p.selected);
    } else {
        b.wash(k, rect, b.px(8.0));
    }
    let ink = if current || hot { p.ink1 } else { p.ink2 };
    if rail {
        b.icon(rect, page_glyph(page), 16.0, ink);
    } else {
        b.icon(
            Rect::new(rect.left + b.px(10.0), rect.top, b.px(16.0), rect.height()),
            page_glyph(page),
            15.0,
            ink,
        );
        let style = b.font(Font::Strong);
        b.text_in(
            Rect::new(
                rect.left + b.px(38.0),
                rect.top,
                rect.width() - b.px(40.0),
                rect.height(),
            ),
            label,
            &style,
            ink,
            Align::Start,
        );
    }
    b.hit_checked(
        k,
        rect,
        Role::Tab,
        label,
        Some(current),
        Action::Panel(Command::Open(page)),
        mypaper_ui::controls::Behavior::Click,
    );
    b.focus_ring(k, rect, b.px(8.0));
}

/// Playing / Paused / mypaper is off / a part stopped, with pause or the fix.
fn status_block(app: &App, b: &mut B, rect: Rect, rail: bool) {
    let p = *b.p;
    let paused = app
        .get(DocId::Config, "/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let running = app.system.running();
    let stopped_part = crate::system::PARTS
        .iter()
        .zip(app.system.parts)
        .skip(1)
        .find(|(_, part)| running && !part.running)
        .map(|((name, _), _)| *name);
    let wallpaper = app
        .library
        .active()
        .map_or("No wallpaper", |i| app.library.items[i].name.as_str());
    let (title, detail, action, button) = if !running {
        (
            "mypaper is off",
            "Windows' own desktop",
            Action::StartMypaper,
            glyph::PLAY,
        )
    } else if let Some(part) = stopped_part {
        (
            if part == "shell" {
                "Shell stopped"
            } else {
                "Engine stopped"
            },
            "Restart it in Troubleshoot",
            Action::Restart(part),
            glyph::RESET,
        )
    } else if paused {
        ("Paused", wallpaper, Action::Pause, glyph::PLAY)
    } else {
        ("Playing", wallpaper, Action::Pause, glyph::PAUSE)
    };
    b.fill(
        rect,
        b.px(10.0),
        if stopped_part.is_some() {
            p.selected
        } else {
            p.fill
        },
    );
    let button_rect = Rect::new(
        rect.right - b.px(34.0),
        rect.top + (rect.height() - b.px(28.0)) / 2.0,
        b.px(28.0),
        b.px(28.0),
    );
    if !rail {
        let mark = Rect::new(rect.left + b.px(12.0), rect.top, b.px(10.0), rect.height());
        if !running {
            b.dot(
                Point::new(mark.left + b.px(4.0), mark.top + mark.height() / 2.0),
                false,
            );
        } else if stopped_part.is_some() {
            b.icon(mark, glyph::ALERT, 11.0, p.ink1);
        } else if paused {
            b.icon(mark, glyph::PAUSE, 10.0, p.ink1);
        } else {
            b.dot(
                Point::new(mark.left + b.px(4.0), mark.top + mark.height() / 2.0),
                true,
            );
        }
        let strong = b.font(Font::Heading);
        let caption = b.font(Font::Caption);
        let tw = button_rect.left - rect.left - b.px(34.0);
        b.text_in(
            Rect::new(rect.left + b.px(30.0), rect.top + b.px(6.0), tw, b.px(16.0)),
            title,
            &strong,
            p.ink1,
            Align::Start,
        );
        b.text_in(
            Rect::new(
                rect.left + b.px(30.0),
                rect.top + b.px(23.0),
                tw,
                b.px(16.0),
            ),
            detail,
            &caption,
            p.ink3,
            Align::Start,
        );
    }
    let button_rect = if rail {
        Rect::new(
            rect.left + (rect.width() - b.px(28.0)) / 2.0,
            button_rect.top,
            b.px(28.0),
            b.px(28.0),
        )
    } else {
        button_rect
    };
    let label = match action {
        Action::StartMypaper => "Start mypaper",
        Action::Restart(_) => "Restart",
        _ if paused => "Resume",
        _ => "Pause",
    };
    b.icon_button(
        key("status-action"),
        button_rect,
        button,
        label,
        false,
        true,
        action,
    );
}

fn window_title(page: Page) -> &'static str {
    match page {
        Page::Root => "Home",
        Page::Wallpapers => "Wallpapers",
        _ => page.title(),
    }
}

fn page_glyph(page: Page) -> mypaper_ui::controls::Glyph {
    match page {
        Page::Root => glyph::HOME,
        Page::Wallpapers => glyph::WALLPAPERS,
        Page::Look | Page::Windows => glyph::LOOK,
        Page::Bars => glyph::BARS,
        Page::Widgets => glyph::WIDGETS,
        Page::DesktopMenu => glyph::SETTINGS,
        Page::Connections => glyph::INTEGRATIONS,
        _ => glyph::SETTINGS,
    }
}

fn page_bar(app: &App, b: &mut B, bar: Rect, chrome: &mut Vec<(Rect, Chrome)>) {
    let p = *b.p;
    chrome.push((bar, Chrome::Caption));
    let caps = caption_buttons(app, b, bar, chrome);
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let mut right = caps - b.px(16.0);
    for (id, icon, label, enabled, action) in [
        (
            "window-search",
            glyph::SEARCH,
            "Search (Ctrl+K)",
            true,
            Action::Panel(Command::Open(Page::Search)),
        ),
        (
            "window-history",
            glyph::CLOCK,
            "History (Ctrl+H)",
            true,
            Action::Panel(Command::Open(Page::History)),
        ),
        (
            "redo",
            glyph::REDO,
            "Redo (Ctrl+Y)",
            app.docs.can_redo(),
            Action::Redo,
        ),
        (
            "undo",
            glyph::UNDO,
            "Undo (Ctrl+Z)",
            app.docs.can_undo(),
            Action::Undo,
        ),
    ] {
        right -= b.px(36.0);
        b.icon_button(
            key(id),
            Rect::new(right, y, b.px(32.0), b.px(32.0)),
            icon,
            label,
            false,
            enabled,
            action,
        );
    }
    let page = app.panel.as_ref().unwrap().page;
    let status = crate::panel::save_status(app, page);
    let caption = b.font(Font::Caption);
    let sw = b.measure(status, &caption).0 + b.px(16.0);
    right -= sw;
    b.text_in(
        Rect::new(right, bar.top, sw, bar.height()),
        status,
        &caption,
        p.ink3,
        Align::Start,
    );
    let mut x = bar.left + b.px(24.0);
    if ![
        Page::Root,
        Page::Wallpapers,
        Page::Look,
        Page::Bars,
        Page::Widgets,
        Page::DesktopMenu,
        Page::Windows,
        Page::Connections,
        Page::Settings,
    ]
    .contains(&page)
    {
        b.icon_button(
            key("window-back"),
            Rect::new(x - b.px(8.0), y, b.px(32.0), b.px(32.0)),
            glyph::CHEVRON_LEFT,
            "Back",
            false,
            true,
            Action::Panel(Command::Back),
        );
        x += b.px(32.0);
    }
    let style = b.font(Font::Title2);
    b.text_in(
        Rect::new(x, bar.top, (right - x - b.px(12.0)).max(0.0), bar.height()),
        window_title(page),
        &style,
        p.ink1,
        Align::Start,
    );
    b.announce(status, app.docs.version);
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
        (
            glyph::MINIMIZE,
            "Minimize",
            Action::Minimize,
            Chrome::Minimize,
        ),
        (
            if maximized {
                glyph::RESTORE
            } else {
                glyph::MAXIMIZE
            },
            if maximized { "Restore" } else { "Maximize" },
            Action::Maximize,
            Chrome::Maximize,
        ),
        (glyph::CAPTION_CLOSE, "Close", Action::Close, Chrome::Close),
    ] {
        let r = Rect::new(x, y, w, h);
        let k = key(("caption", label));
        b.wash(k, r, b.px(6.0));
        b.icon(r, g, 10.0, if b.hovered(k) { p.ink1 } else { p.ink2 });
        b.hit(
            k,
            r,
            Role::Button,
            label,
            action,
            mypaper_ui::controls::Behavior::Click,
        );
        b.focus_ring(k, r, b.px(6.0));
        chrome.push((r, part));
        x += w + b.px(2.0);
    }
    left
}

/// A sheet over the page: scrim, the centered panel with its title and close button. Returns
/// the content rect.
pub fn sheet(app: &App, b: &mut B, title: &str, width: f32, height: f32) -> Rect {
    let r = regions(app);
    let full = Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32);
    b.scrim(key("sheet-scrim"), full, Action::Dismiss);
    let w = width.min(r.main.width() - b.px(48.0));
    let h = height.min(app.size.1 as f32 - b.px(96.0));
    let rect = Rect::new(
        r.main.left + (r.main.width() - w) / 2.0,
        b.px(96.0).min((app.size.1 as f32 - h) / 2.0),
        w,
        h,
    );
    b.elevated(rect, b.px(12.0));
    // Clicks inside the sheet don't reach the scrim.
    b.hit(
        key("sheet-body"),
        rect,
        Role::Dialog,
        title,
        Action::None,
        mypaper_ui::controls::Behavior::Click,
    );
    let style = b.font(Font::Title1);
    b.text_in(
        Rect::new(
            rect.left + b.px(24.0),
            rect.top + b.px(18.0),
            w - b.px(80.0),
            b.px(28.0),
        ),
        title,
        &style,
        b.p.ink1,
        Align::Start,
    );
    b.icon_button(
        key("sheet-close"),
        Rect::new(
            rect.right - b.px(48.0),
            rect.top + b.px(16.0),
            b.px(32.0),
            b.px(32.0),
        ),
        glyph::CLOSE,
        "Close",
        false,
        true,
        Action::Dismiss,
    );
    Rect::new(
        rect.left + b.px(24.0),
        rect.top + b.px(60.0),
        w - b.px(48.0),
        h - b.px(60.0),
    )
}
