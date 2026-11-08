//! The standalone Studio frame: sidebar, fixed band, interactive stage and docked inspector.
//! All editors are the shared panel pages. Navigation and stage zoom never edit documents.

use crate::panel::tokens::radius;
use mypaper_ui::controls::{Align, Behavior, Font, Glyph, ScrollArea, Target, key};
use mypaper_ui::{Chrome, DisplayList, Point, Rect, Role};
use serde_json::Value;

use crate::actions::{Action, Bar};
use crate::app::{App, Overlay};
use crate::docs::DocId;
use crate::panel::{Command, Page};
use crate::stage::{Item, Show, What};
use crate::theme::glyph;
use crate::ui::B;

// Placement replaces /window on close; frame preferences also survive older placement writers.
const SIDEBAR_SETTING: &str = "/frame/sidebar_collapsed";
const SETTINGS_SECTION: &str = "/frame/settings_collapsed";
pub const INSPECTOR_SETTING: &str = "/frame/inspector_width";
const COLLAPSE_TIME: std::time::Duration = std::time::Duration::from_millis(160);

#[derive(Debug, Default)]
pub struct State {
    sidebar: Option<Transition>,
    pub inspector_drag: Option<f32>,
}

#[derive(Debug)]
struct Transition {
    start: std::time::Instant,
    from: f32,
    to: f32,
}

