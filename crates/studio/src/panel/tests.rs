use std::path::PathBuf;

use glaze_ui::{Ui, golden, render};
use serde_json::json;

use super::{Command, Page, State};
use crate::actions::Action;
use crate::app::App;

/// The action a dropdown runs for the option `pick` accepts: a select's `Set` of that value.
fn dropdown_option(built: &super::Built, path: &str, pick: impl Fn(&serde_json::Value) -> bool) -> Action {
    built
        .targets
        .iter()
        .find_map(|t| match &t.action {
            Action::Choose(choices) if choices.edit.path.as_ref() == path => choices
                .options
                .iter()
                .find(|(value, _)| pick(value))
                .map(|(value, _)| Action::Set(choices.edit.clone(), value.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a dropdown for {path}"))
}
use crate::docs::DocId;

fn app(ui: &Ui, dark: bool) -> App {
    let mut app = crate::goldens::app(dark, ui);
    app.panel = Some(State::default());
    let mut bars = app.docs.doc(DocId::Bars).raw.clone();
    bars["dock"]["pinned"] = json!(super::icons::fixture_pins());
    let mut layout = crate::editing::widgets::layout(&app);
    layout.upgrade();
    app.docs = crate::docs::Docs::fixture([
        &app.docs.doc(DocId::Config).raw.to_string(),
        &bars.to_string(),
        &serde_json::to_string(&layout).unwrap(),
        &app.docs.doc(DocId::Studio).raw.to_string(),
    ]);
    crate::editing::plugins::fixture(&mut app.plugins);
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
    let out = std::env::var_os("GLAZE_STUDIO_PANEL_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\glaze-targets\studio-panel\shots"));
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (page, name) in [
            (Page::Root, "root"),
            (Page::Widgets, "widgets"),
            (Page::Bars, "bars"),
            (Page::Bars, "bars-display-2"),
            (Page::BarDesign, "bars-design"),
            (Page::Performance, "performance"),
            (Page::Performance, "performance-custom"),
            (Page::Top, "top-bar"),
            (Page::Top, "top-bar-designs"),
            (Page::Dock, "dock"),
            (Page::Dock, "dock-designs"),
            (Page::Widget, "weather"),
            (Page::Widget, "now-playing"),
            (Page::Widget, "system-monitor"),
            (Page::Widget, "clock"),
            (Page::Widget, "clock-world"),
            (Page::Widget, "clock-greeting"),
            (Page::Widget, "clock-own-look"),
            (Page::Widget, "photos"),
            (Page::Widget, "photos-polaroid"),
            (Page::AddWidget, "add-widget"),
            (Page::Look, "look"),
            (Page::Look, "look-more"),
            (Page::Look, "look-custom-color"),
            (Page::Look, "look-light-desktop"),
            (Page::Look, "newer-settings"),
            (Page::Look, "malformed-settings"),
            (Page::Look, "wallpaper-look"),
            (Page::Windows, "windows"),
            (Page::Windows, "windows-more"),
            (Page::Wallpapers, "wallpapers"),
            (Page::Wallpapers, "wallpapers-broken"),
            (Page::Wallpaper, "wallpaper"),
            (Page::Wallpaper, "wallpaper-own-look"),
            (Page::Wallpaper, "wallpaper-broken"),
            (Page::AddWallpaper, "add-wallpaper"),
            (Page::AddWallpaper, "add-wallpaper-app"),
            (Page::AddWallpaper, "add-wallpaper-file"),
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
            (Page::Search, "search-widgets"),
            (Page::Look, "save-failed"),
            (Page::Wallpaper, "theme-save-failed"),
            (Page::FirstRun, "first-run"),
            (Page::FirstRun, "first-run-taskbar"),
            (Page::FirstRun, "first-run-look"),
            (Page::FirstRun, "first-run-wallpaper"),
            (Page::FirstRun, "first-run-recovery"),
            (Page::Module, "module"),
            (Page::Module, "module-more"),
            (Page::Module, "module-program"),
            (Page::AddModule, "add-module"),
            (Page::Panels, "panels"),
            (Page::BarPanel, "start-menu"),
            (Page::BarPanel, "calendar-panel"),
            (Page::BarPanel, "sound-panel"),
            (Page::BarPanel, "tray-panel"),
            (Page::BarPanel, "notifications-panel"),
        ] {
            if std::env::var("GLAZE_STUDIO_PANEL_SHOT_FILTER").ok()
                .is_some_and(|filter| !filter.split(',').any(|wanted| wanted == name)) {
                continue;
            }
            let mut app = app(&ui, dark);
            app.panel.as_mut().unwrap().page = page;
            match name {
                "bars-display-2" => {
                    app.nav.display = 1;
                    for (path, value) in [("/top/height", json!(44)), ("/top/design", json!("fluent")), ("/dock/enabled", json!(false))] {
                        app.run(Action::Set(crate::actions::Edit::new(DocId::Bars, format!("/overrides/DISPLAY2{path}"), "This display"), value));
                    }
                }
                "performance-custom" => {
                    app.run(Action::Set(crate::actions::Edit::new(DocId::Config, "/engine/fps", "Frame rate"), json!(73)));
                    app.nav.more.insert(glaze_ui::controls::key(("panel-custom-fps", "/engine/fps")));
                }
                "wallpaper-own-look" => {
                    let mut wallpaper = app.library.items[app.library.active().unwrap()].clone();
                    wallpaper.id = "rain-copy".into();
                    wallpaper.name = "Rainy Hong Kong (copy)".into();
                    wallpaper.active = false;
                    wallpaper.origin = crate::library::Origin::User;
                    app.nav.more.insert(glaze_ui::controls::key(("panel-wallpaper-look", &wallpaper.id)));
                    app.nav.wallpaper = Some(app.library.items.len());
                    app.library.items.push(wallpaper);
                }
                "newer-settings" | "malformed-settings" => {
                    let source = if name == "newer-settings" {
                        "{\"future_setting\":true}"
                    } else {
                        "{\n broken\n}"
                    };
                    app.docs = crate::docs::Docs::fixture([source, "{\"version\":1}", "{}", "{}"]);
                }
                "look-light-desktop" => app.run(Action::Set(
                    crate::actions::Edit::new(DocId::Config, "/look/appearance", "Appearance"),
                    json!("light"),
                )),
                "look-custom-color" => {
                    app.nav.more.insert(glaze_ui::controls::key((
                        "panel-color-picker",
                        DocId::Config,
                        "/look/material/tint",
                    )));
                }
                "weather" => app.nav.widget = Some("weather-1".into()),
                "now-playing" => app.nav.widget = Some("now-playing-1".into()),
                "system-monitor" => app.nav.widget = Some("system-monitor-1".into()),
                "clock" | "clock-world" | "clock-greeting" | "clock-own-look" => {
                    app.nav.widget = Some("clock-1".into());
                    if name == "clock-own-look" {
                        app.run(Action::Set(
                            crate::actions::Edit::new(
                                DocId::Widgets,
                                "/instances/0/appearance/surface",
                                "Its own look",
                            ),
                            json!("glass"),
                        ));
                        let layout = crate::editing::widgets::layout(&app);
                        let own = super::widget_look::appearance(&app, &layout.instances[0], super::widget_look::inherited_style(&app));
                        app.run(Action::Set(crate::actions::Edit::new(DocId::Widgets, "/instances/0/appearance", "Its own look"), own));
                    } else if name != "clock" {
                        app.run(Action::Set(
                            crate::actions::Edit::new(
                                DocId::Widgets,
                                "/instances/0/settings/variant",
                                "Face",
                            ),
                            json!(if name == "clock-world" {
                                "world"
                            } else {
                                "words"
                            }),
                        ));
                    }
                }
                "photos" | "photos-polaroid" => {
                    let mut layout = crate::editing::widgets::layout(&app);
                    let mut photo = layout.instances[0].clone();
                    photo.widget = "photo".into();
                    photo.instance = "photo-1".into();
                    photo.size = [6, 4];
                    if name == "photos-polaroid" {
                        photo.settings.insert("variant".into(), json!("polaroid"));
                    }
                    for (n, item) in app
                        .library
                        .items
                        .iter()
                        .filter(|w| w.preview.is_some())
                        .take(3)
                        .enumerate()
                    {
                        photo.settings.insert(
                            format!("image{}", n + 1),
                            json!(glaze_widgets::pick::data_url(
                                "photo",
                                "photo-1",
                                &format!("{}.png", item.name)
                            )),
                        );
                    }
                    layout.instances.push(photo);
                    app.docs
                        .edit(
                            "Fixture",
                            &[crate::docs::Op {
                                doc: DocId::Widgets,
                                path: String::new(),
                                value: Some(serde_json::to_value(layout).unwrap()),
                            }],
                        )
                        .unwrap();
                    app.nav.widget = Some("photo-1".into());
                }
                "look-more" | "windows-more" => {
                    app.nav.more.insert(glaze_ui::controls::key((
                        "panel-more",
                        glaze_ui::controls::key(("studio-panel", page)),
                    )));
                }
                "wallpaper-look" => app.nav.look_theme = true,
                "wallpaper" => {
                    app.nav.wallpaper = app
                        .library
                        .items
                        .iter()
                        .position(|w| w.kind.is_some_and(crate::library::Kind::uses_fit));
                }
                "wallpapers-broken" | "wallpaper-broken" => {
                    let mut wallpaper = app.library.items[0].clone();
                    wallpaper.id = "missing".into();
                    wallpaper.name = "beach-loop.mp4".into();
                    wallpaper.active = false;
                    wallpaper.error = Some("The file was moved or is no longer available.".into());
                    wallpaper.origin = crate::library::Origin::User;
                    app.library.items.insert(0, wallpaper);
                    app.nav.filter = 1;
                    app.nav.wallpaper = Some(0);
                }
                "add-wallpaper" => {
                    app.overlay = Some(crate::app::Overlay::AddWallpaper {
                        source: None,
                        picked: None,
                    })
                }
                "add-wallpaper-app" => {
                    app.overlay = Some(crate::app::Overlay::AddWallpaper {
                        source: Some(crate::actions::Source::App),
                        picked: Some(r"C:\Apps\visualizer.exe".into()),
                    });
                    app.texts
                        .insert(crate::actions::FieldId::AddName, "Visualizer".into());
                    app.texts
                        .insert(crate::actions::FieldId::AddArgs, "--embed {hwnd}".into());
                }
                "add-wallpaper-file" => {
                    let file = app
                        .library
                        .items
                        .iter()
                        .find_map(|w| w.preview.clone())
                        .unwrap();
                    app.overlay = Some(crate::app::Overlay::AddWallpaper {
                        source: Some(crate::actions::Source::File),
                        picked: Some(file.to_string_lossy().into()),
                    });
                    app.texts
                        .insert(crate::actions::FieldId::AddName, "Rain picture".into());
                }
                "connection" => app.nav.integration = crate::editing::integrations::LIST.len(),
                "connection-reconnect" => {
                    app.nav.integration = crate::editing::integrations::LIST.len() + 3
                }
                "history" => {
                    super::history::observe(&app);
                    app.run(Action::Preset("islands"));
                    app.run(Action::SetStyle(Some(glaze_config::look::Style::Solid)));
                    super::history::observe(&app);
                    app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(1);
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
                }
                "search" => {
                    app.texts
                        .insert(crate::actions::FieldId::Palette, "wallpaper".into());
                }
                "search-widgets" => {
                    app.texts
                        .insert(crate::actions::FieldId::Palette, "clock".into());
                }
                "save-failed" => {
                    super::save_failed(
                        &mut app,
                        Some(DocId::Config),
                        "The settings folder is read-only.",
                    );
                }
                "theme-save-failed" => {
                    let mut own = app.library.items[app.library.active().unwrap()].clone();
                    own.name = "Rain picture".into();
                    own.origin = crate::library::Origin::User;
                    own.active = false;
                    own.dir = PathBuf::from(r"C:\glaze-fixture-only\rain-picture");
                    super::theme_failed(&mut app, &own.dir, "The wallpaper folder is read-only.");
                    app.nav.wallpaper = Some(app.library.items.len());
                    app.library.items.push(own);
                }
                "first-run" | "first-run-taskbar" | "first-run-look" | "first-run-wallpaper"
                | "first-run-recovery" => {
                    use super::first_run::{DONE, LOOK, TASKBAR, WALLPAPER, WELCOME};
                    app.overlay = Some(crate::app::Overlay::FirstRun {
                        step: match name {
                            "first-run-taskbar" => TASKBAR,
                            "first-run-look" => LOOK,
                            "first-run-wallpaper" => WALLPAPER,
                            "first-run-recovery" => DONE,
                            _ => WELCOME,
                        },
                        starter: None,
                        taskbar: 0,
                    });
                }
                "top-bar" => app.nav.bar = Some(crate::actions::Bar::Top),
                "top-bar-designs" => {
                    app.panel.as_mut().unwrap().page = Page::BarDesign;
                    app.nav.bar = Some(crate::actions::Bar::Top);
                    app.nav.more.insert(glaze_ui::controls::key((
                        "panel-bar-designs",
                        crate::actions::Bar::Top,
                    )));
                }
                "dock" => app.nav.bar = Some(crate::actions::Bar::Dock),
                "dock-designs" => {
                    app.panel.as_mut().unwrap().page = Page::BarDesign;
                    app.nav.bar = Some(crate::actions::Bar::Dock);
                    app.nav.more.insert(glaze_ui::controls::key((
                        "panel-bar-designs",
                        crate::actions::Bar::Dock,
                    )));
                }
                "module" | "module-more" | "module-program" => {
                    app.nav.bar = Some(crate::actions::Bar::Top);
                    app.nav.module = Some(crate::actions::Slot {
                        bar: crate::actions::Bar::Top,
                        list: "right",
                        index: 0,
                    });
                    if name == "module-more" {
                        app.nav.more.insert(glaze_ui::controls::key((
                            "panel-more",
                            glaze_ui::controls::key(("studio-panel", page)),
                        )));
                    }
                    if name == "module-program" {
                        app.texts.insert(crate::actions::FieldId::Text(DocId::Studio, "/panel/draft/gesture/modules/media/click/left".into()), "program".into());
                        app.run(Action::Set(crate::actions::Edit::new(DocId::Bars, "/modules/media/click/left", "Program"), json!({"exec":"notepad.exe","args":[]})));
                        app.nav.more.insert(glaze_ui::controls::key((
                            "panel-trigger",
                            "/modules/media/click/left",
                        )));
                        app.texts.insert(
                            crate::actions::FieldId::Text(
                                DocId::Studio,
                                "/panel/draft/module-trigger/media/left".into(),
                            ),
                            "notepad.exe".into(),
                        );
                    }
                }
                "add-module" => {
                    app.nav.bar = Some(crate::actions::Bar::Top);
                    app.panel.as_mut().unwrap().add_module = Some("search".into());
                }
                "start-menu" => app.nav.panel = Some(crate::actions::Panel::Start),
                "calendar-panel" => app.nav.panel = Some(crate::actions::Panel::Calendar),
                "sound-panel" => app.nav.panel = Some(crate::actions::Panel::Sound),
                "tray-panel" => app.nav.panel = Some(crate::actions::Panel::Tray),
                "notifications-panel" => app.nav.panel = Some(crate::actions::Panel::Notifications),
                _ => {}
            }
            app.toast = None;
            let built = super::build(&app, &mut ui.text);
            let area = built.scrolls.last().unwrap();
            let content = area.content + area.viewport.top - 48.0;
            app.size.1 = (content + 48.0 + super::editors::footer_height(&app))
                .ceil()
                .clamp(160.0, 760.0) as u32;
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
                golden::write(
                    &image,
                    &out.join(format!(
                        "{name}-full-{}.png",
                        if dark { "dark" } else { "light" }
                    )),
                )
                .unwrap();
            } else {
                let obsolete = out.join(format!(
                    "{name}-full-{}.png",
                    if dark { "dark" } else { "light" }
                ));
                if obsolete.is_file() {
                    std::fs::remove_file(obsolete).unwrap();
                }
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
    let gpu = dropdown_option(&built, "/engine/gpu", |value| value == "auto");
    app.run(gpu);
    assert_eq!(app.get(DocId::Config, "/engine/gpu"), Some(&json!("auto")));
    app.monitors.truncate(1);
    app.panel.as_mut().unwrap().page = Page::Bars;
    app.run(Action::Panel(Command::Design(None)));
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
            Page::BarDesign,
            Page::Settings,
            Page::Performance,
            Page::Widget,
            Page::AddWidget,
            Page::Look,
            Page::Windows,
            Page::Wallpapers,
            Page::Wallpaper,
            Page::AddWallpaper,
            Page::Connections,
            Page::Connection,
            Page::General,
            Page::Keyboard,
            Page::Troubleshoot,
            Page::About,
            Page::DesktopMenu,
            Page::History,
            Page::Search,
            Page::Top,
            Page::Dock,
            Page::Module,
            Page::AddModule,
            Page::Panels,
            Page::BarPanel,
        ] {
            app.panel.as_mut().unwrap().page = page;
            if page == Page::Widget {
                app.nav.widget = Some("weather-1".into());
            }
            let built = super::build(&app, &mut ui.text);
            if page == Page::Performance && text_scale == 1.0 {
                // Three or more choices are a dropdown: its menu lists every label.
                let labels: Vec<_> = built.targets.iter().filter_map(|t| match &t.action {
                    Action::Choose(choices) => Some(choices.options.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>()),
                    _ => None,
                }).flatten().collect();
                for label in ["On battery", "Like Windows", "Always", "Never"] {
                    assert!(labels.iter().any(|l| l == label), "{label} is offered at {width}/{scale}");
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
    let screen = glaze_desktop::Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let work = glaze_desktop::Rect {
        x: 0,
        y: 32,
        width: 1920,
        height: 1048,
    };
    assert_eq!(super::bounds(screen, work, 1.0), (1432, 40, 480, 944));
    // 1920 physical px at 150% is a 1280-DIP laptop display: 360 DIPs, not 480.
    assert_eq!(super::bounds(screen, work, 1.5), (1368, 60, 540, 876));
}

#[test]
fn consecutive_live_picks_share_an_undo_step_and_history_row() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let bars = app.docs.doc(DocId::Bars).raw.clone();
    super::history::observe(&app);
    for preset in ["islands", "windows11", "floating"] {
        app.run(Action::Preset(preset));
    }
    assert_eq!(app.docs.undo_count(), 1);
    assert_eq!(
        app.panel.as_ref().unwrap().history.borrow().entries.len(),
        2
    );
    app.run(Action::Undo);
    assert_eq!(app.docs.doc(DocId::Bars).raw, bars);
    app.run(Action::Redo);
    assert_eq!(app.get(DocId::Bars, "/preset"), Some(&json!("floating")));
    super::run(&mut app, Command::Open(Page::Root));
    app.run(Action::Preset("macos"));
    assert_eq!(
        app.docs.undo_count(),
        2,
        "leaving the list ends the pick sequence"
    );
    let tint = crate::actions::Edit::new(DocId::Config, "/look/material/tint", "Tint");
    for color in ["#8b9eb7", "#7fae8e", "#c88d3c"] {
        app.run(Action::Set(tint.clone(), json!(color)));
    }
    assert_eq!(app.docs.undo_count(), 3, "material picks also coalesce");
    app.run(Action::Undo);
    assert_eq!(app.get(DocId::Config, "/look/material/tint"), None);
}

#[test]
fn restoring_a_desktop_preserves_settings_connections_and_later_rows() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let bars = app.docs.doc(DocId::Bars).raw.clone();
    super::history::observe(&app);
    app.run(Action::Preset("islands"));
    let count = app.panel.as_ref().unwrap().history.borrow().entries.len();
    for (doc, path, value) in [
        (DocId::Config, "/engine/fps", json!(60)),
        (DocId::Bars, "/performance", json!("on")),
        (DocId::Bars, "/animation/reduceMotion", json!("always")),
        (DocId::Integrations, "/media/enabled", json!(false)),
    ] {
        app.run(Action::Set(
            crate::actions::Edit::new(doc, path, "Settings"),
            value,
        ));
    }
    assert_eq!(
        app.panel.as_ref().unwrap().history.borrow().entries.len(),
        count
    );
    super::run(&mut app, Command::History(0));
    super::run(&mut app, Command::Restore);
    assert_eq!(app.get(DocId::Bars, "/preset"), bars.pointer("/preset"));
    assert_eq!(app.get(DocId::Config, "/engine/fps"), Some(&json!(60)));
    assert_eq!(app.get(DocId::Bars, "/performance"), Some(&json!("on")));
    assert_eq!(
        app.get(DocId::Bars, "/animation/reduceMotion"),
        Some(&json!("always"))
    );
    assert_eq!(
        app.get(DocId::Integrations, "/media/enabled"),
        Some(&json!(false))
    );
    assert!(
        app.panel
            .as_ref()
            .unwrap()
            .history
            .borrow()
            .entries
            .iter()
            .any(|e| e.title == "Islands preset")
    );
    app.run(Action::Undo);
    assert_eq!(
        app.get(DocId::Bars, "/preset"),
        Some(&json!("islands")),
        "restore is one undoable jump"
    );
}

#[test]
fn widget_creation_is_staged_and_search_opens_panel_pages() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let original = app.docs.doc(DocId::Widgets).raw.clone();
    super::run(&mut app, Command::Open(Page::AddWidget));
    super::run(&mut app, Command::PickWidget("notes".into()));
    assert_eq!(app.docs.doc(DocId::Widgets).raw, original);
    super::run(&mut app, Command::Back);
    assert_eq!(app.docs.doc(DocId::Widgets).raw, original);
    super::run(&mut app, Command::Open(Page::AddWidget));
    super::run(&mut app, Command::AddWidget);
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Widget);
    assert_eq!(crate::editing::widgets::layout(&app).instances.len(), 7);
    app.texts
        .insert(crate::actions::FieldId::Palette, "on battery".into());
    let results = crate::search::results(&app);
    let i = results
        .iter()
        .position(|e| e.id == "setting:pause_on_battery")
        .unwrap();
    super::run(&mut app, Command::Search(i));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Performance);
    assert!(
        DocId::ALL
            .into_iter()
            .all(|id| app.docs.doc(id).path.is_none())
    );
}

#[test]
fn windows_as_is_is_explicit_and_escape_leaves_the_add_page() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    super::run(&mut app, Command::Open(Page::Windows));
    let built = super::build(&app, &mut ui.text);
    let action = dropdown_option(&built, "/panel/windows/corners", serde_json::Value::is_null);
    app.run(action);
    assert_eq!(
        app.docs
            .doc(DocId::Config)
            .raw
            .pointer("/look/windows/corners"),
        Some(&serde_json::Value::Null)
    );
    super::run(&mut app, Command::Open(Page::AddWallpaper));
    app.key_with(0x1B, (false, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Wallpapers);
    assert!(app.overlay.is_none());
    assert!(!app.quit);
    let city =
        crate::actions::FieldId::Text(DocId::Widgets, "/instances/0/settings/zones/0/tz".into());
    assert_eq!(
        super::field_initial(&app, &city).as_deref(),
        Some("America/New_York")
    );
}

#[test]
fn keyboard_can_reorder_menu_items_without_removing_them() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    super::run(&mut app, Command::Open(Page::DesktopMenu));
    let built = super::build(&app, &mut ui.text);
    app.frame.targets = built.targets;
    app.input.focus = Some(super::lists::row_key(&super::SortList::Menu, 0));
    app.key_with(0x28, (false, false, true));
    let items = app
        .get(DocId::Config, "/desktop_menu/items")
        .and_then(serde_json::Value::as_array)
        .unwrap();
    assert_eq!(items[0], json!("sort"));
    assert_eq!(items[1], json!("view"));
    assert_eq!(
        items.len(),
        glaze_config::desktop_menu::default_items().len()
    );
    assert_eq!(
        app.input.focus,
        Some(super::lists::row_key(&super::SortList::Menu, 1))
    );
}

