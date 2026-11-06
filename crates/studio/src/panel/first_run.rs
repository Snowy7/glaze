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
        docs: [crate::docs::DocId::Config, crate::docs::DocId::Bars, crate::docs::DocId::Widgets]
            .map(|doc| app.docs.doc(doc).raw.clone()),
        applied: false,
    });
}

pub fn run(app: &mut App, action: Action) {
    use crate::docs::{DocId, Op};
    use serde_json::json;
    let Some(Overlay::FirstRun { step, starter, taskbar }) = app.overlay else {
        app.panel.as_ref().unwrap().first_run.borrow_mut().take();
        app.overlay = Some(Overlay::FirstRun { step: 0, starter: 0, taskbar: 0 });
        begin(app);
        return;
    };
    begin(app);
    let apply = match action {
        Action::FirstRunPick(index) if crate::pages::first_run::panel_starter(index).is_some() => {
            app.overlay = Some(Overlay::FirstRun { step, starter: index, taskbar });
            Some((index, taskbar))
        }
        Action::FirstRunTaskbar(choice) if choice < 3 => {
            app.overlay = Some(Overlay::FirstRun { step, starter, taskbar: choice });
            Some((starter, choice))
        }
        Action::FirstRunNext => {
            if step >= 2 {
                app.overlay = None;
                app.panel.as_ref().unwrap().first_run.borrow_mut().take();
                app.apply("First run", vec![Op { doc: DocId::Studio, path: "/first_run_done".into(), value: Some(json!(true)) }]);
                return;
            }
            app.overlay = Some(Overlay::FirstRun { step: step + 1, starter, taskbar });
            (!app.panel.as_ref().unwrap().first_run.borrow().as_ref().unwrap().applied)
                .then_some((starter, taskbar))
        }
        Action::FirstRunSkip => {
            app.overlay = None;
            app.panel.as_ref().unwrap().first_run.borrow_mut().take();
            app.apply("First run", vec![Op { doc: DocId::Studio, path: "/first_run_done".into(), value: Some(json!(true)) }]);
            return;
        }
        _ => None,
    };
    let Some((starter, taskbar)) = apply else { return };
    let (wallpaper, preset, widgets) = crate::pages::first_run::panel_starter(starter).unwrap();
    let mut docs = app.panel.as_ref().unwrap().first_run.borrow().as_ref().unwrap().docs.clone();
    let style = ["liquid", "fluent", "oneui", "mono"][starter];
    let mut edits = vec![
        Op { doc: DocId::Config, path: "/look/style".into(), value: Some(json!(style)) },
        Op { doc: DocId::Bars, path: "/preset".into(), value: Some(json!(preset)) },
        Op { doc: DocId::Bars, path: "/taskbar/mode".into(), value: Some(json!(if taskbar == 1 { "keep" } else { "hide" })) },
    ];
    if let Some(wallpaper) = app.library.items.iter().find(|item| item.id == wallpaper) {
        edits.push(Op { doc: DocId::Config, path: "/theme".into(), value: Some(json!(wallpaper.apply_as)) });
    }
    for bar in ["top", "dock"] {
        for field in ["design", "enabled"] {
            edits.push(Op { doc: DocId::Bars, path: format!("/{bar}/{field}"), value: None });
        }
        if taskbar != 0 || bar == "dock" && matches!(preset, "macos" | "islands") {
            edits.push(Op { doc: DocId::Bars, path: format!("/{bar}/enabled"), value: Some(json!(taskbar == 0)) });
        }
    }
    let monitor = app.display().map_or("", |display| display.device.as_str());
    let mut row = 0;
    let instances: Vec<_> = widgets.iter().enumerate().filter(|_| taskbar != 2).map(|(i, (widget, size))| {
        let instance = json!({"instance":format!("{widget}-{}", i+1),"widget":widget,"monitor":monitor,"cell":[20,row],"size":size});
        row += size[1];
        instance
    }).collect();
    edits.push(Op { doc: DocId::Widgets, path: "/instances".into(), value: Some(json!(instances)) });
    for edit in &edits {
        crate::docs::apply_op(&mut docs[match edit.doc { DocId::Config => 0, DocId::Bars => 1, _ => 2 }], edit);
    }
    // Write only the starter's controlled fields, preserving unrelated settings changed elsewhere.
    let mut ops = Vec::new();
    for (doc, document, paths) in [
        (DocId::Config, &docs[0], &["/look/style", "/theme"][..]),
        (DocId::Bars, &docs[1], &["/preset", "/taskbar/mode", "/top/design", "/top/enabled", "/dock/design", "/dock/enabled"][..]),
        (DocId::Widgets, &docs[2], &["/instances"][..]),
    ] {
        ops.extend(paths.iter().map(|path| Op { doc, path: (*path).into(), value: document.pointer(path).cloned() }));
    }
    app.apply("First-run choice", ops);
    app.panel.as_ref().unwrap().first_run.borrow_mut().as_mut().unwrap().applied = true;
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
    match step {
        0 => {
            title(
                b,
                c,
                "Make this desktop yours",
                "1 of 3 · A starting point you can change",
            );
            group(b, c, "Starter", |b, c| {
                let starters = crate::pages::first_run::panel_starters();
                let choices: Vec<_> = starters
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| (*name, starter == i, Action::FirstRunPick(i)))
                    .collect();
                let shown = super::choosers::draw(
                    app,
                    b,
                    c,
                    key("panel-starters"),
                    b.px(196.0),
                    &choices,
                    |b, i, r| super::editors::starter_preview(app, b, i.unwrap_or(starter), r),
                );
                notice(
                    b,
                    c,
                    starters
                        .get(shown.unwrap_or(starter))
                        .map_or("Choose a starter", |(_, line)| *line),
                );
            });
        }
        1 => {
            title(
                b,
                c,
                "Your taskbar",
                "2 of 3 · Choose what stays on the desktop",
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
    if step < 2 { return; }
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
