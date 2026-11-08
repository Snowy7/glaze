//! Look summaries, style transactions and icon pack editing.

use std::path::PathBuf;

use glaze_bars::icon_pack::{self, IconPack};
use glaze_config::look::Style;
use serde_json::{Value, json};

use crate::actions::{Action, Edit, Page};
use crate::app::App;
use crate::docs::{DocId, Op};

const STYLES: [(Style, &str); 5] = [
    (Style::Liquid, "Liquid"),
    (Style::Oneui, "One UI"),
    (Style::Solid, "Solid"),
    (Style::Fluent, "Fluent"),
    (Style::Mono, "Mono"),
];

/// The user's look style (or the default).
fn style(app: &App) -> Style {
    app.get(DocId::Config, "/look/style")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn style_name(s: Style) -> &'static str {
    STYLES
        .iter()
        .find(|(x, _)| *x == s)
        .map_or("Liquid", |(_, n)| n)
}

/// Home's one line for the look ("Liquid · Dark · accent from wallpaper").
pub fn summary(app: &App) -> String {
    let appearance = match app
        .get(DocId::Config, "/look/appearance")
        .and_then(Value::as_str)
    {
        Some("light") => "Light",
        Some("auto") => "Match Windows",
        _ => "Dark",
    };
    let accent = match app
        .get(DocId::Config, "/look/accent")
        .and_then(Value::as_str)
    {
        Some("auto") | None => "accent from wallpaper",
        Some(_) => "your accent",
    };
    format!("{} · {appearance} · {accent}", style_name(style(app)))
}

/// The active wallpaper, when it's one of yours (its look can be edited in place).
fn own_wallpaper(app: &App) -> Option<&crate::library::Wallpaper> {
    app.library
        .active()
        .map(|i| &app.library.items[i])
        .filter(|w| w.editable())
}

/// Where your icon packs are (`%APPDATA%\glaze\icons`).
fn packs_dir() -> Option<PathBuf> {
    glaze_config::dir().ok().map(|d| d.join("icons"))
}

/// The pack in use, when it's one of your own folders.
fn own_pack(app: &App) -> Option<(PathBuf, IconPack)> {
    let name = app.get(DocId::Config, "/look/text/icon_pack")?.as_str()?;
    let dir = packs_dir()?.join(name);
    IconPack::load(&dir).ok().map(|pack| (dir, pack))
}

/// Writes a pack's manifest (checked first), then has the bars reload it.
fn save_pack(app: &mut App, dir: &std::path::Path, pack: &IconPack) {
    let written = serde_json::to_string_pretty(pack)
        .map_err(anyhow::Error::from)
        .and_then(|text| IconPack::parse(&text).map(|_| text))
        .and_then(|text| {
            std::fs::create_dir_all(dir)?;
            crate::docs::write_atomic(&dir.join(icon_pack::MANIFEST), text.as_bytes())
        })
        .and_then(|()| app.docs.touch(DocId::Bars));
    if let Err(e) = written {
        app.alert(format!("{e:#}"));
    }
    app.dirty = true;
}

