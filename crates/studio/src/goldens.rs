//! Deterministic standalone window renders, compared with `tests/golden/window`.
//! Set `GLAZE_BLESS=1` to update baselines; review copies go to the window shots folder.

use std::path::PathBuf;

use glaze_ui::{Ui, golden, render};

use crate::actions::Action;
use crate::app::App;
use crate::docs::Docs;
use crate::library::Library;
use crate::theme::Theme;

pub(crate) fn check(image: &glaze_ui::Image, path: &std::path::Path) -> anyhow::Result<()> {
    // Keep Studio's documented blessing switch after the kit moved to its own repository.
    if std::env::var_os("GLAZE_BLESS").is_some() {
        golden::write(image, path)
    } else {
        golden::check(image, path)
    }
}

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

pub fn shot(app: &mut App, ui: &mut Ui) -> glaze_ui::Image {
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
        (0xBC, Page::General), // Settings opens on its first tab.
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
        let center = glaze_ui::Point::new(
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
    app.input.hover = Some(glaze_ui::controls::key(("nav-page", Page::Look)));
    let built = crate::view::build(&app, &mut ui);
    assert!(
        built
            .list
            .ops
            .iter()
            .any(|op| matches!(op, glaze_ui::Op::Text { text, .. } if text.as_ref() == "Look"))
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
    use glaze_ui::{Button, Event, Point, Pointer};
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    app.run(Action::Panel(crate::panel::Command::Open(crate::panel::Page::Windows)));
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
    let initial_width = crate::view::inspector_width(&app);
    app.event(Event::Move(pointer(left - 80.0)));
    assert_eq!(crate::view::inspector_width(&app), initial_width + 80.0);
    app.event(Event::Release(pointer(left - 80.0), Button::Left));
    assert_eq!(
        app.get(crate::docs::DocId::Studio, crate::view::INSPECTOR_SETTING)
            .and_then(serde_json::Value::as_f64),
        Some(f64::from(initial_width + 80.0))
    );
    app.panel.as_mut().unwrap().window = Default::default();
    assert_eq!(crate::view::inspector_width(&app), initial_width + 80.0);
    app.size = (960, 640);
    assert_eq!(
        crate::view::inspector_width(&app),
        (960.0_f32 - 56.0 - 360.0).min(initial_width + 80.0)
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
fn window_hover_and_card_clicks_keep_previews_transient_and_undoable() {
    use crate::panel::{Command, Page};
    use glaze_ui::{Button, Event, Point, Pointer};
    let out = std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-window\shots"));
    let mut ui = Ui::new().unwrap();
    for (page, name, action) in [
        (Page::Look, "solid", Action::SetStyle(Some(glaze_config::look::Style::Solid))),
        (Page::Look, "mono", Action::SetStyle(Some(glaze_config::look::Style::Mono))),
        (Page::BarDesign, "bar-preset", Action::Set(
            crate::actions::Edit::new(crate::docs::DocId::Studio, "/panel/bar-preset", "Bar design"),
            serde_json::json!("islands"),
        )),
    ] {
        let mut app = window_app(true, &ui, (1280, 800));
        app.run(Action::Panel(Command::Open(page)));
        let before = crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone());
        shot(&mut app, &mut ui);
        let rect = app.frame.targets.iter().find(|target| target.action == action)
            .and_then(|target| target.visible).expect("the preview card must be visible");
        let pointer = Pointer { at: Point::new(rect.left + 20.0, rect.top + 20.0), screen: (0, 0) };
        app.event(Event::Move(pointer));
        let image = shot(&mut app, &mut ui);
        assert!(crate::view::build(&app, &mut ui).semantics.iter().any(|(_, semantic)| {
            matches!(semantic, glaze_ui::Semantics::Status(message) if message.starts_with("Previewing"))
        }), "the hovered card must actually request a preview");
        golden::write(&image, &out.join(format!("headless-hover-{name}.png"))).unwrap();
        assert_eq!(before, crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone()));
        assert!(!app.docs.can_undo());
        if page == Page::BarDesign {
            app.event(Event::Press(pointer, Button::Left));
            app.event(Event::Release(pointer, Button::Left));
            assert_eq!(crate::panel::editors::bar_value(&app, "/preset"), Some(serde_json::json!("islands")));
            assert_eq!(app.docs.undo_count(), 1);
            for key in [0x5A, 0x59, 0x5A] {
                app.key_with(key, (true, false, false));
            }
            assert_eq!(before, crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone()));
        }
        app.event(Event::Leave);
        let image = shot(&mut app, &mut ui);
        golden::write(&image, &out.join(format!("headless-restored-{name}.png"))).unwrap();
    }
    let mut app = window_app(true, &ui, (960, 640));
    let image = shot(&mut app, &mut ui);
    golden::write(&image, &out.join("headless-home-rail.png")).unwrap();
}

#[test]
fn render_window_pages() {
    use crate::panel::{Command, Page};
    let out = std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-window\shots"));
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
                        app.run(Action::SetStyle(Some(glaze_config::look::Style::Fluent)));
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
                    check(&image, &baseline.join(&file)).unwrap();
                }
            }
        }
    }
}

