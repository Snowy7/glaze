//! Opt-in v16 Studio panel. It shares the legacy app's documents, actions, input and workers;
//! only window placement, navigation and presentation belong to this module.

mod colors;
pub(crate) mod choosers;
pub(crate) mod material;
mod controls;
mod editors;
mod first_run;
pub(crate) use first_run::run as run_first_run;
mod history;
pub(crate) use history::time_label as history_time;
mod icons;
mod lists;
mod measure;
pub(crate) use measure::prepare_frame;
mod options;
mod pages;
mod peek;
mod pickers;
mod saving;
mod specimens;
mod wallpaper_look;
mod widget_look;
pub(crate) use widget_look::slider_edit;
pub use colors::swatches;
pub use lists::List as SortList;
pub use saving::{theme_failed, theme_saved};
mod tokens;

#[path = "../pick_client.rs"]
pub(crate) mod pick_client;

#[cfg(test)]
mod tests;

use mypaper_ui::controls::{Builder, key};
use mypaper_ui::{Chrome, Rect, Text};

use crate::actions::{Action, FieldId};
use crate::app::App;
use crate::view::Built;

pub const TITLE: &str = "mypaper Studio · Panel";

pub fn enabled() -> bool {
    std::env::var("MYPAPER_STUDIO_PANEL").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Root,
    Wallpapers,
    Look,
    Bars,
    BarDesign,
    Top,
    Dock,
    Widgets,
    DesktopMenu,
    Windows,
    Connections,
    Settings,
    Performance,
    Widget,
    AddWidget,
    Wallpaper,
    AddWallpaper,
    Connection,
    General,
    Keyboard,
    Troubleshoot,
    About,
    History,
    Search,
    Module,
    AddModule,
    Panels,
    BarPanel,
    FirstRun,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Self::Root => "Studio",
            Self::Wallpapers => "Wallpaper",
            Self::Look => "Look",
            Self::Bars => "Bars",
            Self::BarDesign => "Design",
            Self::Top => "Top bar",
            Self::Dock => "Dock",
            Self::Widgets => "Widgets",
            Self::DesktopMenu => "Desktop menu",
            Self::Windows => "Windows",
            Self::Connections => "Connections",
            Self::Settings => "Settings",
            Self::Performance => "Performance and power",
            Self::Widget => "Widget",
            Self::AddWidget => "Add a widget",
            Self::Wallpaper => "Wallpaper",
            Self::AddWallpaper => "Add a wallpaper",
            Self::Connection => "Connection",
            Self::General => "General",
            Self::Keyboard => "Keyboard",
            Self::Troubleshoot => "Troubleshoot",
            Self::About => "About",
            Self::History => "History",
            Self::Search => "Search",
            Self::Module => "Module",
            Self::AddModule => "Add a module",
            Self::Panels => "Panels",
            Self::BarPanel => "Panel",
            Self::FirstRun => "Make this desktop yours",
        }
    }

    fn parent(self) -> Option<Self> {
        match self {
            Self::Root => None,
            Self::Performance
            | Self::General
            | Self::Keyboard
            | Self::Troubleshoot
            | Self::About => Some(Self::Settings),
            Self::Top | Self::Dock => Some(Self::Bars),
            Self::BarDesign => Some(Self::Bars),
            Self::Widget | Self::AddWidget => Some(Self::Widgets),
            Self::Wallpaper | Self::AddWallpaper => Some(Self::Wallpapers),
            Self::Connection => Some(Self::Connections),
            Self::Module | Self::AddModule => Some(Self::Top),
            Self::Panels => Some(Self::Bars),
            Self::BarPanel => Some(Self::Panels),
            _ => Some(Self::Root),
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub chooser: std::cell::RefCell<choosers::State>,
    reclaim_heap: std::cell::Cell<bool>,
    raster: measure::Raster,
    pub page: Page,
    pub first_run: std::cell::RefCell<Option<first_run::Baseline>>,
    pub add_widget: Option<String>,
    pub category: Option<String>,
    pub add_module: Option<String>,
    pub history: std::cell::RefCell<history::History>,
    pub theme_data: std::cell::RefCell<saving::ThemeCache>,
    pub search_match: Option<(Page, String)>,
    pub specimens:
        std::cell::RefCell<std::collections::HashMap<String, std::sync::Arc<mypaper_ui::Image>>>,
    specimen_page: std::cell::Cell<Option<Page>>,
    pub save_doc: std::cell::Cell<Option<crate::docs::DocId>>,
    pub sort_drag: Option<lists::Drag>,
    pub color_drag: Option<colors::Drag>,
    pub color_overlay: std::cell::RefCell<Vec<mypaper_ui::Op>>,
    pub color_values: std::cell::RefCell<std::collections::HashMap<mypaper_ui::controls::Key, (String, mypaper_ui::color_row::popover::Hsv)>>,
    pub fonts: std::cell::OnceCell<Vec<String>>,
    pub save_errors: std::cell::RefCell<std::collections::HashMap<crate::docs::DocId, String>>,
    pub theme_retry: Option<crate::actions::Action>,
    pub theme_errors: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, String>>,
    reset_version: std::cell::Cell<Option<(crate::docs::DocId, u64)>>,
    auto_left: std::cell::Cell<Option<bool>>,
}

/// Release temporary page allocations after the previous display list has been replaced.
pub fn after_present(app: &App) {
    measure::reclaim(app);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open(Page),
    Back,
    Done,
    PickWidget(String),
    AddWidget,
    Wallpaper(usize),
    WallpaperLook(usize),
    Connection(usize),
    Scope(bool),
    Category(Option<String>),
    Search(usize),
    History(usize),
    Restore,
    CancelRestore,
    ClearHistory,
    RetryHistory,
    Disconnect(String),
    Connect(String),
    MoveModuleTo(crate::actions::Bar, &'static str),
    Side(bool),
    PickAsset(String),
    PickModule(String),
    AddModule,
    Display(usize),
    Peek,
    PickColor(crate::actions::Edit),
    ColorDrag(crate::actions::Edit, mypaper_ui::color_row::popover::Part, Rect),
    SortRow(SortList, usize),
    SortHandle(SortList, usize),
    SortRemove(SortList, usize),
    AddMenuItem,
    PickZone(pickers::ZoneTarget, usize),
    ConnectionModule(String, crate::actions::Bar, bool),
    HideConnectionWidget(String),
    ShowTheme(std::path::PathBuf),
    ResetTheme(std::path::PathBuf),
    ResetPart(crate::docs::DocId, Option<&'static str>),
    RetryTheme,
    ShowThemeFolder(std::path::PathBuf),
    RetryDoc(crate::docs::DocId),
    ShowFolder(crate::docs::DocId),
    Design(Option<crate::actions::Bar>),
    SameBars,
}

pub fn run(app: &mut App, command: Command) {
    app.blur_field();
    if app.panel.is_none() {
        return;
    }
    let keep_focus = matches!(
        &command,
        Command::PickWidget(_)
            | Command::PickColor(_)
            | Command::PickModule(_)
            | Command::SortRow(_, _)
            | Command::SortHandle(_, _)
    );
    match command {
        Command::Design(bar) => {
            app.nav.bar = bar;
            app.nav.module = None;
            app.panel.as_mut().unwrap().page = Page::BarDesign;
        }
        Command::SameBars => editors::same_bars(app),
        Command::Open(page) => {
            app.overlay = None;
            if page == Page::FirstRun {
                app.panel.as_ref().unwrap().first_run.borrow_mut().take();
                first_run::begin(app);
                app.overlay = Some(crate::app::Overlay::FirstRun {
                    step: 0,
                    starter: 0,
                    taskbar: 0,
                });
            }
            if page == Page::AddWallpaper {
                app.overlay = Some(crate::app::Overlay::AddWallpaper {
                    source: None,
                    picked: None,
                });
            }
            app.panel.as_mut().unwrap().page = page;
            app.nav.page = Some(match page {
                Page::Wallpapers | Page::Wallpaper | Page::AddWallpaper => {
                    crate::actions::Page::Wallpapers
                }
                Page::Look | Page::Windows => crate::actions::Page::Look,
                Page::Widgets | Page::Widget | Page::AddWidget => crate::actions::Page::Widgets,
                Page::Bars
                | Page::Top
                | Page::Dock
                | Page::Module
                | Page::AddModule
                | Page::Panels
                | Page::BarPanel => crate::actions::Page::Bars,
                Page::Connections | Page::Connection => crate::actions::Page::Integrations,
                Page::Settings
                | Page::General
                | Page::Performance
                | Page::Keyboard
                | Page::Troubleshoot
                | Page::About
                | Page::DesktopMenu => crate::actions::Page::Settings,
                _ => crate::actions::Page::Home,
            });
            if page == Page::Widgets {
                app.nav.widget = None;
            }
            if page == Page::Performance && !app.fixture {
                app.system.gpus();
            }
            if page == Page::Top {
                app.nav.bar = Some(crate::actions::Bar::Top);
            }
            if page == Page::Dock {
                app.nav.bar = Some(crate::actions::Bar::Dock);
            }
            if matches!(page, Page::Top | Page::Dock) {
                app.nav.module = None;
            }
            if page == Page::Connections {
                app.nav.page = Some(crate::actions::Page::Integrations);
                app.nav.plugins = true;
                if !app.fixture {
                    app.plugins.refresh();
                }
            }
            app.nav.look_windows = page == Page::Windows;
        }
        Command::Back => {
            app.overlay = None;
            if app.panel.as_ref().unwrap().page == Page::BarDesign {
                let page = match app.nav.bar { Some(crate::actions::Bar::Top) => Page::Top, Some(crate::actions::Bar::Dock) => Page::Dock, None => Page::Bars };
                run(app, Command::Open(page));
                return;
            }
            let state = app.panel.as_mut().unwrap();
            if state.page == Page::Module
                || app.nav.module.is_some() && matches!(state.page, Page::Top | Page::Dock)
            {
                app.nav.module = None;
                state.page = if app.nav.bar == Some(crate::actions::Bar::Dock) {
                    Page::Dock
                } else {
                    Page::Top
                };
            } else if state.page == Page::AddModule {
                state.page = if app.nav.bar == Some(crate::actions::Bar::Dock) {
                    Page::Dock
                } else {
                    Page::Top
                };
            } else {
                state.page = state.page.parent().unwrap_or(Page::Root);
            }
            state.history.borrow_mut().selected = None;
        }
        Command::Done => app.quit = true,
        Command::Disconnect(id) => editors::disconnect(app, &id),
        Command::Connect(id) => editors::connect(app, &id),
        Command::MoveModuleTo(bar, zone) => editors::move_module(app, bar, zone),
        Command::Display(index) => {
            if index < app.monitors.len() {
                app.nav.display = index;
                app.nav.widget = None;
                app.nav.module = None;
                pick_client::clear(app);
                move_to_display(app);
            }
        }
        Command::Peek => peek::start(app, None),
        Command::PickColor(edit) => colors::pick(app, &edit),
        Command::ColorDrag(..) => {},
        Command::SortRow(list, index) => {
            app.input.focus = Some(lists::row_key(&list, index));
            if let SortList::Widget(path) = &list {
                app.run(Action::More(key(("panel-list-item", path.as_str(), index))));
            }
            if list == SortList::Menu {
                app.run(Action::More(key(("panel-menu-item", index))));
            }
        }
        Command::SortHandle(list, index) => app.input.focus = Some(lists::row_key(&list, index)),
        Command::SortRemove(list, index) => lists::remove(app, &list, index),
        Command::AddMenuItem => editors::add_menu_item(app),
        Command::PickZone(target, city) => pickers::choose_zone(app, &target, city),
        Command::ConnectionModule(id, bar, on) => editors::connection_module(app, &id, bar, on),
        Command::HideConnectionWidget(id) => editors::hide_connection_widget(app, &id),
        Command::ShowTheme(dir) => crate::system::reveal(&dir.join(mypaper_theme::MANIFEST)),
        Command::ResetTheme(dir) => saving::reset_theme(app, &dir),
        Command::ResetPart(doc, path) => saving::reset_part(app, doc, path),
        Command::ShowThemeFolder(dir) => crate::system::open(&dir.to_string_lossy()),
        Command::RetryTheme => {
            if let Some(action) = app.panel.as_ref().unwrap().theme_retry.clone() {
                app.run(action);
            }
        }
        Command::ShowFolder(doc) => {
            if let Some(folder) = app
                .docs
                .doc(doc)
                .path
                .as_deref()
                .and_then(std::path::Path::parent)
            {
                crate::system::open(&folder.to_string_lossy());
            }
        }
        Command::RetryDoc(doc) => match app.docs.retry() {
            Ok(()) => {
                sync_connections(app);
                app.panel
                    .as_ref()
                    .unwrap()
                    .save_errors
                    .borrow_mut()
                    .remove(&doc);
                app.retheme();
                app.library
                    .set_active(crate::app::theme_name(&app.docs).as_deref());
            }
            Err(error) => save_failed(app, Some(doc), &format!("{error:#}")),
        },
        Command::PickWidget(id) => {
            let state = app.panel.as_mut().unwrap();
            state.add_widget = Some(id);
            if state.page != Page::AddWidget {
                state.page = Page::AddWidget;
            }
        }
        Command::AddWidget => {
            if let Some(id) = app.panel.as_ref().unwrap().add_widget.clone() {
                if !editors::can_add_widget(app, &id) {
                    app.alert("This widget has reached its instance limit");
                    return;
                }
                app.run(Action::AddWidget(id));
                if app.nav.widget.is_some() {
                    app.panel.as_mut().unwrap().page = Page::Widget;
                }
            }
        }
        Command::Wallpaper(i) => {
            app.nav.wallpaper = Some(i);
            app.panel.as_mut().unwrap().page = Page::Wallpaper;
        }
        Command::WallpaperLook(i) => {
            if i < app.library.items.len() {
                app.nav.wallpaper = Some(i);
                app.nav.look_theme = true;
                app.panel.as_mut().unwrap().page = Page::Look;
            }
        }
        Command::Connection(i) => {
            app.nav.integration = i;
            app.panel.as_mut().unwrap().page = Page::Connection;
        }
        Command::Scope(theme) => {
            if theme && !app.nav.look_theme { app.nav.wallpaper = app.library.active(); }
            app.nav.look_theme = theme;
            app.panel.as_mut().unwrap().page = Page::Look;
        }
        Command::Category(category) => app.panel.as_mut().unwrap().category = category,
        Command::Search(i) => editors::run_search(app, i),
        Command::History(i) => app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(i),
        Command::Restore => history::restore(app),
        Command::CancelRestore => app.panel.as_ref().unwrap().history.borrow_mut().selected = None,
        Command::ClearHistory => history::clear(app),
        Command::RetryHistory => history::retry(app),
        Command::Side(left) => {
            app.set(
                &crate::actions::Edit::new(crate::docs::DocId::Studio, "/panel_side", "Panel side"),
                serde_json::json!(if left { "left" } else { "right" }),
            );
            move_to_display(app);
        }
        Command::PickAsset(path) => editors::pick_asset(app, &path),
        Command::PickModule(id) => app.panel.as_mut().unwrap().add_module = Some(id),
        Command::AddModule => editors::insert_module(app),
    }
    if !keep_focus {
        app.input.focus = None;
    }
    app.input.hover = None;
    app.dirty = true;
}

/// Panel shortcuts are resolved before the legacy app's page shortcuts.
pub fn on_key(app: &mut App, vk: u16, (ctrl, shift, alt): (bool, bool, bool)) -> bool {
    if colors::on_key(app, vk, ctrl, shift, alt) { return true; }
    if choosers::on_key(app, vk, ctrl, shift, alt) { return true; }
    if vk == 0x1B && choosers::end(app) { app.dirty = true; return true; }
    if pick_client::on_key(app, vk) {
        return true;
    }
    if !ctrl
        && vk == 0x1B
        && app.field.is_none()
        && app
            .panel
            .as_ref()
            .is_some_and(|s| s.page == Page::AddWallpaper)
    {
        run(app, Command::Back);
        return true;
    }
    if !ctrl && vk == 0x0D && app.field.as_ref().is_some_and(|f| f.id == FieldId::Palette) {
        app.blur_field();
        app.panel.as_mut().unwrap().page = Page::Search;
        return true;
    }
    if !ctrl {
        if vk == 0x2E && app.field.is_none() && app.panel.as_ref().is_some_and(|s| s.page == Page::Module) && let Some(slot) = app.nav.module.clone() {
            lists::remove(app, &SortList::Modules(slot.bar, slot.list), slot.index);
            return true;
        }
        if alt
            && matches!(vk, 0x26 | 0x28)
            && app.field.is_none()
            && lists::keyboard(app, if vk == 0x26 { -1 } else { 1 })
        {
            return true;
        }
        if app.panel.as_ref().is_some_and(|s| s.page == Page::Widget)
            && app.field.is_none()
            && app.input.focus.is_none()
        {
            let (dx, dy) = match vk {
                0x25 => (-1, 0),
                0x27 => (1, 0),
                0x26 => (0, -1),
                0x28 => (0, 1),
                0x2E => {
                    app.run(Action::RemoveWidget);
                    return true;
                }
                _ => return false,
            };
            if shift {
                crate::pages::widgets::nudge(app, 0, 0, dx, dy);
            } else {
                crate::pages::widgets::nudge(app, dx, dy, 0, 0);
            }
            return true;
        }
        return false;
    }
    match vk {
        0x30 | 0x24 => run(app, Command::Open(Page::Root)),
        0x5A if !shift => app.run(Action::Undo),
        0x5A | 0x59 => app.run(Action::Redo),
        0x57 => run(app, Command::Done),
        0x4B => {
            run(app, Command::Open(Page::Root));
            app.focus_field(FieldId::Palette);
        }
        0x46 => {
            let field = match app.panel.as_ref().unwrap().page {
                Page::Wallpapers => FieldId::WallpaperSearch,
                Page::AddWidget => FieldId::WidgetSearch,
                _ => {
                    run(app, Command::Open(Page::Root));
                    FieldId::Palette
                }
            };
            app.focus_field(field);
        }
        0x31..=0x37 => run(
            app,
            Command::Open(
                [
                    Page::Wallpapers,
                    Page::Look,
                    Page::Bars,
                    Page::Widgets,
                    Page::DesktopMenu,
                    Page::Windows,
                    Page::Connections,
                ][usize::from(vk - 0x31)],
            ),
        ),
        0xBC => run(app, Command::Open(Page::Settings)),
        0x48 => run(app, Command::Open(Page::History)),
        0x44 if !shift => peek::start(app, Some(0x44)),
        _ => return false,
    }
    true
}

pub fn palette_for(app: &App) -> mypaper_ui::controls::Palette {
    if app.panel.is_some() && !app.theme.contrast {
        tokens::palette(app.theme.palette.dark)
    } else {
        app.theme.palette
    }
}

pub fn build(app: &App, text: &mut Text) -> Built {
    pick_client::connect(app);
    choosers::begin(app);
    history::observe(app);
    let mut page = app.panel.as_ref().map_or(Page::Root, |s| s.page);
    if matches!(page, Page::Top | Page::Dock) && app.nav.module.is_some() {
        page = Page::Module;
    }
    let state = app.panel.as_ref().unwrap();
    if state.specimen_page.replace(Some(page)) != Some(page) {
        state.specimens.borrow_mut().clear();
        measure::leave_page(app, text);
    }
    let p = palette_for(app);
    let ty = app.theme.ty.clone();
    let mut b = Builder::new(text, &p, &ty, app.scale, &app.input);
    let window = Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32);
    b.fill(window, 0.0, p.chrome);
    let header = Rect::new(0.0, 0.0, window.width(), b.px(48.0));
    pages::header(app, &mut b, header, page);
    let footer = editors::footer_height(app);
    let body = Rect::new(
        0.0,
        header.bottom,
        window.width(),
        (window.height() - header.height() - app.px(footer)).max(0.0),
    );
    let view = key(("studio-panel", page));
    b.scroll(view, body, app.nav.scroll(view), Action::None, |b, top| {
        let mut c = crate::ui::Col {
            x: b.px(16.0),
            y: top + b.px(4.0),
            w: body.width() - b.px(32.0),
            view,
        };
        controls::save_banner(app, b, &mut c, page);
        if !app.system.running() {
            controls::notice(b, &mut c, "mypaper is off. Changes apply when it starts.");
        }
        pages::view(app, b, &mut c, page);
        controls::boundaries(b);
        colors::finish(app, b);
        c.y + b.px(16.0)
    });
    editors::footer(
        app,
        &mut b,
        Rect::new(0.0, body.bottom, window.width(), app.px(footer)),
    );
    pages::toast(app, &mut b, window);
    choosers::finish(app);
    Built {
        list: b.list,
        semantics: b.semantics,
        targets: b.targets,
        scrolls: b.scrolls,
        chrome: vec![(header, Chrome::Caption)],
        regions: vec![header, body],
        overlay_from: if app.overlay.is_some() { 0 } else { usize::MAX },
    }
}

/// Reserve a top-bar band and 96 DIPs for a floating dock, within the primary work area.
pub fn placement(app: &App) -> (i32, i32, i32, i32) {
    let monitor = app.display();
    let fallback = mypaper_desktop::Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let work = monitor.map_or(fallback, |m| m.work);
    let screen = monitor.map_or(fallback, |m| m.rect);
    let scale = monitor.map_or(app.scale, |m| m.dpi as f32 / 96.0);
    let mut rect = bounds(screen, work, scale);
    let saved = app
        .get(crate::docs::DocId::Studio, "/panel_side")
        .and_then(serde_json::Value::as_str);
    let left = match saved {
        Some("left") => true,
        Some("right") => false,
        _ => app.panel.as_ref().is_some_and(|state| {
            state.auto_left.get().unwrap_or_else(|| {
                let left = first_side(app, work, rect);
                state.auto_left.set(Some(left));
                left
            })
        }),
    };
    if left {
        rect.0 = work.x + (8.0 * scale).round() as i32;
    }
    rect
}

/// On the first open, prefer the side with less of the foreground work window in it.
/// Cache that decision so the panel never jumps when focus changes. An explicit side is saved.
fn first_side(app: &App, work: mypaper_desktop::Rect, right: (i32, i32, i32, i32)) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
    let mut foreground = RECT::default();
    // SAFETY: read-only geometry of the foreground window; no windows are moved or activated.
    let ok = unsafe {
        let window = GetForegroundWindow();
        app.window != Some(window) && GetWindowRect(window, &mut foreground).is_ok()
    };
    if !ok {
        return false;
    }
    let overlap = |x: i32| {
        let width = (foreground.right.min(x + right.2) - foreground.left.max(x)).max(0);
        let height =
            (foreground.bottom.min(right.1 + right.3) - foreground.top.max(right.1)).max(0);
        i64::from(width) * i64::from(height)
    };
    let margin = work.x + work.width - (right.0 + right.2);
    overlap(right.0) > overlap(work.x + margin)
}

/// Fit the client to measured page content; retain the desktop bands as the upper limit.
pub fn fit_height(app: &mut App, surface: &mypaper_ui::Surface, built: &Built) -> bool {
    let Some(area) = built.scrolls.last() else {
        return false;
    };
    let mut rect = placement(app);
    rect.3 = (area.content + app.px(48.0 + editors::footer_height(app))).ceil() as i32;
    rect.3 = rect.3.clamp(
        (app.px(160.0) as i32).min(placement(app).3),
        placement(app).3,
    );
    if surface.client_size() == (rect.2 as u32, rect.3 as u32) {
        return false;
    }
    fit_client(surface, rect);
    app.size = surface.client_size();
    true
}

/// One additive bridge into the legacy action dispatcher, shared with desktop picking.
pub fn before_action(app: &mut App, action: &Action) {
    if matches!(action, Action::LookReset) && let Some(panel) = &app.panel {
        app.nav.look_windows = panel.page == Page::Windows;
    }
    if app.panel.is_some() && matches!(action, Action::AddWidget(_) | Action::DuplicateWidget) {
        editors::prepare_widget_grid(app);
    }
    if matches!(action, Action::AddWallpaper | Action::Panel(Command::Open(Page::AddWallpaper))) {
        app.texts.remove(&FieldId::Text(crate::docs::DocId::Studio, "/add_use_now".into()));
    }
    if app.panel.is_some() { choosers::end(app); }
    history::observe(app);
    if let Action::Set(edit, _) | Action::Unset(edit) = action
        && edit.path.starts_with("/theme:")
    {
        app.panel.as_mut().unwrap().theme_retry = Some(action.clone());
    }
    if matches!(action, Action::Undo | Action::Redo) {
        app.texts.retain(
            |id, _| !matches!(id, FieldId::Text(doc, _) if *doc != crate::docs::DocId::Studio),
        );
    }
    if let Some(state) = &app.panel {
        if let Action::StartOver(doc) = action {
            state.reset_version.set(Some((*doc, app.docs.version)));
        }
        let doc = match action {
            Action::Set(e, _) | Action::Toggle(e, _) | Action::Unset(e) | Action::Slider(e) => {
                Some(e.doc)
            }
            Action::SetStyle(_) | Action::WindowsLook(_) | Action::UseWallpaper(_) => {
                Some(crate::docs::DocId::Config)
            }
            Action::Preset(_) => Some(crate::docs::DocId::Bars),
            _ => None,
        };
        if let Some(doc) = doc {
            state.save_doc.set(Some(doc));
        }
    }
    if matches!(action, Action::AddCommit(_))
        && app
            .panel
            .as_ref()
            .is_some_and(|s| s.page == Page::AddWallpaper)
        && matches!(
            &app.overlay,
            Some(crate::app::Overlay::AddWallpaper {
                source: Some(crate::actions::Source::Web),
                picked: None
            })
        )
        && app.text(&FieldId::AddName).trim().is_empty()
    {
        let name = crate::library::suggested_name(app.text(&FieldId::AddUrl));
        app.texts.insert(FieldId::AddName, name);
    }
    if matches!(
        action,
        Action::Panel(Command::Open(_) | Command::Back | Command::Scope(_))
            | Action::Undo
            | Action::Redo
            | Action::Field(_)
    ) {
        if let Some(state) = &app.panel {
            state.history.borrow_mut().end_picks();
        }
    }
    let Some(state) = app.panel.as_mut() else {
        return;
    };
    match action {
        Action::SelectWidget(Some(_)) => state.page = Page::Widget,
        Action::SelectWidget(None) | Action::RemoveWidget => state.page = Page::Widgets,
        Action::AddWidgetSheet => state.page = Page::AddWidget,
        Action::AddWallpaper => state.page = Page::AddWallpaper,
        Action::SelectIntegration(_) => state.page = Page::Connection,
        Action::LookScope(_) => state.page = Page::Look,
        Action::SelectModule(Some(_)) => state.page = Page::Module,
        Action::SelectModule(None) => {
            state.page = if app.nav.bar == Some(crate::actions::Bar::Dock) {
                Page::Dock
            } else {
                Page::Top
            }
        }
        Action::SelectPanel(Some(_)) => state.page = Page::BarPanel,
        Action::SelectPanel(None) => state.page = Page::Panels,
        Action::AddModule(_) => state.page = Page::AddModule,
        _ => {}
    }
}

/// Remove panel-only hold hooks before the legacy message handler restores our window.
pub fn end_peek() {
    peek::end();
}

/// List drags are transient until release. This early bridge leaves legacy input intact.
pub fn event(app: &mut App, event: &mypaper_ui::Event) -> bool {
    if app.panel.is_some() { choosers::event(app, event); }
    if app.panel.is_some() && colors::event(app, event) { return true; }
    app.panel.is_some() && lists::event(app, event)
}

fn move_to_display(app: &App) {
    use windows::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::*,
    };
    let Some(window) = app.window else { return };
    let (x, y, _, _) = placement(app);
    let mut outer = RECT::default();
    let mut client = POINT::default();
    // SAFETY: only our own panel moves; changing display never activates another window.
    unsafe {
        if GetWindowRect(window, &mut outer).is_ok()
            && ClientToScreen(window, &mut client).as_bool()
        {
            let _ = SetWindowPos(
                window,
                None,
                x - (client.x - outer.left),
                y - (client.y - outer.top),
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
}

pub fn after_action(app: &mut App) {
    let Some(state) = app.panel.as_mut() else {
        return;
    };
    if let Some((doc, version)) = state.reset_version.take()
        && app.docs.version != version
        && app.docs.error(doc).is_none()
    {
        state.save_errors.borrow_mut().remove(&doc);
        app.docs.saved = crate::docs::Saved::Done(std::time::Instant::now());
    }
    if matches!(app.overlay, Some(crate::app::Overlay::FirstRun { .. })) {
        state.page = Page::FirstRun;
    } else if state.page == Page::FirstRun {
        state.page = Page::Root;
    }
    // The panel presents creation as a pushed page; legacy actions still own its source data.
    if state.page == Page::AddWidget && matches!(app.overlay, Some(crate::app::Overlay::AddWidget))
    {
        app.overlay = None;
    }
    if state.page == Page::AddModule
        && matches!(app.overlay, Some(crate::app::Overlay::AddModule { .. }))
    {
        app.overlay = None;
    }
    if state.page == Page::Module && app.nav.module.is_none() {
        state.page = if app.nav.bar == Some(crate::actions::Bar::Dock) {
            Page::Dock
        } else {
            Page::Top
        };
    }
    if state.page == Page::AddWallpaper && app.overlay.is_none() && app.nav.wallpaper.is_some() {
        state.page = Page::Wallpaper;
    }
}

pub fn record_edit(app: &mut App, label: &str, ops: &[crate::docs::Op], checkpoint: usize) {
    if let Some(state) = &app.panel {
        for op in ops {
            state.save_errors.borrow_mut().remove(&op.doc);
        }
        app.texts.retain(|id,_| !matches!(id,FieldId::Text(doc,path) if ops.iter().any(|op| op.doc == *doc && (path.as_ref() == op.path || path.starts_with(&format!("{}/",op.path))))));
    }
    history::record_edit(app, label, ops, checkpoint);
}

pub fn save_failed(app: &mut App, doc: Option<crate::docs::DocId>, error: &str) {
    if let Some(state) = &app.panel {
        if let Some(doc) = doc {
            state.save_doc.set(Some(doc));
            state.save_errors.borrow_mut().insert(doc, error.into());
        }
        app.docs.saved = crate::docs::Saved::Failed(error.into());
    }
}

pub fn record_drag(app: &App, label: &str) {
    history::record_drag(app, label);
}

/// Windows' explicit null means keep Windows' own value, whereas missing inherits the wallpaper.
pub fn intercept_set(
    app: &mut App,
    edit: &crate::actions::Edit,
    value: &serde_json::Value,
) -> bool {
    if app.panel.is_some() && let Some((edit, value)) = slider_edit(edit, value) {
        app.set(&edit, value);
        return true;
    }
    if app.panel.is_some() && colors::intercept_set(app, edit, value) { return true; }
    if app.panel.is_some() && wallpaper_look::intercept_set(app, edit, value) { return true; }
    if app.panel.is_some() && editors::intercept_page_set(app, edit, value) { return true; }
    if app.panel.is_some()
        && edit.doc == crate::docs::DocId::Studio
        && edit.path.starts_with("/plugins/setting/")
    {
        app.docs.reload(crate::docs::DocId::Plugins);
        if app.docs.error(crate::docs::DocId::Plugins).is_some() {
            return true;
        }
    }
    if app.panel.is_some()
        && edit.doc == crate::docs::DocId::Bars
        && edit.path.starts_with("/modules/")
        && edit.path.ends_with("/interval")
        && let Some(text) = value.as_str()
    {
        match text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
        {
            Some(number) => app.apply(
                &edit.label,
                vec![crate::docs::Op {
                    doc: edit.doc,
                    path: edit.path.to_string(),
                    value: Some(serde_json::Value::Number(number)),
                }],
            ),
            None => app.alert("Enter the interval as a number of seconds"),
        }
        return true;
    }
    if app.panel.is_none() || edit.doc != crate::docs::DocId::Studio {
        return false;
    }
    if edit.path.starts_with("/panel/draft/") {
        app.texts.insert(
            FieldId::Text(edit.doc, edit.path.clone()),
            value.as_str().unwrap_or("").to_owned(),
        );
        return true;
    }
    if let Some(index) = edit
        .path
        .strip_prefix("/panel/menu_args/")
        .and_then(|s| s.parse::<usize>().ok())
    {
        match crate::library::split_args(value.as_str().unwrap_or("")) {
            Ok(args) => app.set(
                &crate::actions::Edit::new(
                    crate::docs::DocId::Config,
                    format!("/desktop_menu/items/{index}/args"),
                    "Menu arguments",
                ),
                serde_json::json!(args),
            ),
            Err(error) => app.alert(error.to_string()),
        }
        return true;
    }
    if edit.path.as_ref() == "/panel/header" {
        match value.as_str() {
            Some("history") => run(app, Command::Open(Page::History)),
            Some("undo") => app.run(Action::Undo),
            Some("redo") => app.run(Action::Redo),
            _ => {}
        }
        return true;
    }
    let Some(field) = edit.path.strip_prefix("/panel/windows/") else {
        return false;
    };
    if ![
        "accent",
        "corners",
        "backdrop",
        "apps",
        "cursors",
        "border",
        "caption",
        "caption_text",
    ]
    .contains(&field)
    {
        return false;
    }
    app.apply(
        &edit.label,
        vec![crate::docs::Op {
            doc: crate::docs::DocId::Config,
            path: format!("/look/windows/{field}"),
            value: Some(value.clone()),
        }],
    );
    true
}

/// Manifest defaults and transient drafts aren't stored in the raw documents yet.
pub fn field_initial(app: &App, id: &FieldId) -> Option<String> {
    if app.panel.is_some() && let Some(value) = colors::field_initial(app, id) { return Some(value); }
    if app.panel.is_some() && let Some(value) = editors::field_initial(app, id) { return Some(value); }
    if app.panel.is_none() {
        return None;
    }
    let draft = matches!(id, FieldId::Text(crate::docs::DocId::Studio, path) if path.starts_with("/panel/draft/") || path.starts_with("/list_new") || matches!(path.as_ref(), "/menu_label" | "/menu_command" | "/new_custom" | "/add_fit" | "/add_module_list"));
    // Persisted fields compare against their document/default when committing, never their
    // just-typed cache (which otherwise makes every text edit appear unchanged).
    if draft || !matches!(id, FieldId::Text(..) | FieldId::Hex(_)) {
        if let Some(value) = app.texts.get(id) {
            return Some(value.clone());
        }
    }
    let FieldId::Text(doc, path) = id else {
        return None;
    };
    if *doc == crate::docs::DocId::Studio
        && let Some(rest) = path.strip_prefix("/panel/draft/module-trigger/")
    {
        if let Some(value) = app.texts.get(id) {
            return Some(value.clone());
        }
        if let Some((module, trigger)) = rest.split_once('/') {
            return Some(
                app.get(
                    crate::docs::DocId::Bars,
                    &format!("/modules/{module}/click/{trigger}/exec"),
                )
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .into(),
            );
        }
    }
    let color = *doc == crate::docs::DocId::Config
        && matches!(
            path.as_ref(),
            "/look/accent"
                | "/look/windows/accent"
                | "/look/material/tint"
                | "/look/windows/border"
                | "/look/windows/caption"
                | "/look/windows/caption_text"
        )
        || *doc == crate::docs::DocId::Widgets && path.ends_with("/appearance/tint");
    if color {
        return Some(
            app.get(*doc, path)
                .and_then(serde_json::Value::as_str)
                .filter(|s| s.starts_with('#'))
                .unwrap_or("")
                .to_owned(),
        );
    }
    if *doc == crate::docs::DocId::Studio && path.starts_with("/module_args/") {
        let module = path.trim_start_matches("/module_args/");
        return Some(
            app.get(crate::docs::DocId::Bars, &format!("/modules/{module}/args"))
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(|s| {
                    if s.contains(' ') {
                        format!("\"{s}\"")
                    } else {
                        s.into()
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if *doc == crate::docs::DocId::Studio
        && let Some(index) = path
            .strip_prefix("/panel/menu_args/")
            .and_then(|s| s.parse::<usize>().ok())
    {
        return Some(
            app.get(
                crate::docs::DocId::Config,
                &format!("/desktop_menu/items/{index}/args"),
            )
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(|s| {
                if s.contains(' ') {
                    format!("\"{s}\"")
                } else {
                    s.into()
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        );
    }
    if *doc == crate::docs::DocId::Widgets {
        let parts: Vec<_> = path.split('/').collect();
        if parts.get(1) != Some(&"instances") || parts.get(3) != Some(&"settings") {
            return None;
        }
        let index = parts.get(2)?.parse::<usize>().ok()?;
        let layout = crate::pages::widgets::layout(app);
        let instance = layout.instances.get(index)?;
        let manifest = &app.registry.get(&instance.widget)?.manifest;
        let resolved = serde_json::Value::Object(manifest.resolve_settings(&instance.settings));
        let pointer = format!("/{}", parts.get(4..)?.join("/"));
        return resolved.pointer(&pointer).map(|value| {
            value.as_str().map_or_else(
                || {
                    if value.is_null() {
                        String::new()
                    } else {
                        value.to_string()
                    }
                },
                str::to_owned,
            )
        });
    }
    None
}

/// `Surface::place` sizes the outer Win32 frame. Compensate for its invisible resize
/// borders so the opaque client panel itself is exactly 480 (or 360) DIPs wide.
pub fn fit_client(surface: &mypaper_ui::Surface, bounds: (i32, i32, i32, i32)) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut outer = RECT::default();
    let mut client = POINT::default();
    // SAFETY: read-only geometry queries on our own window into local out-pointers.
    let queried = unsafe {
        GetWindowRect(surface.hwnd(), &mut outer).is_ok()
            && ClientToScreen(surface.hwnd(), &mut client).as_bool()
    };
    if !queried {
        return;
    }
    let size = surface.client_size();
    surface.place((
        bounds.0 - (client.x - outer.left),
        bounds.1 - (client.y - outer.top),
        bounds.2 + (outer.right - outer.left - size.0 as i32),
        bounds.3 + (outer.bottom - outer.top - size.1 as i32),
    ));
}

fn bounds(
    screen: mypaper_desktop::Rect,
    work: mypaper_desktop::Rect,
    scale: f32,
) -> (i32, i32, i32, i32) {
    let px = |v: f32| (v * scale).round() as i32;
    let width = px(if (screen.width as f32 / scale) < 1400.0 {
        360.0
    } else {
        480.0
    })
    .min((work.width - px(16.0)).max(1));
    let top = work.y.max(screen.y + px(40.0));
    let bottom = work.y + work.height - px(96.0);
    (
        work.x + work.width - px(8.0) - width,
        top,
        width,
        (bottom - top).max(1),
    )
}

pub fn round_window(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    // SAFETY: the DWM attribute is sized and belongs to our own opaque App window. The App
    // kind retains Windows' normal frame/shadow and is never made topmost.
    let preference = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const preference).cast(),
            size_of_val(&preference) as u32,
        )
    };
}

/// After a preference edit or Undo, route only the changed connection states to the host.
pub fn sync_connections(app: &mut App) {
    use mypaper_widgets::plugin_manager::{Preferences, Request, send};
    let Ok(next) = serde_json::from_value::<Preferences>(
        app.docs.doc(crate::docs::DocId::Plugins).effective.clone(),
    ) else {
        return;
    };
    if !app.fixture {
        for id in next
            .disabled
            .symmetric_difference(&app.plugins.preferences.disabled)
        {
            let request = if next.disabled.contains(id) {
                Request::Disable(id.clone())
            } else {
                Request::Enable(id.clone())
            };
            if let Err(error) = send(&request) {
                log::warn!("connection update: {error:#}");
            }
        }
    }
    app.plugins.preferences = next;
}

/// UIA uses the same validation, persistence and undo routes as pointer and keyboard input.
pub fn accessibility(
    app: &mut App,
    hit: mypaper_ui::HitId,
    action: mypaper_ui::AccessibilityAction,
) {
    use mypaper_ui::{AccessibilityAction as A, controls::Behavior};
    let Some(target) = hit
        .checked_sub(1)
        .and_then(|id| app.frame.targets.get(id as usize))
        .cloned()
    else {
        return;
    };
    if colors::accessibility(app, &target, &action) { return; }
    match (&target.behavior, &target.action, action) {
        (
            Behavior::Slider {
                min,
                max,
                step,
                value,
                ..
            },
            Action::Slider(edit),
            action,
        ) => {
            let value = match action {
                A::SetNumber(value) => value,
                A::Increment => value + step,
                A::Decrement => value - step,
                _ => return,
            };
            if !value.is_finite() {
                return;
            }
            let value = if *step > 0.0 {
                ((value - min) / step).round() * step + min
            } else {
                value
            };
            let value = value.clamp(*min, *max);
            let value = if value.fract() == 0.0 {
                serde_json::json!(value as i64)
            } else {
                serde_json::json!(value)
            };
            app.set(edit, value);
        }
        (Behavior::Field, Action::Field(id), A::SetText(text)) => {
            app.focus_field(id.clone());
            if let Some(field) = &app.field
                && field.field.set_text(&text).is_ok()
            {
                app.event(mypaper_ui::Event::TextChanged);
                app.blur_field();
            }
        }
        (_, _, A::Custom(direction)) if matches!(direction, 1 | 2) && app.panel.is_some() => {
            app.input.focus = Some(target.key);
            lists::keyboard(app, if direction == 1 { -1 } else { 1 });
        }
        _ => {}
    }
    app.dirty = true;
}

pub fn remember_theme_edit(app: &mut App, path: &str, value: Option<&serde_json::Value>) {
    if let Some(state) = &mut app.panel {
        let edit = crate::actions::Edit::new(crate::docs::DocId::Studio, path, "Wallpaper");
        state.theme_retry = Some(value.map_or_else(
            || Action::Unset(edit.clone()),
            |value| Action::Set(edit.clone(), value.clone()),
        ));
    }
}

#[cfg(test)]
mod state_review;