/// The four Windows looks, as presets of the Windows layer.
const WINDOWS_LOOKS: [(&str, &str); 4] = [
    (
        "As is",
        r#"{ "accent": null, "border": null, "caption": null, "caption_text": null, "corners": null, "backdrop": null, "apps": "as-is", "cursors": null }"#,
    ),
    (
        "Match Glaze",
        r#"{ "accent": "auto", "border": "auto", "corners": "round", "backdrop": "mica", "apps": "match" }"#,
    ),
    (
        "Glass",
        r#"{ "accent": "auto", "border": null, "corners": "round", "backdrop": "acrylic", "apps": "match" }"#,
    ),
    (
        "Sharp",
        r#"{ "accent": "auto", "border": "auto", "corners": "square", "backdrop": "none", "apps": "match" }"#,
    ),
];

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::LookTab(windows) => {
            app.go(Page::Look);
            app.nav.look_windows = windows;
        }
        Action::IconPackNew => {
            let Some(root) = packs_dir() else { return };
            let Some(name) = (1..100)
                .map(|n| {
                    if n == 1 {
                        "My icons".to_owned()
                    } else {
                        format!("My icons {n}")
                    }
                })
                .find(|n| !root.join(n).exists())
            else {
                return;
            };
            save_pack(
                app,
                &root.join(&name),
                &IconPack {
                    name: name.clone(),
                    mono: false,
                    missing: None,
                    entries: Vec::new(),
                },
            );
            app.set(
                &Edit::new(DocId::Config, "/look/text/icon_pack", "Icon pack"),
                json!(name),
            );
        }
        Action::IconPackAdd => {
            let Some((dir, mut pack)) = own_pack(app) else {
                return;
            };
            let Some(exe) = crate::dialogs::pick(
                app.window,
                "Choose the app",
                false,
                crate::dialogs::PROGRAMS,
            ) else {
                return;
            };
            let Some(picture) =
                crate::dialogs::pick(app.window, "Choose its icon", false, crate::dialogs::ICONS)
            else {
                return;
            };
            let (Some(stem), Some(ext), Some(exe_name)) = (
                exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()),
                picture
                    .extension()
                    .map(|s| s.to_string_lossy().to_lowercase()),
                exe.file_name().map(|s| s.to_string_lossy().to_lowercase()),
            ) else {
                return;
            };
            let icon = format!("{stem}.{ext}");
            if let Err(e) = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::copy(&picture, dir.join(&icon)))
            {
                app.alert(format!("Couldn't copy the picture: {e}"));
                return;
            }
            // By file name, so it matches the app wherever it's installed.
            pack.entries
                .retain(|e| e.path.as_deref() != Some(exe_name.as_str()));
            pack.entries.push(icon_pack::Entry {
                path: Some(exe_name),
                icon,
                ..Default::default()
            });
            save_pack(app, &dir, &pack);
        }
        Action::IconPackRemove(i) => {
            let Some((dir, mut pack)) = own_pack(app) else {
                return;
            };
            if i < pack.entries.len() {
                pack.entries.remove(i);
                save_pack(app, &dir, &pack);
            }
        }
        Action::LookScope(theme) => {
            app.go(Page::Look);
            app.nav.look_theme = theme && own_wallpaper(app).is_some();
            if theme && own_wallpaper(app).is_none() {
                app.nav.look_theme = true;
            }
        }
        Action::StageReal(real) => app.nav.stage_real = real,
        Action::SetStyle(style) => {
            app.apply(
                &format!("{} style", style.map_or("Default", style_name)),
                style_ops(style),
            );
        }
        Action::WindowsLook(i) => {
            let preset: Value = serde_json::from_str(WINDOWS_LOOKS[i].1).unwrap_or_default();
            app.apply(
                &format!("Windows look: {}", WINDOWS_LOOKS[i].0),
                vec![Op {
                    doc: DocId::Config,
                    path: "/look/windows".into(),
                    value: Some(preset),
                }],
            );
        }
        Action::LookReset => {
            let path = if app.nav.look_windows {
                "/look/windows"
            } else {
                "/look"
            };
            let mut ops = vec![Op {
                doc: DocId::Config,
                path: path.into(),
                value: None,
            }];
            if !app.nav.look_windows {
                // Keep the Windows layer: Reset on the Desktop tab is the desktop's.
                if let Some(windows) = app.get(DocId::Config, "/look/windows").cloned() {
                    ops.push(Op {
                        doc: DocId::Config,
                        path: "/look/windows".into(),
                        value: Some(windows),
                    });
                }
            }
            app.apply(
                if app.nav.look_windows {
                    "Reset Windows styling"
                } else {
                    "Reset the look"
                },
                ops,
            );
            app.toast_undo("Back to the wallpaper's look");
        }
        _ => {}
    }
}

/// What picking a style writes. A new style starts from its own material and corners: the
/// sliders and the corner radius reset, so the wallpaper's glass (or the style's) shows. The
/// panel's desktop preview sends these same ops, so hovering a style shows what a click saves.
pub(crate) fn style_ops(style: Option<Style>) -> Vec<Op> {
    let mut ops = vec![Op {
        doc: DocId::Config,
        path: "/look/style".into(),
        value: style.map(|s| json!(s)),
    }];
    for field in ["clarity", "blur", "refraction", "highlight"] {
        ops.push(Op {
            doc: DocId::Config,
            path: format!("/look/material/{field}"),
            value: None,
        });
    }
    ops.push(Op {
        doc: DocId::Config,
        path: "/look/shape/corners".into(),
        value: None,
    });
    ops
}

/// `null` for a Windows field means "As is" (keep Windows' own), which a plain Set can't say
/// (it removes the key): these pseudo-paths write an explicit null.
pub fn intercept_null(app: &mut App, path: &str) -> bool {
    let Some(field) = path.strip_prefix("/windows_null/") else {
        return false;
    };
    app.apply(
        &format!("Windows {field}: as is"),
        vec![Op {
            doc: DocId::Config,
            path: format!("/look/windows/{field}"),
            value: Some(Value::Null),
        }],
    );
    true
}