/// Every full-screen onboarding step, on a large and a small display.
#[test]
fn render_onboarding_steps() {
    use crate::panel::{Command, Page, first_run};
    let out = std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-window\shots"));
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for size in [(1920, 1080), (1280, 800)] {
        for dark in [true, false] {
            let mut app = window_app(dark, &ui, size);
            app.run(Action::Panel(Command::Open(Page::FirstRun)));
            for (step, name) in [
                (first_run::WELCOME, "welcome"),
                (first_run::TASKBAR, "taskbar"),
                (first_run::LOOK, "look"),
                (first_run::TOP, "top-bar"),
                (first_run::DOCK, "dock"),
                (first_run::WALLPAPER, "wallpaper"),
                (first_run::EXTRAS, "extras"),
                (first_run::DONE, "done"),
            ] {
                app.overlay = Some(crate::app::Overlay::FirstRun { step, starter: None, taskbar: 0 });
                app.toast = None;
                let image = shot(&mut app, &mut ui);
                assert!(app.frame.targets.iter().any(|t| t.action == Action::FirstRunNext));
                assert_eq!(
                    app.frame.targets.iter().any(|t| t.action == Action::FirstRunSkip),
                    step != first_run::DONE
                );
                let file = format!(
                    "onboarding-{name}-{}x{}-{}.png",
                    size.0,
                    size.1,
                    if dark { "dark" } else { "light" }
                );
                golden::write(&image, &out.join(&file)).unwrap();
                // Large renders are for review only; one size keeps the repository small.
                if size == (1280, 800) {
                    check(&image, &baseline.join(&file)).unwrap();
                }
            }
            // The intro part-way through, for review: the mark is in, the words are rising.
            app.overlay = Some(crate::app::Overlay::FirstRun { step: first_run::WELCOME, starter: None, taskbar: 0 });
            app.panel.as_mut().unwrap().intro = Some(std::time::Instant::now() - std::time::Duration::from_millis(2150));
            let image = shot(&mut app, &mut ui);
            golden::write(&image, &out.join(format!("onboarding-intro-{}x{}.png", size.0, size.1))).unwrap();
            app.panel.as_mut().unwrap().intro = None;
        }
    }
}

/// Actual card-size pictures in both desktop schemes, without the surrounding inspector.
#[test]
fn render_material_signatures() {
    use crate::panel::{controls, specimens};
    use glaze_config::look::Style;
    use glaze_ui::{
        Rect,
        controls::{Align, Builder},
    };
    let out = std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-window\shots"));
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
            &["As is", "Match Glaze", "Glass", "Sharp"][..],
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
                controls::card_picture(
                    &mut b,
                    Rect::new(card.left, card.top, 130.0, 82.0),
                    false,
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
                    Rect::new(card.left, card.top + 86.0, 130.0, 28.0),
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
        check(&image, &baseline.join(&file)).unwrap();
    }
}

#[test]
fn look_preview_segments_switch_pages_without_editing_documents() {
    use crate::panel::{Command, Page};
    use glaze_ui::controls::key;
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    let before = crate::docs::DocId::ALL.map(|id| app.docs.doc(id).raw.clone());
    // Desktop and Windows are tabs under the page bar.
    for (page, destination) in [(Page::Look, Page::Windows), (Page::Windows, Page::Look)] {
        app.run(Action::Panel(Command::Open(page)));
        let built = crate::view::build(&app, &mut ui);
        let selected_key = key(("page-tab", page));
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
            .find(|target| target.key == key(("page-tab", destination)))
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
        ((1280, 800), false, 384.0),
        ((1920, 1080), false, 576.0),
    ] {
        let mut app = window_app(true, &ui, size);
        app.run(Action::Panel(Command::Open(Page::Windows)));
        let r = crate::view::regions(&app);
        let (column, live) = crate::view::content_regions(&app);
        assert_eq!(r.rail, rail);
        assert!(live.is_some());
        assert_eq!(column.width(), inspector_width);
        assert_eq!(r.bar.height(), 48.0);
        assert_eq!(r.sidebar.width(), if rail { 56.0 } else { 208.0 });
        // Adding a widget is a reading page whose footer stays pinned at the bottom.
        app.run(Action::Panel(Command::Open(Page::AddWidget)));
        let built = crate::view::build(&app, &mut ui);
        let interactive: Vec<_> = built.list.hits.iter().filter(|hit| hit.id < u32::MAX / 2).map(|hit| hit.id).collect();
        assert_eq!(interactive, (1..=built.targets.len() as u32).collect::<Vec<_>>());
        let r = crate::view::regions(&app);
        let (column, live) = crate::view::content_regions(&app);
        assert!(live.is_none());
        let cancel = built.targets.iter().rev().find(|t| t.action == Action::Panel(Command::Open(Page::Widgets))).unwrap();
        assert!(cancel.rect.left >= column.left);
        assert_eq!(cancel.rect.bottom, r.body.bottom - 16.0);
        assert!(!built.targets.iter().any(|t| t.action == Action::Panel(Command::Done)));
    }
}