fn collapsed(app: &App) -> bool {
    app.get(DocId::Studio, SIDEBAR_SETTING)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn sidebar_width(app: &App) -> f32 {
    if (app.size.0 as f32) < app.px(1100.0) {
        return 56.0;
    }
    if let Some(animation) = app.panel.as_ref().and_then(|s| s.window.sidebar.as_ref()) {
        let t = (animation.start.elapsed().as_secs_f32() / COLLAPSE_TIME.as_secs_f32()).min(1.0);
        return animation.from + (animation.to - animation.from) * (1.0 - (1.0 - t).powi(3));
    }
    if collapsed(app) { 56.0 } else { 208.0 }
}

pub fn toggle_sidebar(app: &mut App) {
    let from = sidebar_width(app);
    let collapse = !collapsed(app);
    app.apply(
        "Sidebar",
        vec![crate::docs::Op {
            doc: DocId::Studio,
            path: SIDEBAR_SETTING.into(),
            value: Some(serde_json::json!(collapse)),
        }],
    );
    if collapsed(app) != collapse {
        return;
    }
    let animate = !app.fixture
        && app
            .get(DocId::Bars, "/animation/reduceMotion")
            .and_then(Value::as_str)
            != Some("always");
    app.panel.as_mut().unwrap().window.sidebar = animate.then(|| Transition {
        start: std::time::Instant::now(),
        from,
        to: if collapse { 56.0 } else { 208.0 },
    });
    app.dirty = true;
}

pub fn animation_deadline(app: &App) -> Option<std::time::Instant> {
    let animation = app.panel.as_ref()?.window.sidebar.as_ref()?;
    Some(
        (std::time::Instant::now() + std::time::Duration::from_millis(16))
            .min(animation.start + COLLAPSE_TIME),
    )
}

pub fn tick(app: &mut App) {
    if let Some(state) = app.panel.as_mut()
        && let Some(animation) = &state.window.sidebar
    {
        if animation.start.elapsed() >= COLLAPSE_TIME {
            state.window.sidebar = None;
        }
        app.dirty = true;
    }
}

pub fn toggle_settings(app: &mut App) {
    let collapse = app
        .get(DocId::Studio, SETTINGS_SECTION)
        .and_then(Value::as_bool)
        != Some(true);
    app.apply(
        "Settings section",
        vec![crate::docs::Op {
            doc: DocId::Studio,
            path: SETTINGS_SECTION.into(),
            value: Some(serde_json::json!(collapse)),
        }],
    );
}

#[derive(Debug, Clone, Copy)]
pub struct Regions {
    pub sidebar: Rect,
    pub main: Rect,
    pub bar: Rect,
    pub body: Rect,
    pub stage: Option<Rect>,
    pub inspector: Option<Rect>,
    pub rail: bool,
}

pub fn has_stage(page: Page) -> bool {
    !matches!(
        page,
        Page::Settings
            | Page::General
            | Page::Performance
            | Page::Keyboard
            | Page::Troubleshoot
            | Page::About
            | Page::Connections
            | Page::Connection
            | Page::History
            | Page::Search
            | Page::FirstRun
    )
}

pub fn inspector_width(app: &App) -> f32 {
    // Give larger displays a useful editor without taking over the desktop preview.
    let default = (app.size.0 as f32 / app.scale * 0.30).clamp(340.0, 640.0);
    let width = app.panel.as_ref().and_then(|s| s.window.inspector_drag)
        .or_else(|| app.get(DocId::Studio, INSPECTOR_SETTING).and_then(Value::as_f64).map(|v| v as f32))
        .filter(|width| width.is_finite()).unwrap_or(default);
    let max = (app.size.0 as f32 / app.scale - sidebar_width(app) - 360.0).clamp(300.0, 720.0);
    app.px(width.clamp(300.0, max))
}

/// During frame motion reuse one stage raster; shell views and glass are rebuilt on settle.
pub fn resizing(app: &App) -> bool {
    app.panel.as_ref().is_some_and(|s| s.window.sidebar.is_some() || s.window.inspector_drag.is_some())
}

fn stage_show(app: &App) -> Show {
    match crate::panel::current_page(app) {
        Page::Bars | Page::Top | Page::Dock | Page::Module | Page::BarDesign | Page::AddModule => Show::Bars,
        Page::Widgets | Page::Widget | Page::AddWidget => Show::Widgets,
        _ => Show::Desktop,
    }
}

pub fn regions(app: &App) -> Regions {
    let (w, h) = (app.size.0 as f32, app.size.1 as f32);
    let side = app.px(sidebar_width(app));
    let rail = side < app.px(180.0);
    let main = Rect::new(
        side,
        app.px(8.0),
        w - side - app.px(8.0),
        h - app.px(8.0),
    );
    let bar = Rect::new(side, main.top, main.width(), app.px(48.0));
    let body = Rect::new(side, bar.bottom, main.width(), h - bar.bottom);
    let staged = has_stage(crate::panel::current_page(app));
    let inspector = staged.then(|| {
        Rect::new(
            main.right - inspector_width(app),
            bar.bottom,
            inspector_width(app),
            body.height(),
        )
    });
    let stage = inspector.map(|inspector| {
        Rect::new(
            side + app.px(16.0),
            body.top + app.px(16.0),
            inspector.left - side - app.px(32.0),
            body.height() - app.px(32.0),
        )
    });
    Regions {
        sidebar: Rect::new(0.0, 0.0, side, h),
        main,
        bar,
        body,
        stage,
        inspector,
        rail,
    }
}

/// The page host: a responsive inspector, or a centered reading area for settings.
pub fn content_regions(app: &App) -> (Rect, Option<Rect>) {
    let r = regions(app);
    let content = r.inspector.unwrap_or_else(|| {
        let width = (r.body.width() - app.px(32.0)).min(app.px(1200.0));
        Rect::new(
            r.body.left + (r.body.width() - width) / 2.0,
            r.body.top + app.px(16.0),
            width,
            r.body.height() - app.px(16.0),
        )
    });
    (content, r.stage)
}

pub struct Built {
    pub list: DisplayList,
    pub semantics: Vec<(mypaper_ui::HitId, mypaper_ui::Semantics)>,
    pub targets: Vec<Target<Action>>,
    pub scrolls: Vec<ScrollArea>,
    pub chrome: Vec<(Rect, Chrome)>,
    pub regions: Vec<Rect>,
    pub overlay_from: usize,
}

/// Prepare at the display's scale for 100%, or at the fitted picture's scale for Fit.
fn prepare_stage(app: &App, ui: &mut mypaper_ui::Ui, viewport: Rect) -> Rect {
    let actual = app.panel.as_ref().unwrap().stage_actual;
    let show = stage_show(app);
    let mut frame = if show == Show::Bars && !actual {
        let height = app.px(280.0).min(viewport.height() - app.px(48.0));
        Rect::new(viewport.left, viewport.top + (viewport.height() - height - app.px(48.0)) / 2.0, viewport.width(), height)
    } else if actual {
        let (w, h) = app.display().map_or((1280.0, 720.0), |m| {
            let dpi = m.dpi as f32 / 96.0;
            (m.rect.width as f32 / dpi, m.rect.height as f32 / dpi)
        });
        Rect::new(viewport.left, viewport.top, app.px(w), app.px(h))
    } else {
        // Reserve the chips beneath the desktop and center the whole composition.
        let available = Rect::new(
            viewport.left,
            viewport.top,
            viewport.width(),
            viewport.height() - app.px(48.0),
        );
        crate::stage::frame(app, available)
    };
    if resizing(app) && app.stage.borrow().content.size.0 > 0 {
        if app.stage.borrow().picture.is_none() {
            let size = app.stage.borrow().content.size;
            let p = crate::panel::palette_for(app);
            let mut b = crate::ui::builder(&mut ui.text, &p, &app.theme.ty, app.scale, &app.input);
            crate::stage::draw(app, &mut b, Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32), show, |_| Action::None);
            let list = b.list;
            if let Ok(image) = mypaper_ui::render(ui, &list, size.0, size.1) {
                app.stage.borrow_mut().picture = Some(std::sync::Arc::new(image));
            }
        }
        return frame;
    }
    app.stage.borrow_mut().picture = None;
    crate::stage::prepare(
        app,
        ui,
        (frame.width() as u32, frame.height() as u32),
        show,
    );
    if actual {
        let cache = app.stage.borrow();
        let center = cache
            .content
            .items
            .iter()
            .find(|item| selected(app, &item.what))
            .map_or(
                Point::new(frame.width() / 2.0, frame.height() / 2.0),
                |item| {
                    Point::new(
                        (item.rect.left + item.rect.right) / 2.0,
                        (item.rect.top + item.rect.bottom) / 2.0,
                    )
                },
            );
        frame = Rect::new(
            (viewport.left + viewport.width() / 2.0 - center.x).clamp(
                (viewport.right - frame.width()).min(viewport.left),
                viewport.left,
            ),
            (viewport.top + viewport.height() / 2.0 - center.y).clamp(
                (viewport.bottom - frame.height()).min(viewport.top),
                viewport.top,
            ),
            frame.width(),
            frame.height(),
        );
    }
    frame
}

