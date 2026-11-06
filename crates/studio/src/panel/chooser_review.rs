//! Headless review pictures for the chooser states; no windows, services or settings writes.
use super::*;
use crate::{
    actions::{Bar, Edit},
    docs::DocId,
    panel::{Page, State as PanelState},
};
use mypaper_config::look::Style;
use mypaper_ui::{Point, Ui, golden, render};
use serde_json::json;

fn fixture(ui: &Ui, dark: bool, page: Page) -> App {
    let mut app = crate::goldens::app(dark, ui);
    app.panel = Some(PanelState {
        page,
        ..Default::default()
    });
    app.size = (480, 1300);
    if app.library.active().is_none() && !app.library.items.is_empty() {
        let index = app
            .library
            .items
            .iter()
            .position(|wallpaper| wallpaper.id == "rain-city")
            .unwrap_or(0);
        app.library.items[index].active = true;
    }
    let pins = crate::panel::icons::fixture_pins();
    app.run(Action::Set(
        Edit::new(DocId::Bars, "/dock/pinned", "Pinned apps"),
        json!(pins),
    ));
    app
}

fn shot(app: &mut App, ui: &mut Ui, name: &str, hovered: Option<Action>, dark: bool) {
    app.toast = None;
    let out = std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r11");
    let built = crate::panel::build(app, &mut ui.text);
    let scroll = built.scrolls.last().unwrap();
    app.size.1 =
        (scroll.content + app.size.1 as f32 - scroll.viewport.height() + 8.0).ceil() as u32;
    let built = crate::panel::build(app, &mut ui.text);
    if let Some(action) = hovered {
        let target = built
            .targets
            .iter()
            .rfind(|target| {
                target.action == action
                    || matches!(&target.action,
                Action::Panel(super::super::Command::ApplyChoice(_, inner)) if **inner == action)
            })
            .expect("hovered action must be present on the review page");
        app.input.pointer = Some(Point::new(target.rect.left + 30.0, target.rect.top + 10.0));
        app.input.hover = Some(target.key);
    }
    let built = crate::panel::build(app, &mut ui.text);
    let image = render(ui, &built.list, app.size.0, app.size.1).unwrap();
    golden::write(
        &image,
        &out.join(format!(
            "{name}-{}.png",
            if dark { "dark" } else { "light" }
        )),
    )
    .unwrap();
}

fn viewport_shot(app: &App, ui: &mut Ui, name: &str, dark: bool) {
    let built = crate::panel::build(app, &mut ui.text);
    let image = render(ui, &built.list, app.size.0, app.size.1).unwrap();
    golden::write(
        &image,
        &std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r11").join(format!(
            "{name}-{}.png",
            if dark { "dark" } else { "light" }
        )),
    )
    .unwrap();
}