#[test]
fn desktop_pages_are_reading_pages_that_offer_edit_mode() {
    use crate::panel::{Command, Page};
    let mut ui = Ui::new().unwrap();
    let mut app = window_app(true, &ui, (1280, 800));
    for page in [Page::Root, Page::Look, Page::Widgets, Page::Bars, Page::Top] {
        app.run(Action::Panel(Command::Open(page)));
        let built = crate::view::build(&app, &mut ui);
        assert!(crate::view::regions(&app).stage.is_none(), "{page:?} has no desktop picture");
        assert!(built.targets.iter().any(|t| t.action == Action::Panel(Command::EditDesktop(true))), "{page:?} offers edit mode");
    }
    app.run(Action::ArrangeOnDesktop);
    assert_eq!(crate::panel::current_page(&app), Page::Widgets);
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
        let (host, _) = crate::view::content_regions(&app);
        assert!(crate::view::column(&app, host, page).1 <= app.px(1200.0));
    }
}

/// Additional frame checks#[test]
/// Additional frame checks for the rail, native-size selection, and creation/restore footers.
#[test]
fn render_window_interaction_states() {
    use crate::panel::{Command, Page};
    let out = PathBuf::from(
        std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS")
            .unwrap_or_else(|| r"C:\glaze-targets\studio-window\shots".into()),
    );
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (page, name, size) in [
            (Page::General, "large-text", (1280, 800)),
            (Page::DesktopMenu, "desktop-menu", (1280, 800)),
            (Page::AddWidget, "add-widget-rail", (960, 640)),
            (Page::AddWidget, "gallery-weather", (1280, 800)),
            (Page::Widget, "faces", (1280, 800)),
            (Page::BarDesign, "bar-presets", (1280, 800)),
            (Page::History, "history-restore", (1280, 800)),
        ] {
            let mut app = window_app(dark, &ui, size);
            app.run(Action::Panel(Command::Open(page)));
            app.nav.widget = Some("clock-1".into());
            if name == "gallery-weather" {
                app.texts.insert(crate::actions::FieldId::WidgetSearch, "weather".into());
            }
            if name == "faces" {
                // The world-clock review owns the native timezone factory on its own test
                // thread. Weather exercises card geometry without sharing that WinRT cache.
                app.nav.widget = Some("weather-1".into());
                app.nav.more.insert(glaze_ui::controls::key(("panel-faces", "weather-1", "variant")));
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
                app.run(Action::SetStyle(Some(glaze_config::look::Style::Fluent)));
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
            check(&image, &baseline.join(&file)).unwrap();
        }
    }
}

#[test]
fn render_studio_refinements() {
    use crate::panel::{Command, Page};
    use crate::docs::{DocId, Op};
    use serde_json::json;
    let out = std::env::var_os("GLAZE_STUDIO_WINDOW_SHOTS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-window\shots"));
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (page, name) in [(Page::Windows, "wide-sidebar"), (Page::Look, "look-color"), (Page::Bars, "bars-focused"), (Page::Connections, "connections-rows"), (Page::Windows, "sidebar-resizing")] {
            let mut app = window_app(dark, &ui, (1920, 1080));
            app.run(Action::Panel(Command::Open(page)));
            app.apply("Sidebar width", vec![Op { doc: DocId::Studio, path: crate::view::INSPECTOR_SETTING.into(), value: Some(json!(680.0)) }]);
            if page == Page::Connections {
                for (id, name) in [("discord", "Discord"), ("grok", "Grok"), ("obs", "OBS Studio"), ("tailscale", "Tailscale"), ("rss", "RSS")] {
                    app.plugins.installed.push(glaze_widgets::manifest::PluginManifest::parse(&json!({"id":id,"name":name,"version":"1.0.0","component":"fixture.wasm","api":"0.1"}).to_string(), id).unwrap());
                }
            }
            if name == "look-color" {
                app.run(Action::Set(crate::actions::Edit::new(DocId::Config, "/look/accent", "Accent"), json!("#c88d3c")));
                shot(&mut app, &mut ui);
                let scroll = glaze_ui::controls::key(("studio-panel", Page::Look));
                app.nav.scroll.insert(scroll, 610.0);
            }
            if name == "sidebar-resizing" {
                shot(&mut app, &mut ui);
                app.panel.as_mut().unwrap().window.inspector_drag = Some(580.0);
            }
            app.toast = None;
            let image = shot(&mut app, &mut ui);
            let file = format!("{name}-1920x1080-{}.png", if dark { "dark" } else { "light" });
            golden::write(&image, &out.join(&file)).unwrap();
            check(&image, &baseline.join(&file)).unwrap();
        }
    }
}
