use super::{
    controls::{group, notice, row, title},
    editors::toggle,
};
use crate::{
    actions::Action,
    app::{App, Overlay},
    ui::{B, Col},
};
use mypaper_ui::controls::key;
use mypaper_ui::{Rect, controls::Align};

#[derive(Debug, Clone)]
pub struct Baseline {
    docs: [serde_json::Value; 3],
    applied: bool,
}

/// Capture once when this first-run session opens; every choice starts here.
pub fn begin(app: &App) {
    let mut baseline = app.panel.as_ref().unwrap().first_run.borrow_mut();
    baseline.get_or_insert_with(|| Baseline {
        docs: [
            crate::docs::DocId::Config,
            crate::docs::DocId::Bars,
            crate::docs::DocId::Widgets,
        ]
        .map(|doc| app.docs.doc(doc).raw.clone()),
        applied: false,
    });
}

pub fn back(app: &mut App) {
    if let Some(Overlay::FirstRun { step, .. }) = &mut app.overlay {
        *step = step.saturating_sub(1);
    }
}

pub fn run(app: &mut App, action: Action) {
    use crate::docs::{DocId, Op};
    use serde_json::json;
    let Some(Overlay::FirstRun {
        step,
        starter,
        taskbar,
    }) = app.overlay
    else {
        app.panel.as_ref().unwrap().first_run.borrow_mut().take();
        app.overlay = Some(Overlay::FirstRun {
            step: 0,
            starter: 0,
            taskbar: 0,
        });
        begin(app);
        return;
    };
    begin(app);
    let apply = match action {
        Action::FirstRunPick(index) if crate::editing::first_run::panel_starter(index).is_some() => {
            app.overlay = Some(Overlay::FirstRun {
                step,
                starter: index,
                taskbar,
            });
            Some((index, taskbar))
        }
        Action::FirstRunTaskbar(choice) if choice < 3 => {
            app.overlay = Some(Overlay::FirstRun {
                step,
                starter,
                taskbar: choice,
            });
            Some((starter, choice))
        }
        Action::FirstRunNext => {
            if step >= 2 {
                app.overlay = None;
                app.panel.as_ref().unwrap().first_run.borrow_mut().take();
                app.apply(
                    "First run",
                    vec![Op {
                        doc: DocId::Studio,
                        path: "/first_run_done".into(),
                        value: Some(json!(true)),
                    }],
                );
                return;
            }
            app.overlay = Some(Overlay::FirstRun {
                step: step + 1,
                starter,
                taskbar,
            });
            (!app
                .panel
                .as_ref()
                .unwrap()
                .first_run
                .borrow()
                .as_ref()
                .unwrap()
                .applied)
                .then_some((starter, taskbar))
        }
        Action::FirstRunSkip => {
            app.overlay = None;
            app.panel.as_ref().unwrap().first_run.borrow_mut().take();
            app.apply(
                "First run",
                vec![Op {
                    doc: DocId::Studio,
                    path: "/first_run_done".into(),
                    value: Some(json!(true)),
                }],
            );
            return;
        }
        _ => None,
    };
    let Some((starter, taskbar)) = apply else {
        return;
    };
    let (wallpaper, preset, widgets) = crate::editing::first_run::panel_starter(starter).unwrap();
    let mut docs = app
        .panel
        .as_ref()
        .unwrap()
        .first_run
        .borrow()
        .as_ref()
        .unwrap()
        .docs
        .clone();
    let style = ["liquid", "fluent", "oneui", "mono"][starter];
    let mut edits = vec![
        Op {
            doc: DocId::Config,
            path: "/look/style".into(),
            value: Some(json!(style)),
        },
        Op {
            doc: DocId::Bars,
            path: "/preset".into(),
            value: Some(json!(preset)),
        },
        Op {
            doc: DocId::Bars,
            path: "/taskbar/mode".into(),
            value: Some(json!(if taskbar == 1 { "keep" } else { "hide" })),
        },
    ];
    if let Some(wallpaper) = app.library.items.iter().find(|item| item.id == wallpaper) {
        edits.push(Op {
            doc: DocId::Config,
            path: "/theme".into(),
            value: Some(json!(wallpaper.apply_as)),
        });
    }
    for bar in ["top", "dock"] {
        for field in ["design", "enabled"] {
            edits.push(Op {
                doc: DocId::Bars,
                path: format!("/{bar}/{field}"),
                value: None,
            });
        }
        if taskbar != 0 || bar == "dock" && matches!(preset, "macos" | "islands") {
            edits.push(Op {
                doc: DocId::Bars,
                path: format!("/{bar}/enabled"),
                value: Some(json!(taskbar == 0)),
            });
        }
    }
    let monitor = app.display().map_or("", |display| display.device.as_str());
    let mut row = 0;
    let instances: Vec<_> = widgets.iter().enumerate().filter(|_| taskbar != 2).map(|(i, (widget, size))| {
        let instance = json!({"instance":format!("{widget}-{}", i+1),"widget":widget,"monitor":monitor,"cell":[20,row],"size":size});
        row += size[1];
        instance
    }).collect();
    edits.push(Op {
        doc: DocId::Widgets,
        path: "/instances".into(),
        value: Some(json!(instances)),
    });
    for edit in &edits {
        crate::docs::apply_op(
            &mut docs[match edit.doc {
                DocId::Config => 0,
                DocId::Bars => 1,
                _ => 2,
            }],
            edit,
        );
    }
    // Write only the starter's controlled fields, preserving unrelated settings changed elsewhere.
    let mut ops = Vec::new();
    for (doc, document, paths) in [
        (DocId::Config, &docs[0], &["/look/style", "/theme"][..]),
        (
            DocId::Bars,
            &docs[1],
            &[
                "/preset",
                "/taskbar/mode",
                "/top/design",
                "/top/enabled",
                "/dock/design",
                "/dock/enabled",
            ][..],
        ),
        (DocId::Widgets, &docs[2], &["/instances"][..]),
    ] {
        ops.extend(paths.iter().map(|path| Op {
            doc,
            path: (*path).into(),
            value: document.pointer(path).cloned(),
        }));
    }
    app.apply("First-run choice", ops);
    app.panel
        .as_ref()
        .unwrap()
        .first_run
        .borrow_mut()
        .as_mut()
        .unwrap()
        .applied = true;
}

