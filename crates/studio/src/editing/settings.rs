//! Settings actions, desktop menu transactions and recovery confirmations.

use glaze_config::desktop_menu::{Builtin, Entry, default_items};
use glaze_ui::controls::key;
use serde_json::{Value, json};

use crate::actions::{Action, FieldId};
use crate::app::App;
use crate::docs::{DocId, Op, StartedOver};

fn entries(app: &App) -> Vec<Entry> {
    app.get(DocId::Config, "/desktop_menu/items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(default_items)
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::ToggleAutostart(on) => {
            match crate::system::autostart(if on { "on" } else { "off" }) {
                Ok(state) => app.system.autostart = Some(state),
                Err(e) => app.alert(format!("{e:#}")),
            }
        }
        Action::MenuReset => app.apply(
            "Reset the menu",
            vec![Op {
                doc: DocId::Config,
                path: "/desktop_menu/items".into(),
                value: None,
            }],
        ),
        Action::MenuAdd(what) => {
            let mut items = entries(app);
            match what.as_str() {
                "custom" => {
                    let label = app
                        .text(&FieldId::Text(DocId::Studio, "/menu_label".into()))
                        .trim()
                        .to_owned();
                    let command = app
                        .text(&FieldId::Text(DocId::Studio, "/menu_command".into()))
                        .trim()
                        .to_owned();
                    if label.is_empty() || command.is_empty() {
                        app.alert("Give your item a name and a program");
                        return;
                    }
                    items.push(Entry::Custom(glaze_config::desktop_menu::Custom {
                        label,
                        command,
                        args: vec![],
                    }));
                    app.texts
                        .remove(&FieldId::Text(DocId::Studio, "/menu_label".into()));
                    app.texts
                        .remove(&FieldId::Text(DocId::Studio, "/menu_command".into()));
                }
                id => {
                    let Ok(builtin) = serde_json::from_value::<Builtin>(json!(id)) else {
                        return;
                    };
                    items.push(Entry::Builtin(builtin));
                }
            }
            set_items(app, items, "Add a menu item");
        }
        Action::MenuRemove(i) => {
            let mut items = entries(app);
            if i < items.len() {
                items.remove(i);
                set_items(app, items, "Remove a menu item");
            }
        }
        Action::MenuMove(i, delta) => {
            let mut items = entries(app);
            let to = i as i32 + delta;
            if to >= 0 && (to as usize) < items.len() {
                items.swap(i, to as usize);
                set_items(app, items, "Move a menu item");
                app.input.focus = Some(key(("menu-item", to as usize)));
            }
        }
        Action::RestartStudio => match crate::system::restart_studio() {
            Ok(()) => app.quit = true,
            Err(e) => app.alert(format!("{e:#}")),
        },
        Action::StartOver(doc) => {
            if doc == DocId::Config && app.docs.error(DocId::Config).is_none() {
                // Look's start over: only the look goes (a backup of the file is kept).
                if let Some(path) = app.docs.doc(DocId::Config).path.clone() {
                    let _ = std::fs::copy(&path, path.with_extension("json.bak"));
                }
                app.apply(
                    "Start over: look",
                    vec![Op {
                        doc: DocId::Config,
                        path: "/look".into(),
                        value: None,
                    }],
                );
                app.toast(
                    "Your look is back to the wallpaper's (config.json.bak keeps the old one)",
                    None,
                );
                return;
            }
            let defaults = match doc {
                DocId::Bars => "{ \"version\": 1 }\n".to_owned(),
                // The current layout version: an older one would be upgraded (and refused) again.
                DocId::Widgets => format!(
                    "{{ \"version\": {}, \"instances\": [] }}\n",
                    glaze_widgets::layout::VERSION
                ),
                _ => "{}\n".to_owned(),
            };
            match app
                .docs
                .start_over(doc, &defaults, |backup| confirm_start_over(doc, backup))
            {
                Ok(StartedOver::Declined) => {}
                Ok(StartedOver::Done(backup)) => app.toast(
                    format!(
                        "Started over{}",
                        backup.map_or(String::new(), |b| format!(
                            "; the old file is {}",
                            b.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        ))
                    ),
                    None,
                ),
                Err(e) => app.alert(format!("{e:#}")),
            }
        }
        Action::ShowFile(doc) => {
            if let Some(path) = app.docs.doc(doc).path.clone() {
                crate::system::reveal(&path);
            }
        }
        _ => {}
    }
}

fn set_items(app: &mut App, items: Vec<Entry>, label: &str) {
    let value = serde_json::to_value(items).unwrap_or(Value::Null);
    app.apply(
        label,
        vec![Op {
            doc: DocId::Config,
            path: "/desktop_menu/items".into(),
            value: Some(value),
        }],
    );
}

/// Asks before Start over, in a modal dialog (Cancel is the default): what goes, and where the
/// current file is kept first.
fn confirm_start_over(doc: DocId, backup: Option<&std::path::Path>) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MB_TASKMODAL, MessageBoxW,
    };
    let what = match doc {
        DocId::Widgets => "All your widgets and where they sit are removed.",
        DocId::Bars => "Your bars go back to the defaults.",
        _ => "These settings go back to the defaults.",
    };
    let kept = backup
        .and_then(|b| b.file_name())
        .map(|n| {
            format!(
                "\n\nThe current file is kept first as {}.",
                n.to_string_lossy()
            )
        })
        .unwrap_or_default();
    let text = windows::core::HSTRING::from(format!("Start over?\n\n{what}{kept}"));
    // SAFETY: a modal dialog on this (the UI) thread with owned strings.
    unsafe {
        MessageBoxW(
            None,
            &text,
            windows::core::w!("Start over"),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TASKMODAL,
        ) == IDOK
    }
}