/// Display chips stay with the desktop they switch, below its fitted picture.
fn stage_controls(app: &App, b: &mut B, picture: Rect, viewport: Rect) {
    let order = crate::panel::controls::display_order(app);
    let labels: Vec<_> = order
        .iter()
        .map(|&i| crate::panel::controls::display_name(app, i))
        .collect();
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let width = b.segmented_width(&labels).min(viewport.width());
    let rect = Rect::new(
        viewport.left + (viewport.width() - width) / 2.0,
        (picture.bottom + b.px(16.0)).min(viewport.bottom - b.px(32.0)),
        width,
        b.px(32.0),
    );
    b.segmented(
        key("stage-displays"),
        rect,
        &labels,
        order.iter().position(|&i| i == app.nav.display),
        "Stage display",
        true,
        |i| Action::Panel(Command::Display(order[i])),
    );
}

fn selected(app: &App, what: &What) -> bool {
    let page = crate::panel::current_page(app);
    match what {
        What::Widget(id) => (page == Page::Widget || matches!(&app.drag, Some(crate::app::Drag::Widget { instance, .. }) if instance == id)) && app.nav.widget.as_ref() == Some(id),
        What::Module(slot) => page == Page::Module && app.nav.module.as_ref() == Some(slot),
        What::Bar(bar) => {
            matches!(page, Page::Top | Page::Dock | Page::BarDesign) && app.nav.bar == Some(*bar)
        }
    }
}

fn stage_pick(item: Option<&Item>) -> Action {
    match item.map(|item| &item.what) {
        None => Action::Panel(Command::Open(Page::Wallpapers)),
        Some(What::Widget(id)) => Action::SelectWidget(Some(id.clone())),
        Some(What::Module(slot)) => Action::SelectModule(Some(slot.clone())),
        Some(What::Bar(bar)) => Action::Panel(Command::Open(if *bar == Bar::Top {
            Page::Top
        } else {
            Page::Dock
        })),
    }
}

pub fn build(app: &App, ui: &mut mypaper_ui::Ui) -> Built {
    let page = crate::panel::prepare_page(app, &mut ui.text);
    let r = regions(app);
    let (host, _) = content_regions(app);
    let picture = r.stage.map(|viewport| prepare_stage(app, ui, viewport));
    let p = crate::panel::palette_for(app);
    let ty = app.theme.ty.clone();
    let mut b = crate::ui::builder(&mut ui.text, &p, &ty, app.scale, &app.input);
    b.fill(
        Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32),
        0.0,
        p.chrome,
    );
    b.fill(r.sidebar, 0.0, p.chrome);
    b.fill(r.main, app.px(radius::CARD), p.bg);
    // Only the top edge is rounded; the sheet continues to the window's bottom.
    b.fill(
        Rect::new(
            r.main.left,
            r.main.top + app.px(radius::CARD),
            r.main.width(),
            r.main.height() - app.px(radius::CARD),
        ),
        0.0,
        p.bg,
    );
    sidebar(app, &mut b, r);
    let mut chrome = vec![
        (r.bar, Chrome::Caption),
        (
            Rect::new(0.0, 0.0, r.sidebar.width(), app.px(48.0)),
            Chrome::Caption,
        ),
    ];
    page_bar(app, &mut b, r, &mut chrome);
    let mut focus_regions = vec![r.sidebar, r.bar];
    if let (Some(viewport), Some(picture)) = (r.stage, picture) {
        b.scroll(key("window-stage"), viewport, 0.0, Action::None, |b, _| {
            if crate::builder::stage(app, b, viewport) { return viewport.bottom; }
            if page == Page::Windows {
                crate::panel::specimens::window_stage(app, b, viewport);
                b.hit(
                    key("window-look-preview"),
                    viewport,
                    Role::Image,
                    "Windows look preview",
                    Action::None,
                    Behavior::Click,
                );
                return viewport.bottom;
            }
            if page == Page::DesktopMenu {
                crate::panel::specimens::backdrop(app, b, viewport);
                stage_menu(app, b, viewport);
                return viewport.bottom;
            }
            let items = crate::stage::draw(app, b, picture, stage_show(app), stage_pick);
            for item in items.iter().filter(|item| selected(app, &item.what)) {
                crate::stage::outline(b, item.rect, item.radius, false);
            }
            if let Some(item) = items.iter().find(|item| selected(app, &item.what)) {
                stage_tag(b, item, viewport);
            }
            if !app.system.running() {
                off_card(b, picture, viewport);
            }
            stage_controls(app, b, picture, viewport);
            viewport.bottom
        });
        focus_regions.push(viewport);
    }
    if let Some(inspector) = r.inspector {
        b.fill(inspector, 0.0, p.bg);
    }
    crate::panel::content(app, &mut b, host, page);
    if let Some(inspector) = r.inspector {
        let handle = Rect::new(inspector.left - b.px(4.0), inspector.top, b.px(8.0), inspector.height());
        let k = key("inspector-resize");
        if b.hovered(k) || app.panel.as_ref().unwrap().window.inspector_drag.is_some() {
            b.fill(Rect::new(inspector.left, inspector.top + b.px(16.0), b.px(1.0), inspector.height() - b.px(32.0)), 0.0, p.edge);
        }
        b.hit(k, handle, Role::Button, "Resize inspector", Action::Panel(Command::ResizeInspector), Behavior::Drag);
    }
    focus_regions.push(host);
    crate::panel::pages::toast(
        app,
        &mut b,
        if r.inspector.is_some() { host } else { r.body },
    );
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
    if r.rail && app.overlay.is_none() { sidebar_tooltip(app, &mut b, r); }
    crate::panel::choosers::finish(app);
    Built {
        list: b.list,
        semantics: b.semantics,
        targets: b.targets,
        scrolls: b.scrolls,
        chrome,
        regions: focus_regions,
        overlay_from,
    }
}