fn contents(index: usize, taskbar: usize) -> String {
    let (_, _, widgets) = crate::editing::first_run::panel_starter(index).unwrap();
    let style = ["Liquid", "Fluent", "One UI", "Mono"][index];
    if taskbar == 2 {
        return format!("Wallpaper · {style}");
    }
    let bars = if taskbar == 1 {
        "Windows"
    } else {
        ["Dock + top", "Taskbar", "Islands", "Top bar"][index]
    };
    let count = widgets.len();
    format!(
        "{bars} · {count} {} · {style}",
        if count == 1 { "widget" } else { "widgets" }
    )
}

/// Tile-sized shell drawings: a visible bar band and the starter's own complete widgets.
fn starter_preview(app: &App, b: &mut B, index: usize, rect: Rect, taskbar: usize) {
    use serde_json::json;
    let Some((wallpaper, preset, widgets)) = crate::editing::first_run::panel_starter(index) else {
        return;
    };
    let style = [
        mypaper_config::look::Style::Liquid,
        mypaper_config::look::Style::Fluent,
        mypaper_config::look::Style::Oneui,
        mypaper_config::look::Style::Mono,
    ][index];
    b.fill(rect, b.px(6.0), b.p.well);
    super::specimens::backdrop(app, b, rect);
    if let Some(image) = app
        .library
        .items
        .iter()
        .find(|item| item.id == wallpaper)
        .and_then(|wallpaper| wallpaper.preview.as_ref().or(wallpaper.video.as_ref()))
        .and_then(|path| {
            app.images.borrow_mut().get(
                path,
                (rect.width().ceil() as u32, rect.height().ceil() as u32),
            )
        })
    {
        crate::stage::image_round(b, rect, &image, 1.0, b.px(6.0));
    }
    if taskbar == 2 {
        return;
    }
    let mut raw = if taskbar == 1 {
        json!({"version":1,"preset":"windows11"})
    } else {
        json!({"version":1,"preset":preset,"look":{"style":style}})
    };
    if taskbar == 0 && matches!(preset, "macos" | "islands") {
        raw["dock"] = json!({"enabled":true});
    }
    raw["dock"]["pinned"] = app
        .get(crate::docs::DocId::Bars, "/dock/pinned")
        .cloned()
        .unwrap_or(json!([]));
    super::specimens::starter_bars(app, b, rect, &raw);
    let width = rect.width() * 0.43;
    for (i, (id, size)) in widgets.iter().take(2).enumerate() {
        let Some(widget) = app.registry.get(id) else {
            continue;
        };
        let mut instance = mypaper_widgets::layout::Instance {
            instance: format!("starter-{index}-{i}"),
            widget: (*id).into(),
            monitor: String::new(),
            cell: [0, 0],
            size: *size,
            settings: Default::default(),
            appearance: Default::default(),
        };
        instance.appearance =
            serde_json::from_value(super::widget_look::appearance(app, &instance, style))
                .unwrap_or_default();
        let r = Rect::new(
            rect.left + rect.width() * (0.05 + i as f32 * 0.47),
            rect.top + rect.height() * 0.23,
            width,
            rect.height() * 0.47,
        );
        super::editors::widgets::face(app, b, &instance, &widget.manifest, "", "", r);
    }
}