#[test]
fn render_chooser_review() {
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        let mut app = fixture(&ui, dark, Page::Look);
        app.size = (480, 800);
        viewport_shot(&app, &mut ui, "look-first-screen", dark);
        let frame = crate::panel::build(&app, &mut ui.text);
        let row = frame
            .targets
            .iter()
            .find(|target| target.action == Action::SetStyle(Some(Style::Fluent)))
            .unwrap();
        app.input.pointer = Some(Point::new(row.rect.left + 20.0, row.rect.top + 10.0));
        viewport_shot(&app, &mut ui, "look-first-screen-hover", dark);
        let mut gallery = fixture(&ui, dark, Page::AddWidget);
        gallery.size = (480, 800);
        viewport_shot(&gallery, &mut ui, "gallery-top", dark);
        let frame = crate::panel::build(&gallery, &mut ui.text);
        let scroll = frame.scrolls.last().unwrap();
        gallery.nav.scroll.insert(
            scroll.key,
            (scroll.content - scroll.viewport.height()).max(0.0),
        );
        viewport_shot(&gallery, &mut ui, "gallery-plugins", dark);
        let mut photo = fixture(&ui, dark, Page::Widget);
        photo.run(Action::AddWidget("photo".into()));
        photo.nav.widget = crate::pages::widgets::layout(&photo)
            .instances
            .iter()
            .find(|instance| instance.widget == "photo")
            .map(|instance| instance.instance.clone());
        shot(&mut photo, &mut ui, "photos-full", None, dark);
        for style in Style::ALL {
            let mut app = fixture(&ui, dark, Page::Look);
            app.run(Action::Set(
                Edit::new(DocId::Config, "/look/appearance", "Appearance"),
                json!(if dark { "dark" } else { "light" }),
            ));
            shot(
                &mut app,
                &mut ui,
                &format!("look-hover-{style:?}"),
                Some(Action::SetStyle(Some(style))),
                dark,
            );
        }
        for index in 0..4 {
            let mut app = fixture(&ui, dark, Page::Windows);
            app.run(Action::Set(
                Edit::new(DocId::Config, "/look/appearance", "Appearance"),
                json!(if dark { "dark" } else { "light" }),
            ));
            app.run(Action::Set(
                Edit::new(DocId::Config, "/look/material/tint", "Tint"),
                json!("#488B73"),
            ));
            shot(
                &mut app,
                &mut ui,
                &format!("windows-hover-{index}"),
                Some(Action::WindowsLook(index)),
                dark,
            );
        }
        for (widget, face) in [("clock-1", "world"), ("now-playing-1", "strip")] {
            let mut app = fixture(&ui, dark, Page::Widget);
            app.nav.widget = Some(widget.into());
            let layout = crate::pages::widgets::layout(&app);
            let index = layout
                .instances
                .iter()
                .position(|instance| instance.instance == widget)
                .unwrap();
            shot(
                &mut app,
                &mut ui,
                &format!("{widget}-collapsed"),
                None,
                dark,
            );
            app.nav.more.insert(key(("panel-faces", widget, "variant")));
            let path = format!("/instances/{index}/settings/variant");
            let action = crate::panel::build(&app, &mut ui.text)
                .targets
                .into_iter()
                .find_map(|target| {
                    let action = match target.action {
                        Action::Panel(super::super::Command::ApplyChoice(_, action)) => *action,
                        action => action,
                    };
                    match action {
                        Action::Set(edit, value)
                            if edit.path.as_ref() == path && value == json!(face) =>
                        {
                            Some(Action::Set(edit, value))
                        }
                        _ => None,
                    }
                })
                .unwrap();
            shot(
                &mut app,
                &mut ui,
                &format!("{widget}-hover-{face}"),
                Some(action),
                dark,
            );
        }
        let mut app = fixture(&ui, dark, Page::Widget);
        app.nav.widget = Some("clock-1".into());
        let layout = crate::pages::widgets::layout(&app);
        let own = crate::panel::widget_look::appearance(&app, &layout.instances[0], Style::Liquid);
        app.run(Action::Set(
            Edit::new(DocId::Widgets, "/instances/0/appearance", "Its own look"),
            own,
        ));
        shot(&mut app, &mut ui, "clock-own-material", None, dark);
        let mut app = fixture(&ui, dark, Page::Widget);
        app.nav.widget = Some("calendar-1".into());
        let layout = crate::pages::widgets::layout(&app);
        let index = layout
            .instances
            .iter()
            .position(|instance| instance.instance == "calendar-1")
            .unwrap();
        let own =
            crate::panel::widget_look::appearance(&app, &layout.instances[index], Style::Oneui);
        app.run(Action::Set(
            Edit::new(
                DocId::Widgets,
                format!("/instances/{index}/appearance"),
                "Its own look",
            ),
            own,
        ));
        shot(&mut app, &mut ui, "calendar-own-material", None, dark);
        let mut app = fixture(&ui, dark, Page::Look);
        app.library.items.clear();
        shot(
            &mut app,
            &mut ui,
            "look-no-wallpaper",
            Some(Action::SetStyle(Some(Style::Liquid))),
            dark,
        );
        let mut app = fixture(&ui, dark, Page::Windows);
        app.run(Action::Set(Edit::new(DocId::Config, "/look/windows", "Windows look"),
            json!({"apps":"light","caption":"#CFD8EA","caption_text":"#1E2A42","border":"#8398B7","corners":"square","backdrop":"acrylic"})));
        shot(&mut app, &mut ui, "windows-custom", None, dark);
        let mut app = fixture(&ui, dark, Page::Look);
        let index = app
            .library
            .items
            .iter()
            .position(|wallpaper| !wallpaper.active)
            .unwrap();
        app.library.items[index].origin = crate::library::Origin::User;
        app.run(Action::Panel(crate::panel::Command::WallpaperLook(index)));
        let frame = crate::panel::build(&app, &mut ui.text);
        app.run(
            frame
                .targets
                .iter()
                .find(|target| matches!(target.action, Action::More(_)))
                .unwrap()
                .action
                .clone(),
        );
        shot(&mut app, &mut ui, "wallpaper-own-look", None, dark);
        let mut app = fixture(&ui, dark, Page::Windows);
        app.size.0 = 360;
        app.theme.ty.text_scale = 1.5;
        shot(
            &mut app,
            &mut ui,
            "windows-hover-360-large-text",
            Some(Action::WindowsLook(1)),
            dark,
        );
        for bar in [None, Some(Bar::Top), Some(Bar::Dock)] {
            let mut app = fixture(&ui, dark, Page::BarDesign);
            app.nav.bar = bar;
            shot(&mut app, &mut ui, &format!("bars-{bar:?}"), None, dark);
        }
        let mut launcher = fixture(&ui, dark, Page::Widget);
        launcher.run(Action::AddWidget("launcher".into()));
        let layout = crate::pages::widgets::layout(&launcher);
        let index = layout
            .instances
            .iter()
            .position(|instance| instance.widget == "launcher")
            .unwrap();
        launcher.nav.widget = Some(layout.instances[index].instance.clone());
        let path = format!("/instances/{index}/settings/shortcuts");
        launcher
            .nav
            .more
            .insert(key(("panel-list-item", path.as_str(), 0usize)));
        shot(&mut launcher, &mut ui, "launcher-picker", None, dark);
        for index in 0..crate::pages::first_run::panel_starters().len() {
            let mut starter = fixture(&ui, dark, Page::FirstRun);
            starter.overlay = Some(crate::app::Overlay::FirstRun {
                step: 1,
                starter: 0,
                taskbar: 0,
            });
            starter.panel.as_mut().unwrap().page = Page::FirstRun;
            shot(
                &mut starter,
                &mut ui,
                &format!("starter-hover-{index}"),
                Some(Action::FirstRunPick(index)),
                dark,
            );
        }
        let mut app = fixture(&ui, dark, Page::Look);
        app.size = (360, 620);
        let edit = Edit::new(DocId::Config, "/look/material/tint", "Tint");
        app.run(Action::Set(edit.clone(), json!("#2379AB")));
        app.run(Action::Panel(crate::panel::Command::PickColor(edit)));
        let built = crate::panel::build(&app, &mut ui.text);
        let image = render(&mut ui, &built.list, 360, 620).unwrap();
        golden::write(
            &image,
            &std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r11").join(
                format!("color-360-{}.png", if dark { "dark" } else { "light" }),
            ),
        )
        .unwrap();
    }
}

