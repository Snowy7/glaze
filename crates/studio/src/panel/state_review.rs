//! State fixtures rendered through the real panel; never changes Windows settings.
use super::{Page, State};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    docs::{DocId, Op},
};
use mypaper_ui::{Rgba, Ui, golden, render};
use serde_json::json;

fn fixture(ui: &Ui, dark: bool, page: Page) -> App {
    let mut app = crate::goldens::app(dark, ui);
    app.panel = Some(State {
        page,
        ..Default::default()
    });
    app.size = (480, 760);
    if app.library.active().is_none() && !app.library.items.is_empty() {
        let i = app
            .library
            .items
            .iter()
            .position(|item| item.id == "rain-city")
            .unwrap_or(0);
        app.library.items[i].active = true;
    }
    app
}

#[test]
fn first_run_choices_are_independent_of_the_previous_choice() {
    let mut ui = Ui::new().unwrap();
    let mut app = fixture(&ui, true, Page::Root);
    super::run(&mut app, super::Command::Open(Page::FirstRun));
    app.run(Action::FirstRunPick(0));
    let expected =
        [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| app.docs.doc(id).raw.clone());
    app.run(Action::FirstRunTaskbar(2));
    assert!(crate::editing::widgets::layout(&app).instances.is_empty());
    app.run(Action::FirstRunTaskbar(0));
    assert_eq!(
        [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| app.docs.doc(id).raw.clone()),
        expected
    );
    app.run(Action::FirstRunTaskbar(1));
    app.run(Action::FirstRunPick(2));
    assert_eq!(app.get(DocId::Bars, "/top/enabled"), Some(&json!(false)));
    app.run(Action::FirstRunTaskbar(0));
    assert_eq!(app.get(DocId::Bars, "/preset"), Some(&json!("islands")));
    assert_eq!(app.get(DocId::Bars, "/dock/enabled"), Some(&json!(true)));
    assert!(!crate::editing::widgets::layout(&app).instances.is_empty());
    for step in 0..=super::first_run::DONE {
        app.overlay = Some(crate::app::Overlay::FirstRun {
            step,
            starter: Some(2),
            taskbar: 0,
        });
        let built = super::build(&app, &mut ui.text);
        assert_eq!(
            built.list.ops.iter().any(
                |op| matches!(op, mypaper_ui::Op::Text {text,..} if text.as_ref() == "If something goes wrong")
            ),
            step == super::first_run::DONE
        );
        assert!(
            built
                .list
                .hits
                .iter()
                .any(|hit| hit.label.as_ref() == "Skip setup")
        );
    }
}

#[test]
fn release_state_routes_and_shared_units_are_exposed() {
    let mut ui = Ui::new().unwrap();
    let mut app = fixture(&ui, true, Page::Root);
    app.system.parts[1].running = false;
    let built = super::build(&app, &mut ui.text);
    assert!(
        built
            .targets
            .iter()
            .any(|target| target.action == Action::Restart("engine"))
    );
    app.system.parts = Default::default();
    let built = super::build(&app, &mut ui.text);
    let header: Vec<_> = built
        .list
        .hits
        .iter()
        .filter(|hit| hit.rect.top < 48.0)
        .map(|hit| hit.label.as_ref())
        .collect();
    assert_eq!(header, ["Done"]);
    assert!(
        built
            .targets
            .iter()
            .any(|target| target.action == Action::StartMypaper)
    );
    assert_eq!(super::controls::unit(12.0, "px"), "12 px");
    assert_eq!(super::controls::unit(55.0, "%"), "55%");
}

