//! Deterministic standalone window renders, compared with `tests/golden/window`.
//! Set `MYPAPER_BLESS=1` to update baselines; review copies go to the window shots folder.

use std::path::PathBuf;

use mypaper_ui::{Ui, golden, render};

use crate::actions::Action;
use crate::app::App;
use crate::docs::Docs;
use crate::library::Library;
use crate::theme::Theme;

const CONFIG: &str = r##"{ "theme": "rain-city", "engine": { "gpu": "integrated", "fps": 30 } }"##;
const BARS: &str = r##"{ "version": 1, "preset": "macos", "taskbar": { "mode": "hide" },
  "dock": { "enabled": true, "pinned": [
    { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" },
    { "path": "C:\\Program Files\\Browser\\browser.exe", "name": "Browser" },
    { "path": "C:\\Program Files\\Mail\\mail.exe", "name": "Mail" },
    { "path": "C:\\Program Files\\Code\\code.exe", "name": "Code" },
    { "path": "C:\\Windows\\System32\\wt.exe", "name": "Terminal" },
    { "path": "C:\\Program Files\\Music\\music.exe", "name": "Music" },
    { "path": "C:\\Program Files\\Photos\\photos.exe", "name": "Photos" }
  ] } }"##;
const WIDGETS: &str = r##"{ "version": 1, "instances": [
  { "instance": "clock-1", "widget": "clock", "monitor": "\\\\.\\DISPLAY1", "cell": [0, 0], "size": [3, 2] },
  { "instance": "weather-1", "widget": "weather", "monitor": "\\\\.\\DISPLAY1", "cell": [0, 2], "size": [3, 2] },
  { "instance": "now-playing-1", "widget": "now-playing", "monitor": "\\\\.\\DISPLAY1", "cell": [0, 4], "size": [3, 1] },
  { "instance": "calendar-1", "widget": "calendar", "monitor": "\\\\.\\DISPLAY1", "cell": [20, 0], "size": [3, 3] },
  { "instance": "system-monitor-1", "widget": "system-monitor", "monitor": "\\\\.\\DISPLAY1", "cell": [20, 3], "size": [3, 2] },
  { "instance": "todo-1", "widget": "todo", "monitor": "\\\\.\\DISPLAY1", "cell": [20, 5], "size": [3, 2] }
] }"##;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn app(dark: bool, ui: &Ui) -> App {
    // Isolated Cargo targets are not beside the bundled themes. Resolve the fixture's
    // active wallpaper explicitly, including after edits call Library::set_active.
    let active = repo().join("themes/rain-city").display().to_string();
    let mut config: serde_json::Value = serde_json::from_str(CONFIG).unwrap();
    config["theme"] = serde_json::json!(active);
    let docs = Docs::fixture([&config.to_string(), BARS, WIDGETS, "{}"]);
    let library = Library::scan_in(&[repo().join("themes")], None, Some(&active));
    App::fixture(docs, Theme::fixed(dark, &ui.text), library, (1280, 800))
}

/// Shared fixture pages inside the real standalone frame.
pub fn window_app(dark: bool, ui: &Ui, size: (u32, u32)) -> App {
    let mut app = app(dark, ui);
    app.panel = Some(crate::panel::State::default());
    app.panel.as_mut().unwrap().windowed = true;
    app.size = size;
    if app.library.active().is_none() {
        if let Some(item) = app
            .library
            .items
            .iter_mut()
            .find(|item| item.id == "rain-city")
        {
            item.active = true;
        }
    }
    crate::editing::plugins::fixture(&mut app.plugins);
    app
}

pub fn shot(app: &mut App, ui: &mut Ui) -> mypaper_ui::Image {
    let built = crate::view::build(app, ui);
    let image = render(ui, &built.list, app.size.0, app.size.1).unwrap();
    app.frame.targets = built.targets;
    app.frame.scrolls = built.scrolls;
    app.frame.regions = built.regions;
    app.frame.overlay_from = built.overlay_from;
    image
}