#[test]
fn render_gallery_countdown_fits_and_taskbar_choices() {
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        let mut gallery = fixture(&ui, dark, Page::AddWidget);
        gallery.panel.as_mut().unwrap().category = Some("widget:Time".into());
        gallery.run(Action::Panel(crate::panel::Command::PickWidget(
            "clock".into(),
        )));
        gallery.size = (480, 800);
        viewport_shot(&gallery, &mut ui, "gallery-selected", dark);
        let mut countdown = fixture(&ui, dark, Page::Widget);
        countdown.run(Action::AddWidget("countdown".into()));
        countdown.nav.widget = crate::pages::widgets::layout(&countdown)
            .instances
            .iter()
            .find(|instance| instance.widget == "countdown")
            .map(|instance| instance.instance.clone());
        shot(&mut countdown, &mut ui, "countdown-faces", None, dark);
        for taskbar in 0..3 {
            let mut first = fixture(&ui, dark, Page::FirstRun);
            first.overlay = Some(crate::app::Overlay::FirstRun {
                step: 1,
                starter: 0,
                taskbar,
            });
            first.panel.as_mut().unwrap().page = Page::FirstRun;
            shot(
                &mut first,
                &mut ui,
                &format!("starters-taskbar-{taskbar}"),
                None,
                dark,
            );
            if taskbar == 0 {
                first.overlay = Some(crate::app::Overlay::FirstRun {
                    step: 0,
                    starter: 0,
                    taskbar,
                });
                shot(&mut first, &mut ui, "first-run-taskbar-first", None, dark);
            }
        }
        for page in [Page::Look, Page::FirstRun, Page::AddWidget] {
            let mut narrow = fixture(&ui, dark, page);
            narrow.size = (360, 800);
            narrow.theme.ty.text_scale = 1.5;
            if page == Page::FirstRun {
                narrow.overlay = Some(crate::app::Overlay::FirstRun {
                    step: 1,
                    starter: 0,
                    taskbar: 0,
                });
                narrow.panel.as_mut().unwrap().page = Page::FirstRun;
            }
            shot(
                &mut narrow,
                &mut ui,
                &format!("{page:?}-360-large"),
                None,
                dark,
            );
        }
        let app = fixture(&ui, dark, Page::Wallpaper);
        let p = crate::panel::palette_for(&app);
        let mut b =
            mypaper_ui::controls::Builder::new(&mut ui.text, &p, &app.theme.ty, 1.0, &app.input);
        b.fill(Rect::new(0.0, 0.0, 480.0, 330.0), 0.0, p.chrome);
        let mut c = crate::ui::Col {
            x: 16.0,
            y: 16.0,
            w: 448.0,
            view: key("fit-review"),
        };
        if let Some(image) = crate::panel::specimens::wallpaper(&app) {
            crate::panel::specimens::fit_tiles(
                &app,
                &mut b,
                &mut c,
                "cover",
                Edit::new(DocId::Config, "/wallpaper/fit", "Fit"),
                Some(image),
            );
        }
        c.y += 12.0;
        let image = std::sync::Arc::new(mypaper_ui::Image {
            width: 120,
            height: 180,
            pixels: (0..120 * 180)
                .flat_map(|index| {
                    let (x, y) = (index % 120, index / 120);
                    [
                        if x < 60 { 190 } else { 65 },
                        (90 + y / 2) as u8,
                        if y < 90 { 210 } else { 80 },
                        255,
                    ]
                })
                .collect(),
        });
        crate::panel::specimens::fit_tiles(
            &app,
            &mut b,
            &mut c,
            "contain",
            Edit::new(DocId::Config, "/portrait/fit", "Fit"),
            Some(image),
        );
        let list = b.list;
        let image = render(&mut ui, &list, 480, c.y.ceil() as u32 + 16).unwrap();
        golden::write(
            &image,
            &std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r11").join(
                format!("fit-stills-{}.png", if dark { "dark" } else { "light" }),
            ),
        )
        .unwrap();
    }
}

