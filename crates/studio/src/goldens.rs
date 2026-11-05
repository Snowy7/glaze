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

/// One frame of `app`, as the window would show it.
pub fn shot(app: &mut App, ui: &mut Ui) -> mypaper_ui::Image {
    if let Some((frame, show)) = crate::pages::stage(app) {
        crate::stage::prepare(app, ui, (frame.width() as u32, frame.height() as u32), show);
    }
    let built = crate::view::build(app, &mut ui.text);
    render(ui, &built.list, app.size.0, app.size.1).unwrap()
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

