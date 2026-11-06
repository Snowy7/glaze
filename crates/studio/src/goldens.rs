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
fn sidebar_preferences_survive_reopening_and_narrow_windows_do_not_change_them() {
    use crate::{
        docs::DocId,
        panel::{Command, Page},
    };
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    let unchanged =
        [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| app.docs.doc(id).raw.clone());
    app.key_with(0x42, (true, false, false));
    assert_eq!(crate::view::regions(&app).sidebar.width(), 56.0);
    app.run(Action::Panel(Command::Open(Page::Look)));
    let built = crate::view::build(&app, &mut ui);
    assert!(
        built
            .list
            .hits
            .iter()
            .any(|hit| hit.label.as_ref() == "Look" && hit.checked == Some(true))
    );
    app.input.hover = Some(mypaper_ui::controls::key(("nav-page", Page::Look)));
    let built = crate::view::build(&app, &mut ui);
    assert!(
        built
            .list
            .ops
            .iter()
            .any(|op| matches!(op, mypaper_ui::Op::Text { text, .. } if text.as_ref() == "Look"))
    );
    // The existing close path replaces the placement object, including in older readers.
    app.apply("Window", vec![crate::docs::Op {
        doc: DocId::Studio,
        path: "/window".into(),
        value: Some(serde_json::json!({"x":0,"y":0,"width":1280,"height":800,"maximized":false})),
    }]);
    let studio = app.docs.doc(DocId::Studio).raw.to_string();
    let documents =
        [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| app.docs.doc(id).raw.to_string());
    let mut reopened = window_app(true, &ui, (1280, 800));
    reopened.docs = Docs::fixture([&documents[0], &documents[1], &documents[2], &studio]);
    assert_eq!(crate::view::regions(&reopened).sidebar.width(), 56.0);
    reopened.key_with(0x42, (true, false, false));
    assert_eq!(crate::view::regions(&reopened).sidebar.width(), 208.0);
    reopened.size = (960, 640);
    assert_eq!(crate::view::regions(&reopened).sidebar.width(), 56.0);
    reopened.size = (1280, 800);
    assert_eq!(crate::view::regions(&reopened).sidebar.width(), 208.0);
    assert_eq!(
        unchanged,
        [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| reopened.docs.doc(id).raw.clone())
    );
    assert!(
        !reopened.docs.can_undo(),
        "window preferences are outside desktop undo"
    );
}

#[test]
fn inspector_drag_is_bounded_remembered_and_does_not_edit_the_desktop() {
    use mypaper_ui::{Button, Event, Point, Pointer};
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    let before = [
        crate::docs::DocId::Config,
        crate::docs::DocId::Bars,
        crate::docs::DocId::Widgets,
    ]
    .map(|id| app.docs.doc(id).raw.clone());
    shot(&mut app, &mut ui);
    let left = crate::view::regions(&app).inspector.unwrap().left;
    let pointer = |x| Pointer {
        at: Point::new(x, 200.0),
        screen: (0, 0),
    };
    app.event(Event::Press(pointer(left), Button::Left));
    app.event(Event::Move(pointer(left - 80.0)));
    assert_eq!(crate::view::inspector_width(&app), 420.0);
    app.event(Event::Release(pointer(left - 80.0), Button::Left));
    assert_eq!(
        app.get(crate::docs::DocId::Studio, crate::view::INSPECTOR_SETTING)
            .and_then(serde_json::Value::as_f64),
        Some(420.0)
    );
    app.panel.as_mut().unwrap().window = Default::default();
    assert_eq!(crate::view::inspector_width(&app), 420.0);
    app.size = (960, 640);
    assert_eq!(
        crate::view::inspector_width(&app),
        520.0_f32.min(960.0 - 56.0 - 360.0).min(420.0)
    );
    assert_eq!(
        before,
        [
            crate::docs::DocId::Config,
            crate::docs::DocId::Bars,
            crate::docs::DocId::Widgets
        ]
        .map(|id| app.docs.doc(id).raw.clone())
    );
    assert!(!app.docs.can_undo());
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
            for collapsed in [false, true] {
                for (page, name) in [
                    (Page::Root, "home"),
                    (Page::Wallpapers, "wallpapers"),
                    (Page::Look, "look"),
                    (Page::Windows, "windows"),
                    (Page::Bars, "bars"),
                    (Page::Widget, "widget"),
                    (Page::Settings, "settings"),
                    (Page::History, "history"),
                    (Page::FirstRun, "first-run"),
                    (Page::Look, "off"),
                ] {
                    let mut app = window_app(dark, &ui, size);
                    if collapsed {
                        crate::view::toggle_sidebar(&mut app);
                        app.toast = None;
                    }
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
                        "{name}{}-{}x{}-{}.png",
                        if collapsed { "-collapsed" } else { "" },
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
}

/// Actual card-size pictures in both desktop schemes, without the surrounding inspector.
#[test]
fn render_material_signatures() {
    use crate::panel::{controls, specimens};
    use mypaper_config::look::Style;
    use mypaper_ui::{
        Rect,
        controls::{Align, Builder},
    };
    let out = std::env::var_os("MYPAPER_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\mypaper-targets\studio-window\shots"));
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        let mut app = window_app(dark, &ui, (1280, 800));
        app.run(Action::Set(
            crate::actions::Edit::new(crate::docs::DocId::Config, "/look/appearance", "Appearance"),
            serde_json::json!(if dark { "dark" } else { "light" }),
        ));
        let mut b = Builder::new(
            &mut ui.text,
            &app.theme.palette,
            &app.theme.ty,
            1.0,
            &app.input,
        );
        b.fill(Rect::new(0.0, 0.0, 750.0, 280.0), 0.0, b.p.bg);
        for (row, names) in [
            &["Liquid", "One UI", "Solid", "Fluent", "Mono"][..],
            &["As is", "Match mypaper", "Glass", "Sharp"][..],
        ]
        .into_iter()
        .enumerate()
        {
            for (index, name) in names.iter().enumerate() {
                let card = Rect::new(
                    10.0 + index as f32 * 150.0,
                    10.0 + row as f32 * 140.0,
                    130.0,
                    120.0,
                );
                controls::card(&mut b, card, false);
                controls::card_picture(
                    &mut b,
                    card,
                    Rect::new(card.left, card.top, 130.0, 82.0),
                    |b, rect| {
                        if row == 0 {
                            specimens::material(&app, b, rect, Style::ALL[index]);
                        } else {
                            specimens::window(&app, b, rect, index);
                        }
                    },
                );
                let ink = b.p.ink1;
                controls::text(
                    &mut b,
                    Rect::new(card.left + 8.0, card.top + 86.0, 114.0, 28.0),
                    name,
                    12.0,
                    500,
                    ink,
                    Align::Start,
                );
            }
        }
        let list = b.list;
        let image = render(&mut ui, &list, 750, 280).unwrap();
        let file = format!(
            "material-signatures-{}.png",
            if dark { "dark" } else { "light" }
        );
        golden::write(&image, &out.join(&file)).unwrap();
        golden::check(&image, &baseline.join(&file)).unwrap();
    }
}