const PLACES: [Page; 6] = [
    Page::Root,
    Page::Wallpapers,
    Page::Look,
    Page::Widgets,
    Page::Bars,
    Page::Connections,
];
const SETTINGS: [Page; 6] = [
    Page::General,
    Page::Performance,
    Page::DesktopMenu,
    Page::Keyboard,
    Page::Troubleshoot,
    Page::About,
];

fn window_title(page: Page) -> &'static str {
    match page {
        Page::Root => "Your desktop",
        Page::Wallpapers => "Wallpapers",
        _ => page.title(),
    }
}

fn page_glyph(page: Page) -> Glyph {
    match page {
        Page::Root => glyph::MONITOR,
        Page::Wallpapers => glyph::WALLPAPERS,
        Page::Look | Page::Windows => glyph::LOOK,
        Page::Widgets => glyph::WIDGETS,
        Page::Bars => glyph::BARS,
        Page::Connections => glyph::INTEGRATIONS,
        Page::Performance => glyph::GAUGE,
        Page::Keyboard => glyph::KEYBOARD,
        Page::Troubleshoot => glyph::WRENCH,
        Page::About => glyph::INFO,
        _ => glyph::SETTINGS,
    }
}

fn sidebar(app: &App, b: &mut B, r: Regions) {
    let p = *b.p;
    let pad = b.px(if r.rail { 8.0 } else { 12.0 });
    let x = pad;
    let width = r.sidebar.width() - 2.0 * pad;
    let mark = Rect::new(
        x + b.px(if r.rail { 10.0 } else { 4.0 }),
        b.px(14.0),
        b.px(20.0),
        b.px(20.0),
    );
    b.fill(mark, b.px(radius::SMALL), p.solid);
    b.icon(mark, glyph::WALLPAPERS, 12.0, p.on_solid);
    if !r.rail {
        let font = b.font(Font::Strong);
        b.text_at(mark.right + b.px(8.0), b.px(14.0), "Studio", &font, p.ink1);
    }
    let search = Rect::new(x, b.px(52.0), width, b.px(32.0));
    b.fill(search, b.control_radius(search), p.well);
    b.icon(
        Rect::new(x + b.px(8.0), search.top, b.px(20.0), search.height()),
        glyph::SEARCH,
        15.0,
        p.ink3,
    );
    if !r.rail {
        let font = b.font(Font::Body);
        b.text_in(
            Rect::new(
                x + b.px(34.0),
                search.top,
                width - b.px(96.0),
                search.height(),
            ),
            "Search",
            &font,
            p.ink3,
            Align::Start,
        );
        b.keys(
            search.right - b.px(58.0),
            search.top + b.px(6.0),
            &["Ctrl", "K"],
        );
    }
    b.hit(
        key("sidebar-search"),
        search,
        Role::Button,
        "Search everything (Ctrl+K)",
        Action::Panel(Command::Open(Page::Search)),
        Behavior::Click,
    );
    b.focus_ring(key("sidebar-search"), search, b.control_radius(search));
    let status_height = b.px(if r.rail { 46.0 } else { 34.0 * b.t.text_scale + 12.0 });
    let status = Rect::new(x, r.sidebar.bottom - status_height - b.px(12.0), width, status_height);
    let nav = Rect::new(x, b.px(96.0), width, status.top - b.px(108.0));
    let nav_key = key("window-sidebar-links");
    b.scroll(
        nav_key,
        nav,
        app.nav.scroll(nav_key),
        Action::None,
        |b, top| {
            let row_height = b.px(32.0 * if r.rail { 1.0 } else { b.t.text_scale });
            let step = row_height + b.px(2.0);
            let mut y = top;
            for page in PLACES {
                nav_item(app, b, Rect::new(x, y, width, row_height), page, r.rail);
                y += step;
            }
            let settings_collapsed = app.get(DocId::Studio, SETTINGS_SECTION).and_then(Value::as_bool) == Some(true);
            if r.rail {
                y += b.px(12.0);
            } else {
                let header = Rect::new(x, y + b.px(6.0), width, b.px(30.0 * b.t.text_scale));
                let k = key("sidebar-settings");
                if settings_collapsed && SETTINGS.contains(&nav_place(app)) {
                    b.fill(header, b.px(radius::CONTROL), p.selected);
                } else {
                    b.wash(k, header, b.px(radius::CONTROL));
                }
                let font = b.font(Font::Caption);
                let ink = if b.hovered(k) { p.ink1 } else { p.ink3 };
                b.text_in(Rect::new(x + b.px(10.0), header.top, width - b.px(36.0), header.height()), "Settings", &font, ink, Align::Start);
                b.icon(Rect::new(header.right - b.px(26.0), header.top, b.px(16.0), header.height()), if settings_collapsed { glyph::CHEVRON_RIGHT } else { glyph::CHEVRON_DOWN }, 13.0, ink);
                b.hit_checked(k, header, Role::Button, "Settings section", Some(!settings_collapsed), Action::Panel(Command::SidebarSettings), Behavior::Click);
                b.focus_ring(k, header, b.px(radius::CONTROL));
                y = header.bottom;
            }
            if r.rail || !settings_collapsed {
                for page in SETTINGS {
                    nav_item(app, b, Rect::new(x, y, width, row_height), page, r.rail);
                    y += step;
                }
            }
            y
        },
    );
    status_block(app, b, status, r.rail);
}

