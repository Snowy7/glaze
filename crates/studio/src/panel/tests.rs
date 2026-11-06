use std::path::PathBuf;

use mypaper_ui::{Ui, golden, render};
use serde_json::json;

use super::{Command, Page, State};
use crate::actions::Action;
use crate::app::App;
use crate::docs::DocId;

fn app(ui: &Ui, dark: bool) -> App {
    let mut app = crate::goldens::app(dark, ui);
    app.panel = Some(State::default());
    crate::pages::plugins::fixture(&mut app.plugins);
    app.size = (480, 760);
    // Bare IDs resolve beside the executable. An isolated target isn't beside the repo's
    // themes; keep the fixture's bundled wallpaper active without touching any files.
    if app.library.active().is_none() && !app.library.items.is_empty() {
        let i = app
            .library
            .items
            .iter()
            .position(|w| w.id == "rain-city")
            .unwrap_or(0);
        app.library.items[i].active = true;
    }
    app
}

/// Review artifacts only: does not bless or replace the legacy Studio's golden baselines.
#[test]
fn render_panel_pages() {
    let out = std::env::var_os("MYPAPER_STUDIO_PANEL_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\mypaper-targets\studio-panel\shots"));
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (page, name) in [
            (Page::Root, "root"),
            (Page::Widgets, "widgets"),
            (Page::Bars, "bars"),
            (Page::Performance, "performance"),
            (Page::Top, "top-bar"),
            (Page::Dock, "dock"),
            (Page::Widget, "weather"),
            (Page::Widget, "clock"),
            (Page::Widget, "photos"),
            (Page::AddWidget, "add-widget"),
            (Page::Look, "look"),
            (Page::Look, "look-more"),
            (Page::Look, "wallpaper-look"),
            (Page::Windows, "windows"),
            (Page::Windows, "windows-more"),
            (Page::Wallpapers, "wallpapers"),
            (Page::Wallpapers, "wallpapers-broken"),
            (Page::Wallpaper, "wallpaper"),
            (Page::Wallpaper, "wallpaper-broken"),
            (Page::AddWallpaper, "add-wallpaper"),
            (Page::AddWallpaper, "add-wallpaper-app"),
            (Page::Connections, "connections"),
            (Page::Connection, "connection"),
            (Page::Connection, "connection-reconnect"),
            (Page::Settings, "settings"),
            (Page::General, "general"),
            (Page::Keyboard, "keyboard"),
            (Page::Troubleshoot, "troubleshoot"),
            (Page::About, "about"),
            (Page::DesktopMenu, "desktop-menu"),
            (Page::History, "history"),
            (Page::Search, "search"),
        ] {
            let mut app = app(&ui, dark);
            app.panel.as_mut().unwrap().page = page;
            match name {
                "weather" => app.nav.widget = Some("weather-1".into()),
                "clock" => app.nav.widget = Some("clock-1".into()),
                "photos" => {
                    let mut layout = crate::pages::widgets::layout(&app);
                    let mut photo = layout.instances[0].clone();
                    photo.widget = "photo".into(); photo.instance = "photo-1".into(); photo.size = [6, 4];
                    for (n, item) in app.library.items.iter().filter(|w| w.preview.is_some()).take(3).enumerate() {
                        photo.settings.insert(format!("image{}", n + 1), json!(item.preview.as_ref().unwrap()));
                    }
                    layout.instances.push(photo);
                    app.docs.edit("Fixture", &[crate::docs::Op { doc: DocId::Widgets, path: String::new(), value: Some(serde_json::to_value(layout).unwrap()) }]).unwrap();
                    app.nav.widget = Some("photo-1".into());
                }
                "look-more" | "windows-more" => { app.nav.more.insert(mypaper_ui::controls::key(("panel-more", mypaper_ui::controls::key(("studio-panel", page))))); }
                "wallpaper-look" => app.nav.look_theme = true,
                "wallpaper" => {
                    app.nav.wallpaper = app.library.items.iter().position(|w| w.kind.is_some_and(crate::library::Kind::uses_fit));
                }
                "wallpapers-broken" | "wallpaper-broken" => {
                    let mut wallpaper = app.library.items[0].clone();
                    wallpaper.id = "missing".into(); wallpaper.name = "beach-loop.mp4".into(); wallpaper.active = false;
                    wallpaper.error = Some("The file was moved or is no longer available.".into());
                    wallpaper.origin = crate::library::Origin::User;
                    app.library.items.insert(0, wallpaper); app.nav.filter = 1; app.nav.wallpaper = Some(0);
                }
                "add-wallpaper" => app.overlay = Some(crate::app::Overlay::AddWallpaper { source: None, picked: None }),
                "add-wallpaper-app" => {
                    app.overlay = Some(crate::app::Overlay::AddWallpaper { source: Some(crate::actions::Source::App), picked: Some(r"C:\Apps\visualizer.exe".into()) });
                    app.texts.insert(crate::actions::FieldId::AddName, "Visualizer".into());
                    app.texts.insert(crate::actions::FieldId::AddArgs, "--embed {hwnd}".into());
                }
                "connection" => app.nav.integration = crate::pages::integrations::LIST.len(),
                "connection-reconnect" => app.nav.integration = crate::pages::integrations::LIST.len() + 3,
                "history" => {
                    super::history::observe(&app);
                    app.run(Action::Preset("islands"));
                    super::history::observe(&app);
                    app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(0);
                }
                "search" => { app.texts.insert(crate::actions::FieldId::Palette, "wallpaper".into()); }
                _ => {}
            }
            let built = super::build(&app, &mut ui.text);
            let content = built.scrolls.last().unwrap().content;
            app.size.1 = (content + 48.0 + super::editors::footer_height(&app)).ceil().clamp(160.0, 760.0) as u32;
            let built = super::build(&app, &mut ui.text);
            let image = render(&mut ui, &built.list, 480, app.size.1).unwrap();
            let path = out.join(format!(
                "{name}-{}.png",
                if dark { "dark" } else { "light" }
            ));
            golden::write(&image, &path).unwrap();
            // Full content is an additional review artifact for long pages, so every option
            // can be visually checked without synthesizing desktop input.
            if content + 48.0 + super::editors::footer_height(&app) > 760.0 {
                app.size.1 = (content + 48.0 + super::editors::footer_height(&app)).ceil() as u32;
                let built = super::build(&app, &mut ui.text);
                let image = render(&mut ui, &built.list, 480, app.size.1).unwrap();
                golden::write(&image, &out.join(format!("{name}-full-{}.png", if dark { "dark" } else { "light" }))).unwrap();
            }
        }
    }
}

