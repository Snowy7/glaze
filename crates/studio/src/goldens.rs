//! Golden images of every main screen, light and dark, rendered headless through the same
//! software target the window gets, from fixed files, wallpapers and monitors. They are compared
//! with `tests/golden/*.png`; `MYPAPER_BLESS=1` writes them anew (and the README's copies in
//! `docs/media/studio/`).

use std::path::PathBuf;

use mypaper_ui::{Ui, golden, render};

use crate::actions::{Bar, Page, Section};
use crate::app::{App, Overlay};
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
    let docs = Docs::fixture([CONFIG, BARS, WIDGETS, "{}"]);
    let library = Library::scan_in(&[repo().join("themes")], None, Some("rain-city"));
    App::fixture(docs, Theme::fixed(dark, &ui.text), library, (1280, 800))
}

/// `app` as the window would show it once settled (design thumbnails take a second frame).
pub fn shot(app: &mut App, ui: &mut Ui) -> mypaper_ui::Image {
    loop {
        if let Some((frame, show)) = crate::pages::stage(app) {
            crate::stage::prepare(app, ui, (frame.width() as u32, frame.height() as u32), show);
        }
        crate::stage::prepare_designs(app, ui);
        let built = crate::view::build(app, &mut ui.text);
        if !app.stage.borrow().designs.pending(app.docs.version) {
            return render(ui, &built.list, app.size.0, app.size.1).unwrap();
        }
    }
}

/// Keyboard only: every place by its shortcut, regions by F6, controls by Tab, the palette by
/// Ctrl+K and back out by Esc, with a fresh frame after each key (as the window does).
#[test]
fn keyboard_reaches_every_place_region_and_the_palette() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(true, &ui);
    let ctrl = (true, false, false);
    let none = (false, false, false);
    let press = |app: &mut App, ui: &mut Ui, vk: u16, mods| {
        app.key_with(vk, mods);
        let built = crate::view::build(app, &mut ui.text);
        app.frame.targets = built.targets;
        app.frame.regions = built.regions;
        app.frame.overlay_from = built.overlay_from;
    };
    press(&mut app, &mut ui, 0x31, ctrl);
    for (vk, page) in [(0x32, Page::Wallpapers), (0x33, Page::Look), (0x34, Page::Widgets), (0x35, Page::Bars), (0x36, Page::Integrations), (0xBC, Page::Settings), (0x31, Page::Home)] {
        press(&mut app, &mut ui, vk, ctrl);
        assert_eq!(app.nav.page(), page);
    }
    // F6 visits each region in turn.
    let mut regions = Vec::new();
    for _ in 0..app.frame.regions.len() {
        press(&mut app, &mut ui, 0x75, none);
        let focus = app.input.focus.expect("F6 focuses a control");
        let rect = app.frame.targets.iter().find(|t| t.key == focus).unwrap().rect;
        let c = mypaper_ui::Point::new((rect.left + rect.right) / 2.0, (rect.top + rect.bottom) / 2.0);
        regions.push(app.frame.regions.iter().position(|r| r.contains(c)));
    }
    regions.dedup();
    assert_eq!(regions.len(), app.frame.regions.len(), "F6 reaches every region: {regions:?}");
    // Tab moves on, and Shift+Tab comes back.
    let start = app.input.focus;
    press(&mut app, &mut ui, 0x09, none);
    assert_ne!(app.input.focus, start);
    press(&mut app, &mut ui, 0x09, (false, true, false));
    assert_eq!(app.input.focus, start);
    // The palette opens on Ctrl+K and Esc closes it.
    press(&mut app, &mut ui, 0x4B, ctrl);
    assert!(matches!(app.overlay, Some(Overlay::Palette { .. })));
    press(&mut app, &mut ui, 0x1B, none);
    assert_eq!(app.overlay, None);
}

fn check(name: &str, dark: bool, setup: impl Fn(&mut App)) {
    let mut ui = Ui::new().unwrap();
    ui.text.set_capacity(480);
    let mut app = app(dark, &ui);
    setup(&mut app);
    let image = shot(&mut app, &mut ui);
    let file = format!("{name}-{}.png", if dark { "dark" } else { "light" });
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(&file);
    golden::check(&image, &path).unwrap();
    if std::env::var_os("MYPAPER_BLESS").is_some() {
        let media = repo().join("docs/media/studio").join(&file);
        golden::check(&image, &media).unwrap();
    }
}

macro_rules! screens {
    ($($name:ident: $setup:expr;)*) => {$(
        #[test]
        fn $name() {
            check(stringify!($name), true, $setup);
            check(stringify!($name), false, $setup);
        }
    )*};
}

screens! {
    plugins: |app: &mut App| {
        app.nav.page = Some(Page::Integrations);
        app.nav.plugins = true;
        crate::pages::plugins::fixture(&mut app.plugins);
    };
    plugins_consent: |app: &mut App| crate::pages::plugins::consent_fixture(app);
    home: |app: &mut App| app.nav.page = Some(Page::Home);
    wallpapers: |app: &mut App| {
        app.nav.page = Some(Page::Wallpapers);
        app.nav.wallpaper = app.library.items.iter().position(|w| w.id == "rain-city-amber");
    };
    look: |app: &mut App| app.nav.page = Some(Page::Look);
    look_windows: |app: &mut App| {
        app.nav.page = Some(Page::Look);
        app.nav.look_windows = true;
    };
    widgets: |app: &mut App| {
        app.nav.page = Some(Page::Widgets);
        app.nav.widget = Some("weather-1".into());
    };
    bars: |app: &mut App| {
        app.nav.page = Some(Page::Bars);
        app.nav.bar = Some(Bar::Top);
    };
    bars_overview: |app: &mut App| app.nav.page = Some(Page::Bars);
    integrations: |app: &mut App| app.nav.page = Some(Page::Integrations);
    settings: |app: &mut App| {
        app.nav.page = Some(Page::Settings);
        app.nav.section = Some(Section::Performance);
    };
    troubleshoot: |app: &mut App| {
        app.nav.page = Some(Page::Settings);
        app.nav.section = Some(Section::Troubleshoot);
    };
    palette: |app: &mut App| {
        app.texts.insert(crate::actions::FieldId::Palette, "dark".into());
        app.overlay = Some(Overlay::Palette { selected: 0 });
    };
    add_wallpaper: |app: &mut App| {
        app.nav.page = Some(Page::Wallpapers);
        app.overlay = Some(Overlay::AddWallpaper { source: None, picked: None });
    };
    first_run: |app: &mut App| app.overlay = Some(Overlay::FirstRun { step: 0, starter: 0, taskbar: 0 });
}