fn nav_place(app: &App) -> Page {
    let mut active = crate::panel::current_page(app);
    if active == Page::Windows {
        active = Page::Look;
    }
    if active == Page::Settings {
        active = Page::General;
    }
    while !PLACES.contains(&active) && !SETTINGS.contains(&active) {
        active = active.parent().unwrap_or(Page::Root);
    }
    active
}

fn nav_item(app: &App, b: &mut B, rect: Rect, page: Page, rail: bool) {
    let p = *b.p;
    let current = nav_place(app) == page;
    let k = key(("nav-page", page));
    if current {
        b.fill(rect, b.control_radius(rect), p.selected);
    } else {
        b.wash(k, rect, b.control_radius(rect));
    }
    let ink = if current || b.hovered(k) {
        p.ink1
    } else {
        p.ink2
    };
    let icon_ink = if current || b.hovered(k) { p.ink1 } else { p.ink3 };
    if rail {
        b.icon(rect, page_glyph(page), 16.0, icon_ink);
    } else {
        b.icon(
            Rect::new(rect.left + b.px(10.0), rect.top, b.px(16.0), rect.height()),
            page_glyph(page),
            15.0,
            icon_ink,
        );
        let font = crate::panel::controls::font(b, 13.0, if current { 600 } else { 500 });
        b.text_in(
            Rect::new(
                rect.left + b.px(36.0),
                rect.top,
                rect.width() - b.px(38.0),
                rect.height(),
            ),
            window_title(page),
            &font,
            ink,
            Align::Start,
        );
    }
    b.hit_checked(
        k,
        rect,
        Role::Tab,
        window_title(page),
        Some(current),
        Action::Panel(Command::Open(page)),
        Behavior::Click,
    );
    b.focus_ring(k, rect, b.control_radius(rect));
}

