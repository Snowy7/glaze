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

pub fn view(app: &App, b: &mut B, c: &mut Col) {
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
                crate::ui::tiles(
                    b,
                    c,
                    key("panel-starters"),
                    2,
                    false,
                    &choices,
                    |b, i, r| super::editors::starter_preview(app, b, i, r),
                );
                notice(
                    b,
                    c,
                    starters
                        .get(starter)
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
    group(b, c, "Recovery", |b, c| {
        notice(
            b,
            c,
            "Ctrl Alt Shift Backspace puts the Windows desktop back, from anywhere.",
        )
    });
}