#[test]
fn keyboard_reaches_window_pages_search_and_history() {
    use crate::panel::Page;
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    for (vk, page) in [
        (0x30, Page::Root),
        (0x31, Page::Wallpapers),
        (0x32, Page::Look),
        (0x33, Page::Bars),
        (0x34, Page::Widgets),
        (0x35, Page::DesktopMenu),
        (0x36, Page::Windows),
        (0x37, Page::Connections),
        (0xBC, Page::Settings),
        (0x48, Page::History),
    ] {
        app.key_with(vk, (true, false, false));
        shot(&mut app, &mut ui);
        assert_eq!(app.panel.as_ref().unwrap().page, page);
    }
    app.key_with(0x4B, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Search);
    app.key_with(0x1B, (false, false, false));
    assert!(!app.quit);
    // F6 reaches sidebar, title tools and editor; Tab returns with Shift+Tab.
    shot(&mut app, &mut ui);
    let mut visited = std::collections::HashSet::new();
    for _ in 0..app.frame.regions.len() {
        app.key_with(0x75, (false, false, false));
        let rect = app
            .frame
            .targets
            .iter()
            .find(|t| Some(t.key) == app.input.focus)
            .unwrap()
            .rect;
        let center = mypaper_ui::Point::new(
            (rect.left + rect.right) / 2.0,
            (rect.top + rect.bottom) / 2.0,
        );
        visited.insert(
            app.frame
                .regions
                .iter()
                .position(|r| r.contains(center))
                .unwrap(),
        );
    }
    assert_eq!(visited.len(), app.frame.regions.len());
    let before = app.input.focus;
    app.key_with(0x09, (false, false, false));
    app.key_with(0x09, (false, true, false));
    assert_eq!(app.input.focus, before);
}

#[test]
fn render_window_pages() {
    use crate::panel::{Command, Page};
    let out = std::env::var_os("MYPAPER_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\mypaper-targets\studio-window\shots"));
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for size in [(1280, 800), (1920, 1080)] {
        for dark in [true, false] {
            for (page, name) in [
                (Page::Root, "home"),
                (Page::Wallpapers, "wallpapers"),
                (Page::Look, "look"),
                (Page::Bars, "bars"),
                (Page::Widget, "widget"),
                (Page::Settings, "settings"),
                (Page::History, "history"),
                (Page::FirstRun, "first-run"),
                (Page::Look, "off"),
            ] {
                let mut app = window_app(dark, &ui, size);
                app.run(Action::Panel(Command::Open(page)));
                if page == Page::Widget {
                    app.nav.widget = Some("clock-1".into());
                }
                if page == Page::History {
                    app.run(Action::SetStyle(Some(mypaper_config::look::Style::Fluent)));
                    app.run(Action::Preset("islands"));
                    for (i, entry) in app
                        .panel
                        .as_ref()
                        .unwrap()
                        .history
                        .borrow_mut()
                        .entries
                        .iter_mut()
                        .enumerate()
                    {
                        entry.at = 1793958138 + i as u64 * 60;
                    }
                    app.toast = None;
                }
                if name == "off" {
                    app.system
                        .parts
                        .iter_mut()
                        .for_each(|part| part.running = false);
                }
                let image = shot(&mut app, &mut ui);
                let file = format!(
                    "{name}-{}x{}-{}.png",
                    size.0,
                    size.1,
                    if dark { "dark" } else { "light" }
                );
                golden::write(&image, &out.join(&file)).unwrap();
                golden::check(&image, &baseline.join(&file)).unwrap();
            }
        }
    }
}

#[test]
fn window_layout_keeps_the_stage_inspector_band_and_footer_fixed() {
    use crate::panel::{Command, Page};
    let mut ui = Ui::new().unwrap();
    for (size, rail, inspector_width) in [
        ((960, 640), true, 340.0),
        ((1280, 800), false, 340.0),
        ((1920, 1080), false, 380.0),
    ] {
        let mut app = window_app(true, &ui, size);
        app.run(Action::Panel(Command::Open(Page::AddWidget)));
        let built = crate::view::build(&app, &mut ui);
        let interactive: Vec<_> = built
            .list
            .hits
            .iter()
            .filter(|hit| hit.id < u32::MAX / 2)
            .map(|hit| hit.id)
            .collect();
        assert_eq!(
            interactive,
            (1..=built.targets.len() as u32).collect::<Vec<_>>()
        );
        let r = crate::view::regions(&app);
        let (column, live) = crate::view::content_regions(&app);
        assert_eq!(r.rail, rail);
        assert!(live.is_some());
        assert_eq!(column.width(), inspector_width);
        assert_eq!(r.bar.height(), 48.0);
        assert_eq!(r.sidebar.width(), if rail { 56.0 } else { 208.0 });
        let cancel = built
            .targets
            .iter()
            .rev()
            .find(|t| t.action == Action::Panel(Command::Open(Page::Widgets)))
            .unwrap();
        assert!(cancel.rect.left >= column.left);
        assert_eq!(cancel.rect.bottom, r.body.bottom - 16.0);
        assert!(
            !built
                .targets
                .iter()
                .any(|t| t.action == Action::Panel(Command::Done))
        );
    }
}