#[test]
fn a_list_drag_writes_once_on_drop_and_is_one_undo_step() {
    use glaze_ui::{Button, Event, Point, Pointer};
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    app.size.1 = 1400;
    super::run(&mut app, Command::Open(Page::DesktopMenu));
    let before = app.docs.doc(DocId::Config).raw.clone();
    let built = super::build(&app, &mut ui.text);
    let handle = built
        .targets
        .iter()
        .find(|t| t.action == Action::Panel(Command::SortHandle(super::SortList::Menu, 0)))
        .unwrap()
        .rect;
    let destination = built
        .targets
        .iter()
        .find(|t| t.action == Action::Panel(Command::SortRow(super::SortList::Menu, 2)))
        .unwrap()
        .rect;
    let pointer = |r: glaze_ui::Rect| Pointer {
        at: Point::new((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0),
        screen: (0, 0),
    };
    app.frame.targets = built.targets;
    app.event(Event::Press(pointer(handle), Button::Left));
    app.event(Event::Move(pointer(destination)));
    assert_eq!(
        app.docs.doc(DocId::Config).raw,
        before,
        "moving only paints the insertion marker"
    );
    assert_eq!(app.docs.undo_count(), 0);
    app.event(Event::Release(pointer(destination), Button::Left));
    let items = app
        .get(DocId::Config, "/desktop_menu/items")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(
        &items[..3],
        &[json!("sort"), json!("refresh"), json!("view")]
    );
    assert_eq!(app.docs.undo_count(), 1);
    app.run(Action::Undo);
    assert_eq!(app.docs.doc(DocId::Config).raw, before);
}

#[test]
fn sorting_modules_keeps_the_editor_on_the_same_module_and_removing_closes_it() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    app.monitors.truncate(1);
    app.nav.module = Some(crate::actions::Slot {
        bar: crate::actions::Bar::Top,
        list: "right",
        index: 0,
    });
    let list = super::SortList::Modules(crate::actions::Bar::Top, "right");
    let before = app.get(DocId::Bars, "/top/right/0").cloned().unwrap();
    super::lists::reorder(&mut app, &list, 0, 1);
    assert_eq!(app.nav.module.as_ref().unwrap().index, 1);
    assert_eq!(app.get(DocId::Bars, "/top/right/1"), Some(&before));
    super::lists::remove(&mut app, &list, 1);
    assert!(app.nav.module.is_none());
}