#[test]
fn render_panel_states() {
    let out = std::env::var_os("MYPAPER_STUDIO_PANEL_SHOTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"C:\mypaper-targets\studio-panel\shots".into())
        .join("states");
    std::fs::create_dir_all(&out).unwrap();
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        for (name, page) in [
            ("off", Page::Root),
            ("paused", Page::Root),
            ("stopped-part", Page::Root),
            ("bars-stopped", Page::Bars),
            ("wallpapers-none-own", Page::Wallpapers),
            ("wallpapers-brings-look", Page::Wallpapers),
            ("search-empty", Page::Search),
            ("widgets-empty", Page::Widgets),
            ("no-room", Page::AddWidget),
            ("large-home", Page::Root),
            ("large-bars", Page::Bars),
            ("large-top-bar", Page::Top),
            ("large-performance", Page::Performance),
            ("performance-360", Page::Performance),
            ("performance-360-large-text", Page::Performance),
            ("wallpaper-web", Page::Wallpaper),
            ("wallpaper-editable-fit", Page::Wallpaper),
            ("wallpaper-fit-360-large-text", Page::Wallpaper),
            ("large-widget", Page::Widget),
            ("large-wallpapers", Page::Wallpapers),
            ("high-contrast", Page::Root),
            ("look-widgets-no-background", Page::Look),
        ] {
            let mut app = fixture(&ui, dark, page);
            match name {
                "off" => app.system.parts = Default::default(),
                "paused" => app.run(Action::Set(
                    Edit::new(DocId::Config, "/paused", "Pause"),
                    json!(true),
                )),
                "stopped-part" => app.system.parts[1].running = false,
                "bars-stopped" => app.system.parts[2].running = false,
                "wallpaper-web" | "wallpaper-editable-fit" | "wallpaper-fit-360-large-text" => {
                    let mut wallpaper = app.library.items[app.library.active().unwrap()].clone();
                    wallpaper.origin = crate::library::Origin::User;
                    wallpaper.active = false;
                    if name == "wallpaper-web" {
                        wallpaper.kind = Some(crate::library::Kind::Web);
                        wallpaper.interactive = Some(true);
                        wallpaper.name = "Web wallpaper".into();
                    } else {
                        wallpaper.kind = Some(crate::library::Kind::Image);
                        wallpaper.name = "Rain picture".into();
                    }
                    app.nav.wallpaper = Some(app.library.items.len());
                    app.library.items.push(wallpaper);
                }
                "wallpapers-none-own" => {
                    app.nav.filter = 1;
                    app.library.items.retain(|wallpaper| !wallpaper.editable());
                }
                "wallpapers-brings-look" => {
                    let mut wallpaper = app.library.items[app.library.active().unwrap()].clone();
                    wallpaper.name = "Rain with a look".into();
                    wallpaper.id = "rain-look-fixture".into();
                    wallpaper.origin = crate::library::Origin::User;
                    wallpaper.active = false;
                    wallpaper.brings_look = true;
                    app.library.items.insert(0, wallpaper);
                }
                "search-empty" => {
                    app.texts
                        .insert(FieldId::Palette, "no-such-setting-73921".into());
                }
                "widgets-empty" => app
                    .docs
                    .edit(
                        "Fixture",
                        &[Op {
                            doc: DocId::Widgets,
                            path: "/instances".into(),
                            value: Some(json!([])),
                        }],
                    )
                    .unwrap(),
                "no-room" => {
                    let grid = crate::stage::grid_for(
                        &crate::editing::widgets::layout(&app),
                        app.display().unwrap(),
                    );
                    app.docs.edit("Fixture", &[Op{doc:DocId::Widgets,path:"/instances".into(),value:Some(json!([{"instance":"cover","widget":"clock","monitor":"","cell":[0,0],"size":[grid.cols,grid.rows]}]))}]).unwrap();
                    app.panel.as_mut().unwrap().add_widget = Some("clock".into());
                }
                "large-widget" => app.nav.widget = Some("clock-1".into()),
                "look-widgets-no-background" => {
                    app.run(Action::Set(
                        Edit::new(DocId::Widgets, "/appearance/surface", "Widget background"),
                        json!("none"),
                    ));
                    app.nav.more.insert(mypaper_ui::controls::key((
                        "panel-more",
                        mypaper_ui::controls::key(("studio-panel", Page::Look)),
                    )));
                }
                "high-contrast" => {
                    app.theme.contrast = true;
                    let (window, ink) = if dark {
                        ([0, 0, 0], [255, 255, 255])
                    } else {
                        ([255, 255, 255], [0, 0, 0])
                    };
                    app.theme.palette = mypaper_ui::controls::Palette::contrast(
                        Rgba::rgb8(window, 1.0),
                        Rgba::rgb8(ink, 1.0),
                        Rgba::rgb8(if dark { [26, 235, 255] } else { [0, 0, 128] }, 1.0),
                        Rgba::rgb8([128, 128, 128], 1.0),
                        Rgba::rgb8(window, 1.0),
                    );
                }
                _ => {}
            }
            if name.starts_with("large-") || name.ends_with("large-text") {
                app.theme.ty.text_scale = 1.5;
            }
            if name.contains("360") {
                app.size.0 = 360;
            }
            let width = app.size.0;
            app.toast = None;
            let built = super::build(&app, &mut ui.text);
            let full = (built.scrolls.last().unwrap().content
                + 48.0
                + super::editors::footer_height(&app))
            .ceil()
            .max(160.0) as u32;
            for full_page in [false, true] {
                if full_page && full <= 760 {
                    continue;
                }
                app.size.1 = if full_page { full } else { full.min(760) };
                let built = super::build(&app, &mut ui.text);
                for target in &built.targets {
                    assert!(
                        target.rect.left >= 0.0 && target.rect.right <= width as f32,
                        "{name}: {:?}",
                        target.rect
                    );
                }
                let image = render(&mut ui, &built.list, width, app.size.1).unwrap();
                let file = format!(
                    "{name}{}-{}.png",
                    if full_page { "-full" } else { "" },
                    if dark { "dark" } else { "light" }
                );
                golden::write(&image, &out.join(file)).unwrap();
            }
        }
    }
}