#[test]
fn stage_selection_and_zoom_only_change_navigation_and_keep_the_inspector_in_place() {
    use crate::{
        actions::Bar,
        panel::{Command, Page},
    };
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    let before = app.docs.doc(crate::docs::DocId::Widgets).raw.clone();
    let inspector = crate::view::regions(&app).inspector.unwrap();
    let built = crate::view::build(&app, &mut ui);
    let clock = built
        .targets
        .iter()
        .find(|target| target.action == Action::SelectWidget(Some("clock-1".into())))
        .unwrap()
        .action
        .clone();
    app.run(clock);
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Widget);
    app.run(Action::Panel(Command::StageZoom(true)));
    let built = crate::view::build(&app, &mut ui);
    assert_eq!(crate::view::regions(&app).inspector.unwrap(), inspector);
    let stage = crate::view::regions(&app).stage.unwrap();
    for target in built
        .targets
        .iter()
        .filter(|target| target.scroll == Some(mypaper_ui::controls::key("window-stage")))
    {
        if let Some(visible) = target.visible {
            assert!(
                visible.left >= stage.left
                    && visible.top >= stage.top
                    && visible.right <= stage.right
                    && visible.bottom <= stage.bottom
            );
        }
    }
    app.run(Action::Panel(Command::Open(Page::Top)));
    assert_eq!(app.nav.bar, Some(Bar::Top));
    assert!(app.panel.as_ref().unwrap().stage_actual);
    let built = crate::view::build(&app, &mut ui);
    let slot = crate::actions::Slot {
        bar: Bar::Top,
        list: "right",
        index: 6,
    };
    let module = built
        .targets
        .iter()
        .find(|target| target.action == Action::SelectModule(Some(slot.clone())))
        .unwrap()
        .action
        .clone();
    app.run(module);
    assert_eq!(crate::panel::current_page(&app), Page::Module);
    assert_eq!(
        crate::panel::editors::selected_module_name(&app).as_deref(),
        Some("Date and clock")
    );
    assert_eq!(crate::view::regions(&app).inspector.unwrap(), inspector);
    assert_eq!(app.docs.doc(crate::docs::DocId::Widgets).raw, before);
    assert!(!app.docs.can_undo());
    for page in [
        Page::Connections,
        Page::Settings,
        Page::General,
        Page::Performance,
        Page::Keyboard,
        Page::Troubleshoot,
        Page::About,
        Page::History,
        Page::Search,
        Page::FirstRun,
    ] {
        app.run(Action::Panel(Command::Open(page)));
        let r = crate::view::regions(&app);
        assert!(r.stage.is_none() && r.inspector.is_none());
        assert!(crate::view::content_regions(&app).0.width() <= 672.0);
    }
}

/// Additional frame checks for the rail, native-size selection, and creation/restore footers.
#[test]
fn render_window_interaction_states() {
    use crate::panel::{Command, Page};
    let out = PathBuf::from(
        std::env::var_os("MYPAPER_STUDIO_WINDOW_SHOTS")
            .unwrap_or_else(|| r"C:\mypaper-targets\studio-window\shots".into()),
    );
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (page, name, size, actual) in [
            (Page::Widget, "widget-100", (1280, 800), true),
            (Page::Module, "module-100", (1280, 800), true),
            (Page::Windows, "windows", (1280, 800), false),
            (Page::General, "large-text", (1280, 800), false),
            (Page::DesktopMenu, "desktop-menu", (1280, 800), false),
            (Page::AddWidget, "add-widget-rail", (960, 640), false),
            (Page::History, "history-restore", (1280, 800), false),
        ] {
            let mut app = window_app(dark, &ui, size);
            app.run(Action::Panel(Command::Open(page)));
            app.panel.as_mut().unwrap().stage_actual = actual;
            app.nav.widget = Some("clock-1".into());
            if name == "large-text" {
                app.theme.ty.text_scale = 1.5;
            }
            if page == Page::Module {
                app.nav.bar = Some(crate::actions::Bar::Top);
                app.nav.module = Some(crate::actions::Slot {
                    bar: crate::actions::Bar::Top,
                    list: "right",
                    index: 6,
                });
            }
            if page == Page::History {
                app.run(Action::SetStyle(Some(mypaper_config::look::Style::Fluent)));
                app.run(Action::Preset("islands"));
                app.run(Action::Panel(Command::History(1)));
                for (i, entry) in app
                    .panel
                    .as_ref()
                    .unwrap()
                    .history
                    .borrow_mut()
                    .entries
                    .iter_mut()
                    .enumerate()
                {
                    entry.at = 1793958138 + i as u64 * 60;
                }
                app.toast = None;
            }
            let image = shot(&mut app, &mut ui);
            let file = format!(
                "{name}-{}x{}-{}.png",
                size.0,
                size.1,
                if dark { "dark" } else { "light" }
            );
            golden::write(&image, &out.join(&file)).unwrap();
            golden::check(&image, &baseline.join(&file)).unwrap();
        }
    }
}