#[test]
fn save_failures_are_visible_on_the_affected_page_and_clear_after_a_valid_edit() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    super::run(&mut app, Command::Open(Page::Performance));
    let edit = crate::actions::Edit::new(DocId::Config, "/engine/fps", "Frame rate");
    app.run(Action::Set(edit.clone(), json!("invalid")));
    assert!(matches!(app.docs.saved, crate::docs::Saved::Failed(_)));
    let built = super::build(&app, &mut ui.text);
    assert!(
        built
            .targets
            .iter()
            .any(|t| t.action == Action::Panel(Command::RetryDoc(DocId::Config)))
    );
    assert!(
        !built
            .targets
            .iter()
            .any(|t| t.action == Action::StartOver(DocId::Config))
    );
    app.run(Action::Set(edit, json!(60)));
    assert!(app.panel.as_ref().unwrap().save_errors.borrow().is_empty());
    let built = super::build(&app, &mut ui.text);
    assert!(
        !built
            .targets
            .iter()
            .any(|t| t.action == Action::StartOver(DocId::Config))
    );
}

#[test]
fn display_navigation_and_home_shortcuts_do_not_edit_the_desktop() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    for page in [Page::Widgets, Page::Bars] {
        super::run(&mut app, Command::Open(page));
        let built = super::build(&app, &mut ui.text);
        assert!(
            built
                .targets
                .iter()
                .any(|t| t.action == Action::Panel(Command::Display(1)))
        );
    }
    let before = app.docs.version;
    super::run(&mut app, Command::Display(1));
    assert_eq!(app.nav.display, 1);
    app.key_with(0x30, (true, false, false));
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Root);
    assert_eq!(app.docs.version, before);
    app.monitors.truncate(1);
    app.nav.display = 0;
    super::run(&mut app, Command::Open(Page::Widgets));
    assert!(
        !super::build(&app, &mut ui.text)
            .targets
            .iter()
            .any(|t| matches!(t.action, Action::Panel(Command::Display(_))))
    );
}