#[test]
fn render_retained_plugin_face_and_transparent_widget() {
    let mut ui = Ui::new().unwrap();
    let trees: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../plugins/t3/golden/faces.json"
    )).unwrap();
    for dark in [true, false] {
        let mut app = fixture(&ui, dark, Page::AddWidget);
        let id = "t3--running";
        let manifest = mypaper_widgets::manifest::WidgetManifest::parse(
            &json!({"id": id,"name":"T3 · Running now","category":"Plugins",
                "description":"From your connected T3 app","size":{"default":[4,3],"min":[2,2]},"settings":[]}).to_string(), id
        ).unwrap();
        app.registry.widgets.clear();
        app.registry.widgets.insert(id.into(), mypaper_widgets::registry::Installed { manifest });
        super::begin(&app);
        app.panel.as_ref().unwrap().chooser.borrow_mut().plugin_faces = Some(json!({
            id: {"title":"Running now","icon":"terminal","status":"live","face":trees["running"]["small"]}
        }));
        app.size = (480, 650);
        viewport_shot(&app, &mut ui, "gallery-retained-plugin", dark);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().drawings.keys().any(|key| key.starts_with("face:t3--running")));

        let mut app = fixture(&ui, dark, Page::Widget);
        app.nav.widget = Some("clock-1".into());
        app.run(Action::Set(Edit::new(DocId::Widgets, "/appearance/surface", "Background"), json!("none")));
        app.run(Action::Set(Edit::new(DocId::Widgets, "/instances/0/settings/variant", "Face"), json!("flip")));
        shot(&mut app, &mut ui, "clock-no-background", None, dark);
    }
}