#[test]
fn panel_navigation_has_three_levels_and_never_saves_a_page_or_quits_on_escape() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let before = app.docs.doc(DocId::Studio).raw.clone();
    super::run(&mut app, Command::Open(Page::Settings));
    super::run(&mut app, Command::Open(Page::Performance));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Performance);
    for expected in [Page::Settings, Page::Root, Page::Root] {
        app.key_with(0x1B, (false, false, false));
        assert_eq!(app.panel.as_ref().unwrap().page, expected);
        assert!(!app.quit);
    }
    app.key_with(0x33, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Bars);
    app.key_with(0x34, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Widgets);
    app.key_with(0xBC, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Settings);
    app.key_with(0x4B, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Root);
    assert_eq!(app.docs.doc(DocId::Studio).raw, before);
    assert!(!app.docs.can_undo());
    app.key_with(0x57, (true, false, false));
    assert!(app.quit);
}

#[test]
fn panel_controls_use_the_existing_validated_documents_and_undo() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    app.panel.as_mut().unwrap().page = Page::Performance;
    let built = super::build(&app, &mut ui.text);
    let toggle = built.targets.iter().find(|t| matches!(&t.action, Action::Toggle(edit, _) if edit.path.as_ref() == "/pause_on_battery")).unwrap().action.clone();
    let before = app.get(DocId::Config, "/pause_on_battery").cloned();
    app.run(toggle);
    assert_eq!(
        app.get(DocId::Config, "/pause_on_battery"),
        Some(&json!(false))
    );
    app.run(Action::Undo);
    assert_eq!(app.get(DocId::Config, "/pause_on_battery").cloned(), before);
    app.run(Action::Redo);
    assert_eq!(
        app.get(DocId::Config, "/pause_on_battery"),
        Some(&json!(false))
    );
    let gpu = built.targets.iter().find(|t| matches!(&t.action, Action::Set(edit, value) if edit.path.as_ref() == "/engine/gpu" && value == "auto")).unwrap().action.clone();
    app.run(gpu);
    assert_eq!(app.get(DocId::Config, "/engine/gpu"), Some(&json!("auto")));
    app.panel.as_mut().unwrap().page = Page::Bars;
    let built = super::build(&app, &mut ui.text);
    let preset = built
        .targets
        .iter()
        .find(|t| t.action == Action::Preset("islands"))
        .unwrap()
        .action
        .clone();
    app.run(preset);
    assert_eq!(app.get(DocId::Bars, "/preset"), Some(&json!("islands")));
    assert!(
        DocId::ALL.iter().all(|&id| app.docs.doc(id).path.is_none()),
        "only memory fixtures are edited"
    );
}

#[test]
fn panel_controls_stay_inside_laptop_and_scaled_widths() {
    let mut ui = Ui::new().unwrap();
    for (width, scale, text_scale) in [
        (480, 1.0, 1.0),
        (360, 1.0, 1.0),
        (720, 1.5, 1.0),
        (360, 1.0, 1.5),
    ] {
        let mut app = app(&ui, true);
        app.size = (width, 760);
        app.scale = scale;
        app.theme.ty.text_scale = text_scale;
        for page in [
            Page::Root,
            Page::Widgets,
            Page::Bars,
            Page::Settings,
            Page::Performance,
        ] {
            app.panel.as_mut().unwrap().page = page;
            let built = super::build(&app, &mut ui.text);
            if page == Page::Performance && text_scale == 1.0 {
                for label in ["On battery", "Like Windows", "Always", "Never"] {
                    assert!(built.list.ops.iter().any(|op| matches!(op, mypaper_ui::Op::Text { text, .. } if text.as_ref() == label)), "{label} is readable at {width}/{scale}");
                }
            }
            for target in built.targets {
                assert!(
                    target.rect.left >= 0.0 && target.rect.right <= width as f32,
                    "{page:?} at {width}/{scale}/{text_scale}: {:?}",
                    target.rect
                );
            }
        }
    }
}

#[test]
fn placement_uses_work_area_and_display_dpi() {
    let screen = mypaper_desktop::Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let work = mypaper_desktop::Rect {
        x: 0,
        y: 32,
        width: 1920,
        height: 1048,
    };
    assert_eq!(super::bounds(screen, work, 1.0), (1432, 40, 480, 944));
    // 1920 physical px at 150% is a 1280-DIP laptop display: 360 DIPs, not 480.
    assert_eq!(super::bounds(screen, work, 1.5), (1368, 60, 540, 876));
}