#[test]
fn built_in_wallpaper_fit_requires_an_explicit_copy() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let index = app
        .library
        .items
        .iter()
        .position(|w| !w.editable() && w.kind.is_some_and(crate::library::Kind::uses_fit))
        .unwrap();
    super::run(&mut app, Command::Wallpaper(index));
    let built = super::build(&app, &mut ui.text);
    // The copy is offered in the details' "…" menu.
    assert!(built.targets.iter().any(|t| matches!(&t.action, Action::Menu(items)
        if items.iter().any(|item| matches!(item, crate::actions::MenuItem::Item { action, .. } if *action == Action::CopyWallpaper(index))))));
    assert!(
        !built.targets.iter().any(
            |t| matches!(&t.action,Action::Set(edit,_) if edit.path.ends_with("/wallpaper/fit"))
        )
    );
}

#[test]
fn first_run_stays_in_the_panel_and_finishes_through_its_footer() {
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    super::run(&mut app, Command::Open(Page::FirstRun));
    let built = super::build(&app, &mut ui.text);
    assert!(
        built
            .targets
            .iter()
            .any(|t| t.action == Action::FirstRunNext)
    );
    app.run(Action::FirstRunPick(0));
    app.run(Action::FirstRunNext);
    assert!(matches!(
        app.overlay,
        Some(crate::app::Overlay::FirstRun { step: super::first_run::TASKBAR, .. })
    ));
    app.run(Action::FirstRunTaskbar(1));
    app.run(Action::FirstRunNext);
    assert!(matches!(
        app.overlay,
        Some(crate::app::Overlay::FirstRun { step: super::first_run::LOOK, .. })
    ));
    // Keeping Windows' taskbar skips the bar steps: look, wallpaper, extras, then done.
    for _ in 0..4 {
        app.run(Action::FirstRunNext);
    }
    assert_eq!(app.panel.as_ref().unwrap().page, Page::Root);
    assert_eq!(
        app.get(DocId::Studio, "/first_run_done"),
        Some(&json!(true))
    );
    assert!(
        DocId::ALL
            .into_iter()
            .all(|id| app.docs.doc(id).path.is_none())
    );
}