pub fn view(app: &App, b: &mut B, c: &mut Col) {
    begin(app);
    let (step, starter, taskbar) = match app.overlay {
        Some(Overlay::FirstRun {
            step,
            starter,
            taskbar,
        }) => (step, starter, taskbar),
        _ => (0, 0, 0),
    };
    if step > 0 {
        let rect = Rect::new(c.x, c.y, b.button_width("Back", true, true).min(c.w), super::controls::height(b, 30.0));
        b.button(
            key("first-run-back"),
            rect,
            "Back",
            Some(crate::theme::glyph::CHEVRON_LEFT),
            mypaper_ui::controls::ButtonKind::Ghost,
            true,
            Action::Panel(super::Command::FirstRunBack),
        );
        c.y = rect.bottom + b.px(4.0);
    }
    match step {
        1 => {
            title(
                b,
                c,
                "Choose a starter",
                "2 of 3 · A starting point you can change",
            );
            group(b, c, "Starter", |b, c| {
                let starters = crate::editing::first_run::panel_starters();
                let choices: Vec<_> = starters
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| (*name, starter == i, Action::FirstRunPick(i)))
                    .collect();
                let details: Vec<_> = (0..starters.len())
                    .map(|index| contents(index, taskbar))
                    .collect();
                super::choosers::tiles(
                    app,
                    b,
                    c,
                    key("panel-starters"),
                    &choices,
                    &details,
                    |b, i, r| starter_preview(app, b, i, r, taskbar),
                );
            });
        }
        0 => {
            title(
                b,
                c,
                "Your taskbar",
                "1 of 3 · Choose what stays on the desktop",
            );
            group(b, c, "", |b, c| {
                for (i, (label, caption)) in [
                    (
                        "Use mypaper's bars",
                        "A dock by default · Windows' taskbar hides",
                    ),
                    (
                        "Keep Windows' taskbar",
                        "Add widgets to your Windows desktop",
                    ),
                    ("Just the wallpaper", "Hide bars and widgets"),
                ]
                .into_iter()
                .enumerate()
                {
                    row(
                        b,
                        c,
                        None,
                        label,
                        "",
                        caption,
                        Some(taskbar == i),
                        Action::FirstRunTaskbar(i),
                    );
                }
            });
        }
        _ => {
            title(b, c, "You're set", "3 of 3 · Your desktop is ready");
            group(b, c, "Startup", |b, c| {
                let on = app.system.autostart.as_ref().is_some_and(|a| a.enabled);
                toggle(
                    b,
                    c,
                    "Start with Windows",
                    "When you sign in",
                    on,
                    Action::ToggleAutostart(!on),
                );
            });
        }
    }
    if step < 2 {
        return;
    }
    group(b, c, "Recovery", |b, c| {
        let style = super::controls::font(b, 12.0, 500);
        let mut x = c.x;
        for label in ["Ctrl", "Alt", "Shift", "Backspace"] {
            let width = b.measure(label, &style).0 + b.px(16.0);
            if x + width > c.right() {
                x = c.x;
                c.y += b.px(32.0);
            }
            let r = Rect::new(x, c.y + b.px(4.0), width, b.px(26.0));
            b.fill(r, b.px(5.0), b.p.fill);
            b.stroke(r, b.px(5.0), b.px(1.0), b.p.edge);
            super::controls::text(b, r, label, 12.0, 500, b.p.ink1, Align::Center);
            x += width + b.px(6.0);
        }
        c.y += b.px(36.0);
        notice(b, c, "Put the Windows desktop back, from anywhere.");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::DocId;

    #[test]
    fn taskbar_precedes_starters_and_every_pick_respects_that_choice() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State {
            page: super::super::Page::FirstRun,
            ..Default::default()
        });
        app.size = (480, 800);
        run(&mut app, Action::FirstRunPick(0));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(
            frame
                .targets
                .iter()
                .any(|target| target.action == Action::FirstRunTaskbar(0))
        );
        assert!(
            !frame
                .targets
                .iter()
                .any(|target| matches!(target.action, Action::FirstRunPick(_)))
        );
        run(&mut app, Action::FirstRunTaskbar(1));
        run(&mut app, Action::FirstRunNext);
        let frame = crate::panel::build(&app, &mut ui.text);
        assert_eq!(
            frame
                .targets
                .iter()
                .filter(|target| matches!(target.action, Action::FirstRunPick(_)))
                .count(),
            4
        );
        for starter in 0..4 {
            run(&mut app, Action::FirstRunPick(starter));
            assert_eq!(
                app.get(DocId::Bars, "/taskbar/mode"),
                Some(&serde_json::json!("keep"))
            );
            for bar in ["top", "dock"] {
                assert_eq!(
                    app.get(DocId::Bars, &format!("/{bar}/enabled")),
                    Some(&serde_json::json!(false))
                );
            }
        }
        run(&mut app, Action::FirstRunTaskbar(2));
        run(&mut app, Action::FirstRunPick(2));
        assert_eq!(
            app.get(DocId::Widgets, "/instances"),
            Some(&serde_json::json!([]))
        );
        assert_eq!(contents(2, 2), "Wallpaper · One UI");
        run(&mut app, Action::FirstRunNext);
        let docs =
            [DocId::Config, DocId::Bars, DocId::Widgets].map(|doc| app.docs.doc(doc).raw.clone());
        for step in [1, 0] {
            let frame = crate::panel::build(&app, &mut ui.text);
            assert!(
                frame
                    .targets
                    .iter()
                    .any(|target| target.action
                        == Action::Panel(super::super::Command::FirstRunBack))
            );
            back(&mut app);
            assert!(
                matches!(app.overlay, Some(Overlay::FirstRun { step: current, .. }) if current == step)
            );
            assert_eq!(
                docs,
                [DocId::Config, DocId::Bars, DocId::Widgets].map(|doc| app
                    .docs
                    .doc(doc)
                    .raw
                    .clone())
            );
        }
    }
}