fn sidebar_tooltip(app: &App, b: &mut B, r: Regions) {
    let Some((index, target)) = app
        .input
        .hover
        .and_then(|k| b.targets.iter().enumerate().find(|(_, t)| t.key == k))
    else {
        return;
    };
    if target.rect.right > r.sidebar.right || target.rect.top < b.px(48.0) {
        return;
    }
    let Some(label) = b
        .list
        .hits
        .iter()
        .find(|h| h.id == index as u32 + 1)
        .map(|h| h.label.to_string())
    else {
        return;
    };
    let top = target.rect.top;
    let font = b.font(Font::Caption);
    let width = b.measure(&label, &font).0 + b.px(20.0);
    let rect = Rect::new(r.sidebar.right + b.px(8.0), top, width, b.px(30.0));
    b.fill(rect, b.px(radius::CONTROL), b.p.fill);
    b.stroke(rect, b.px(radius::CONTROL), b.px(1.0), b.p.line);
    b.text_in(rect, &label, &font, b.p.ink1, Align::Center);
}

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
        b.px(radius::MEDIUM),
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
            Rect::new(rect.left + b.px(30.0), rect.top + b.px(6.0), tw, b.px(18.0 * b.t.text_scale)),
            title,
            &strong,
            p.ink1,
            Align::Start,
        );
        b.text_in(
            Rect::new(
                rect.left + b.px(30.0),
                rect.top + b.px(6.0 + 18.0 * b.t.text_scale),
                tw,
                b.px(16.0 * b.t.text_scale),
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

fn parent(app: &App, page: Page) -> Option<Page> {
    match page {
        Page::Windows => Some(Page::Look),
        Page::Module | Page::AddModule | Page::BarDesign => Some(match app.nav.bar {
            Some(Bar::Dock) => Page::Dock,
            Some(Bar::Top) => Page::Top,
            None => Page::Bars,
        }),
        _ => page.parent(),
    }
}

fn page_title(app: &App, page: Page) -> String {
    if page == Page::Widget
        && let Some(instance) =
            crate::editing::widgets::layout(app).get(app.nav.widget.as_deref().unwrap_or(""))
    {
        return app
            .registry
            .get(&instance.widget)
            .map_or(instance.widget.clone(), |w| w.manifest.name.clone());
    }
    if page == Page::Wallpaper
        && let Some(wallpaper) = app.nav.wallpaper.and_then(|i| app.library.items.get(i))
    {
        return wallpaper.name.clone();
    }
    if page == Page::Module
        && let Some(name) = crate::panel::editors::selected_module_name(app)
    {
        return name;
    }
    window_title(page).to_owned()
}

pub fn header_back(app: &App) -> bool {
    let page = crate::panel::current_page(app);
    !PLACES.contains(&page) && !SETTINGS.contains(&page) && page != Page::Settings
}

fn page_bar(app: &App, b: &mut B, r: Regions, chrome: &mut Vec<(Rect, Chrome)>) {
    let p = *b.p;
    let bar = r.bar;
    let caps = caption_buttons(app, b, bar, chrome);
    let y = bar.top + (bar.height() - b.px(28.0)) / 2.0;
    let mut right = caps - b.px(8.0);
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
        right -= b.px(28.0);
        b.icon_button(
            key(id),
            Rect::new(right, y, b.px(28.0), b.px(28.0)),
            icon,
            label,
            false,
            enabled,
            action,
        );
    }
    let page = crate::panel::current_page(app);
    let status = crate::panel::save_status(app, page);
    let caption = b.font(Font::Caption);
    let sw = b.measure(status, &caption).0 + b.px(12.0);
    right -= sw;
    b.text_in(
        Rect::new(right, bar.top, sw, bar.height()),
        status,
        &caption,
        p.ink3,
        Align::Start,
    );
    b.announce(status, app.docs.version);
    let mut end = r
        .inspector
        .map_or(right - b.px(12.0), |inspector| inspector.left - b.px(12.0));
    if r.stage.is_some() {
        let zoom = Rect::new(end - b.px(104.0), y, b.px(104.0), b.px(28.0));
        b.segmented(
            key("stage-zoom"),
            zoom,
            &["Fit", "100%"],
            Some(usize::from(app.panel.as_ref().unwrap().stage_actual)),
            "Stage zoom",
            true,
            |i| Action::Panel(Command::StageZoom(i == 1)),
        );
        end = zoom.left - b.px(8.0);
        if matches!(page, Page::Look | Page::Windows) {
            let scope = Rect::new(end - b.px(160.0), y, b.px(160.0), b.px(28.0));
            b.segmented(
                key("look-stage-scope"),
                scope,
                &["Desktop", "Windows"],
                Some(usize::from(page == Page::Windows)),
                "Look preview",
                true,
                |i| Action::Panel(Command::Open(if i == 0 { Page::Look } else { Page::Windows })),
            );
            end = scope.left - b.px(8.0);
        }
    }
    let mut x = bar.left + b.px(12.0);
    let toggle = Rect::new(x, y, b.px(28.0), b.px(28.0));
    let k = key("sidebar-toggle");
    b.wash(k, toggle, b.px(radius::CONTROL));
    let icon = Rect::new(x + b.px(6.0), y + b.px(7.0), b.px(16.0), b.px(14.0));
    let ink = if b.hovered(k) { p.ink1 } else { p.ink3 };
    b.stroke(icon, b.px(2.0), b.px(1.75), ink);
    b.vline(icon.left + b.px(5.0), icon.top, icon.bottom, ink);
    b.hit(k, toggle, Role::Button, if collapsed(app) { "Expand sidebar (Ctrl+B)" } else { "Collapse sidebar (Ctrl+B)" }, Action::Panel(Command::Sidebar), Behavior::Click);
    b.focus_ring(k, toggle, b.px(radius::CONTROL));
    x += b.px(36.0);
    let top_level = PLACES.contains(&page) || SETTINGS.contains(&page) || page == Page::Settings;
    let mut crumbs = vec![page];
    let mut next = parent(app, page);
    while let Some(level) = next.filter(|level| *level != Page::Root) {
        crumbs.push(level);
        next = parent(app, level);
    }
    crumbs.reverse();
    for (index, level) in crumbs.iter().enumerate() {
        let current = index + 1 == crumbs.len();
        if current && !top_level && matches!(page, Page::Create | Page::Effects | Page::Shader | Page::FirstRun) {
            b.icon_button(key("window-back"), Rect::new(x, y, b.px(28.0), b.px(28.0)), glyph::CHEVRON_LEFT, "Back", false, true, Action::Panel(Command::Back));
            x += b.px(32.0);
        }
        let title = page_title(app, *level);
        let font = b.font(if current { Font::Strong } else { Font::Caption });
        let width = (b.measure(&title, &font).0 + b.px(12.0)).min((end - x).max(0.0));
        let rect = Rect::new(x, y, width, b.px(28.0));
        if current {
            b.text_in(rect, &title, &font, p.ink1, Align::Start);
        } else {
            let k = key(("breadcrumb", level));
            b.wash(k, rect, b.px(radius::SMALL));
            b.text_in(rect, &title, &font, p.ink3, Align::Start);
            b.hit(
                k,
                rect,
                Role::Button,
                &format!("Go to {title}"),
                Action::Panel(Command::Open(*level)),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(radius::SMALL));
        }
        x += width;
        if !current {
            b.icon(
                Rect::new(x, y, b.px(16.0), b.px(28.0)),
                glyph::CHEVRON_RIGHT,
                11.0,
                p.ink3,
            );
            x += b.px(16.0);
        }
    }
}

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
        b.wash(k, r, b.px(radius::SMALL));
        b.icon(r, g, 10.0, if b.hovered(k) { p.ink1 } else { p.ink2 });
        b.hit(
            k,
            r,
            Role::Button,
            label,
            action,
            mypaper_ui::controls::Behavior::Click,
        );
        b.focus_ring(k, r, b.px(radius::SMALL));
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
    b.elevated(rect, b.px(b.kit.radius.card));
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

/// Labels stay within the stage and use the same dark plate over light or dark imagery.
fn stage_tag(b: &mut B, item: &Item, viewport: Rect) {
    let font = b.font(Font::Caption);
    let width = (b.measure(&item.label, &font).0 + b.px(16.0)).min(viewport.width() - b.px(16.0));
    let beside = item.rect.right + b.px(8.0);
    let (x, y) = if beside + width <= viewport.right - b.px(8.0) {
        (
            beside,
            item.rect.top + (item.rect.height() - b.px(24.0)) / 2.0,
        )
    } else {
        (
            item.rect.left.clamp(
                viewport.left + b.px(8.0),
                viewport.right - width - b.px(8.0),
            ),
            item.rect.bottom + b.px(8.0),
        )
    };
    let y = y.clamp(viewport.top + b.px(8.0), viewport.bottom - b.px(32.0));
    let rect = Rect::new(x, y, width, b.px(24.0));
    b.fill(
        rect,
        b.px(radius::SMALL),
        mypaper_ui::Rgba::new(0.04, 0.04, 0.04, 0.9),
    );
    b.text_in(
        Rect::new(x + b.px(8.0), y, width - b.px(16.0), rect.height()),
        &item.label,
        &font,
        mypaper_ui::Rgba::new(0.95, 0.95, 0.95, 1.0),
        Align::Start,
    );
}

fn off_card(b: &mut B, picture: Rect, viewport: Rect) {
    let visible = mypaper_ui::controls::intersect(picture, viewport).unwrap_or(viewport);
    let width = b.px(340.0).min(viewport.width() - b.px(32.0));
    let rect = Rect::new(
        viewport.left + (viewport.width() - width) / 2.0,
        visible.top + (visible.height() - b.px(100.0)) / 2.0,
        width,
        b.px(100.0),
    );
    b.elevated(rect, b.px(b.kit.radius.card));
    let title = b.font(Font::Title2);
    let caption = b.font(Font::Caption);
    b.text_in(
        Rect::new(
            rect.left + b.px(16.0),
            rect.top + b.px(16.0),
            rect.width() - b.px(32.0),
            b.px(28.0),
        ),
        "mypaper is off",
        &title,
        b.p.ink1,
        Align::Center,
    );
    b.text_in(
        Rect::new(
            rect.left + b.px(12.0),
            rect.top + b.px(50.0),
            rect.width() - b.px(24.0),
            b.px(26.0),
        ),
        "Changes apply when mypaper starts",
        &caption,
        b.p.ink3,
        Align::Center,
    );
}

/// A drawing of the configured desktop menu; its commands never execute inside the stage.
fn stage_menu(app: &App, b: &mut B, picture: Rect) {
    use mypaper_shell::preview::menu;
    let config = app
        .get(DocId::Config, "/desktop_menu")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let mut tokens = menu::tokens(
        &crate::app::theme_name(&app.docs).unwrap_or_default(),
        b.text,
    );
    tokens.shadow = false;
    let entries = menu::build(
        &config,
        menu::ViewState {
            icons_visible: true,
            ..Default::default()
        },
    );
    let layout = mypaper_ui::menu::layout(&entries, &tokens, b.s, b.text);
    let list = mypaper_ui::menu::view(&entries, &layout, None, &tokens);
    let x = picture.left + (picture.width() - layout.width) / 2.0;
    let y = picture.top + (picture.height() - layout.height) / 2.0;
    b.list.ops.extend(crate::stage::offset(&list.ops, x, y));
    b.hit(
        key("stage-menu"),
        Rect::new(x, y, layout.width, layout.height),
        Role::Image,
        "Desktop menu preview",
        Action::None,
        Behavior::Click,
    );
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn sidebar_motion_reuses_one_stage_picture_until_settled() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::window_app(true, &ui, (1280, 800));
        build(&app, &mut ui);
        let size = app.stage.borrow().content.size;
        app.panel.as_mut().unwrap().window.sidebar = Some(Transition {
            start: std::time::Instant::now(), from: 208.0, to: 56.0,
        });
        build(&app, &mut ui);
        let picture = app.stage.borrow().picture.clone().unwrap();
        for _ in 0..8 {
            let built = build(&app, &mut ui);
            assert_eq!(app.stage.borrow().content.size, size);
            assert!(std::sync::Arc::ptr_eq(&picture, app.stage.borrow().picture.as_ref().unwrap()));
            assert!(built.list.ops.iter().any(|op| matches!(op, mypaper_ui::Op::RoundedImage { image, .. } if std::sync::Arc::ptr_eq(&picture, image))));
        }
        app.panel.as_mut().unwrap().window.sidebar = None;
        build(&app, &mut ui);
        assert!(app.stage.borrow().picture.is_none());
    }

    #[test]
    fn settings_grow_and_groups_reflow_without_horizontal_overflow() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let out = std::env::var_os("MYPAPER_STUDIO_LAYOUT_SHOTS").map(std::path::PathBuf::from);
        for size in [(960, 640), (1280, 800), (1920, 1080), (2560, 1440)] {
            for dark in [true, false] {
                for (page, name) in [
                    (Page::Settings, "settings"),
                    (Page::General, "general"),
                    (Page::Performance, "performance"),
                    (Page::Keyboard, "keyboard"),
                    (Page::Troubleshoot, "troubleshoot"),
                    (Page::About, "about"),
                    (Page::Look, "look"),
                    (Page::Windows, "windows"),
                    (Page::Wallpapers, "wallpapers"),
                    (Page::Bars, "bars"),
                    (Page::Top, "top"),
                    (Page::Dock, "dock"),
                    (Page::Widgets, "widgets"),
                    (Page::Widget, "widget"),
                    (Page::DesktopMenu, "desktop-menu"),
                    (Page::Connections, "connections"),
                ] {
                    let mut app = crate::goldens::window_app(dark, &ui, size);
                    app.run(Action::Panel(Command::Open(page)));
                    if page == Page::Widget {
                        app.nav.widget = Some("clock-1".into());
                    }
                    let (host, stage) = content_regions(&app);
                    assert!(host.width() <= 1200.0);
                    if let Some(stage) = stage {
                        assert!(stage.width() > host.width());
                    }
                    let built = build(&app, &mut ui);
                    let scroll = key(("studio-panel", page));
                    for target in built
                        .targets
                        .iter()
                        .filter(|target| target.scroll == Some(scroll))
                    {
                        assert!(
                            target.rect.left >= host.left - 1.0
                                && target.rect.right <= host.right + 1.0,
                            "{name} {size:?}: {:?} outside {host:?}",
                            target.rect
                        );
                    }
                    if page == Page::General {
                        let heading = |name: &str| {
                            built
                                .list
                                .ops
                                .iter()
                                .find_map(|op| match op {
                                    mypaper_ui::Op::Text { rect, text, .. }
                                        if text.as_ref() == name =>
                                    {
                                        Some(*rect)
                                    }
                                    _ => None,
                                })
                                .unwrap()
                        };
                        let startup = heading("Startup");
                        let experimental = heading("Experimental");
                        if size.0 >= 1280 {
                            assert_eq!(startup.top, experimental.top);
                            assert!(experimental.left > startup.right);
                        } else {
                            assert_eq!(startup.left, experimental.left);
                            assert!(experimental.top > startup.bottom);
                        }
                    }
                    if let Some(out) = &out {
                        let image =
                            mypaper_ui::render(&mut ui, &built.list, size.0, size.1).unwrap();
                        mypaper_ui::golden::write(
                            &image,
                            &out.join(format!(
                                "{name}-{}x{}-{}.png",
                                size.0,
                                size.1,
                                if dark { "dark" } else { "light" }
                            )),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}