#[test]
fn history_summarizes_the_whole_coalesced_style_change_and_fields_ignore_typed_cache() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    super::history::observe(&app);
    app.run(Action::SetStyle(Some(glaze_config::look::Style::Solid)));
    app.run(Action::SetStyle(Some(glaze_config::look::Style::Fluent)));
    assert_eq!(
        app.panel
            .as_ref()
            .unwrap()
            .history
            .borrow()
            .entries
            .last()
            .unwrap()
            .summary,
        "Look: Liquid → Fluent"
    );
    let field =
        crate::actions::FieldId::Text(DocId::Widgets, "/instances/0/settings/zones/0/tz".into());
    app.texts.insert(field.clone(), "just-typed text".into());
    assert_eq!(
        super::field_initial(&app, &field).as_deref(),
        Some("America/New_York")
    );
}

#[test]
fn custom_module_interval_text_is_saved_as_a_number() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    app.run(Action::Set(
        crate::actions::Edit::new(DocId::Bars, "/modules/custom:example/command", "Program"),
        json!("example.exe"),
    ));
    app.run(Action::Set(
        crate::actions::Edit::new(DocId::Bars, "/modules/custom:example/interval", "Run every"),
        json!("60"),
    ));
    assert_eq!(
        app.get(DocId::Bars, "/modules/custom:example/interval"),
        Some(&json!(60.0)),
        "{:?}",
        app.docs.saved
    );
}

