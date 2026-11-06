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
    let out = std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r7");
    let built = crate::panel::build(app, &mut ui.text);
    app.size.1 = (built.scrolls.last().unwrap().content + 48.0).ceil() as u32;
    let built = crate::panel::build(app, &mut ui.text);
    if let Some(action) = hovered {
        let target = built
            .targets
            .iter()
            .find(|target| target.action == action)
            .unwrap();
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

#[test]
fn render_chooser_review() {
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
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
            let path = format!("/instances/{index}/settings/variant");
            let action = crate::panel::build(&app, &mut ui.text)
                .targets
                .into_iter()
                .find_map(|target| match target.action {
                    Action::Set(edit, value)
                        if edit.path.as_ref() == path && value == json!(face) =>
                    {
                        Some(Action::Set(edit, value))
                    }
                    _ => None,
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
                step: 0,
                starter: 0,
                taskbar: 0,
            });
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
            &std::path::PathBuf::from(r"C:\mypaper-targets\studio-panel\shots-a-r7").join(format!(
                "color-360-{}.png",
                if dark { "dark" } else { "light" }
            )),
        )
        .unwrap();
    }
}