#[test]
fn look_preview_segments_switch_pages_without_editing_documents() {
    use crate::panel::{Command, Page};
    use mypaper_ui::controls::key;
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    let before = crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone());
    for (page, selected, destination) in [
        (Page::Look, 0usize, Page::Windows),
        (Page::Windows, 1usize, Page::Look),
    ] {
        app.run(Action::Panel(Command::Open(page)));
        let built = crate::view::build(&app, &mut ui);
        let group = key("look-stage-scope");
        let selected_key = key((group, selected));
        let selected_id = built
            .targets
            .iter()
            .position(|target| target.key == selected_key)
            .unwrap() as u32
            + 1;
        assert!(
            built
                .list
                .hits
                .iter()
                .any(|hit| hit.id == selected_id && hit.checked == Some(true))
        );
        let target = built
            .targets
            .iter()
            .find(|target| target.key == key((group, 1 - selected)))
            .unwrap();
        app.run(target.action.clone());
        assert_eq!(crate::panel::current_page(&app), destination);
    }
    assert_eq!(
        before,
        crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone())
    );
    assert!(!app.docs.can_undo());
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
    let viewport = crate::view::regions(&app).stage.unwrap();
    let desktop = built.targets.iter().find(|target| target.key == mypaper_ui::controls::key("stage-wallpaper")).unwrap().rect;
    let chips = built.targets.iter().filter(|target| {
        target.scroll == Some(mypaper_ui::controls::key("window-stage"))
            && matches!(target.action, Action::Panel(Command::Display(_)))
    }).map(|target| target.rect).collect::<Vec<_>>();
    assert_eq!(chips.len(), app.monitors.len());
    assert!(chips.iter().all(|rect| rect.top > desktop.bottom));
    assert!(((desktop.left + desktop.right) / 2.0 - (viewport.left + viewport.right) / 2.0).abs() <= 1.0);
    assert!(((desktop.top + chips[0].bottom) / 2.0 - (viewport.top + viewport.bottom) / 2.0).abs() <= 1.0);
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
    app.run(Action::ArrangeOnDesktop);
    assert_eq!(crate::panel::current_page(&app), Page::Widgets);
    assert!(!app.docs.can_undo());
    app.texts
        .insert(crate::actions::FieldId::Palette, "Arrange widgets".into());
    assert!(
        !crate::search::results(&app)
            .iter()
            .any(|entry| entry.action == Action::ArrangeOnDesktop)
    );
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
            (Page::General, "large-text", (1280, 800), false),
            (Page::DesktopMenu, "desktop-menu", (1280, 800), false),
            (Page::AddWidget, "add-widget-rail", (960, 640), false),
            (Page::AddWidget, "gallery-weather", (1280, 800), false),
            (Page::Widget, "faces", (1280, 800), false),
            (Page::BarDesign, "bar-presets", (1280, 800), false),
            (Page::History, "history-restore", (1280, 800), false),
        ] {
            let mut app = window_app(dark, &ui, size);
            app.run(Action::Panel(Command::Open(page)));
            app.panel.as_mut().unwrap().stage_actual = actual;
            app.nav.widget = Some("clock-1".into());
            if name == "gallery-weather" {
                app.texts.insert(crate::actions::FieldId::WidgetSearch, "weather".into());
            }
            if name == "faces" {
                // The world-clock review owns the native timezone factory on its own test
                // thread. Weather exercises card geometry without sharing that WinRT cache.
                app.nav.widget = Some("weather-1".into());
                app.nav.more.insert(mypaper_ui::controls::key(("panel-faces", "weather-1", "variant")));
            }
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