#[test]
fn moving_a_module_between_bars_and_disconnect_are_single_undo_steps() {
    let ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    app.nav.module = Some(crate::actions::Slot {
        bar: crate::actions::Bar::Top,
        list: "right",
        index: 0,
    });
    let before = app.docs.doc(DocId::Bars).raw.clone();
    super::run(
        &mut app,
        Command::MoveModuleTo(crate::actions::Bar::Dock, "left"),
    );
    assert_eq!(app.docs.undo_count(), 1);
    assert_eq!(
        app.nav.module.as_ref().unwrap().bar,
        crate::actions::Bar::Dock
    );
    app.run(Action::Undo);
    assert_eq!(app.docs.doc(DocId::Bars).raw, before);
    let mut widgets = app.docs.doc(DocId::Widgets).raw.clone();
    let mut face = widgets["instances"][0].clone();
    face["instance"] = json!("t3-agents-1");
    face["widget"] = json!("t3--agents");
    face["cell"] = json!([10, 10]);
    widgets["instances"].as_array_mut().unwrap().push(face);
    let mut bars = before;
    for (bar, zone) in [("top", "left"), ("dock", "right")] {
        let mut modules = app
            .get(DocId::Bars, &format!("/{bar}/{zone}"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        modules.push(json!("custom:t3--status"));
        if !bars[bar].is_object() {
            bars[bar] = json!({});
        }
        bars[bar][zone] = json!(modules);
    }
    app.docs = crate::docs::Docs::fixture([
        &app.docs.doc(DocId::Config).raw.to_string(),
        &bars.to_string(),
        &widgets.to_string(),
        "{}",
    ]);
    assert!(app.docs.doc(DocId::Bars).error.is_none());
    assert!(app.docs.doc(DocId::Widgets).error.is_none());
    super::run(&mut app, Command::Disconnect("t3".into()));
    assert!(app.plugins.preferences.disabled.contains("t3"));
    assert_eq!(app.docs.undo_count(), 1);
    assert_eq!(
        app.get(DocId::Widgets, "/instances")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        widgets["instances"].as_array().unwrap().len() - 1
    );
    for pointer in ["/top/left", "/dock/right"] {
        assert!(
            !app.get(DocId::Bars, pointer)
                .unwrap()
                .as_array()
                .unwrap()
                .contains(&json!("custom:t3--status"))
        );
    }
    app.run(Action::Undo);
    assert!(!app.plugins.preferences.disabled.contains("t3"));
    assert_eq!(app.docs.doc(DocId::Widgets).raw, widgets);
    assert_eq!(app.docs.doc(DocId::Bars).raw, bars);
}

#[test]
fn style_specimens_ignore_the_current_material_and_native_windows_are_distinct() {
    use glaze_ui::Rect;
    use glaze_ui::controls::Builder;
    let mut ui = Ui::new().unwrap();
    let mut app = app(&ui, true);
    let render_material = |app: &App, text: &mut glaze_ui::Text| {
        let mut b = Builder::new(text, &app.theme.palette, &app.theme.ty, 1.0, &app.input);
        super::specimens::material(
            app,
            &mut b,
            Rect::new(0.0, 0.0, 160.0, 100.0),
            glaze_config::look::Style::Fluent,
        );
        b.list.ops
    };
    let before = render_material(&app, &mut ui.text);
    app.docs
        .edit(
            "Current material",
            &[crate::docs::Op {
                doc: DocId::Config,
                path: "/look/material/clarity".into(),
                value: Some(json!(1.0)),
            }],
        )
        .unwrap();
    assert_eq!(render_material(&app, &mut ui.text), before);
    let window = |look, text: &mut glaze_ui::Text| {
        let mut b = Builder::new(text, &app.theme.palette, &app.theme.ty, 1.0, &app.input);
        super::specimens::window(&app, &mut b, Rect::new(0.0, 0.0, 160.0, 100.0), look);
        b.list.ops
    };
    assert_ne!(window(0, &mut ui.text), window(1, &mut ui.text));
}

#[test]
fn unreadable_history_cannot_be_cleared_by_an_older_reader() {
    let ui = Ui::new().unwrap();
    let app = app(&ui, true);
    super::history::observe(&app);
    let count = app.panel.as_ref().unwrap().history.borrow().entries.len();
    app.panel.as_ref().unwrap().history.borrow_mut().error = Some(crate::docs::NEWER.into());
    super::history::clear(&app);
    let history = app.panel.as_ref().unwrap().history.borrow();
    assert_eq!(history.error.as_deref(), Some(crate::docs::NEWER));
    assert_eq!(history.entries.len(), count);
}
