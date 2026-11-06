//! Wallpaper creation, selection, copying and removal.

use mypaper_theme::Fit;
use serde_json::{Value, json};

use crate::actions::{Action, FieldId, Page, Source};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op};
use crate::library::NewWallpaper;

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::SelectWallpaper(i) => {
            app.go(Page::Wallpapers);
            app.nav.wallpaper = Some(i);
        }
        Action::UseWallpaper(i) => use_wallpaper(app, i),
        Action::WallpaperFilter(f) => app.nav.filter = f,
        Action::WallpapersOwn => {
            app.go(Page::Wallpapers);
            app.nav.filter = 3;
        }
        Action::AddWallpaper => {
            app.go(Page::Wallpapers);
            app.overlay = Some(Overlay::AddWallpaper {
                source: None,
                picked: None,
            });
        }
        Action::AddSource(source) => {
            if source == Source::Web {
                app.overlay = Some(Overlay::AddWallpaper {
                    source: Some(source),
                    picked: None,
                });
                app.focus_field(FieldId::AddUrl);
                return;
            }
            let (title, folder, filters) = match source {
                Source::File => (
                    "Choose a picture, video, shader or 3D scene",
                    false,
                    crate::dialogs::WALLPAPER_FILES,
                ),
                Source::Folder => ("Choose a folder of pictures", true, &[][..]),
                Source::App => ("Choose a program", false, crate::dialogs::PROGRAMS),
                Source::Web => unreachable!(),
            };
            if let Some(path) = crate::dialogs::pick(app.window, title, folder, filters) {
                let picked = path.display().to_string();
                app.texts
                    .insert(FieldId::AddName, crate::library::suggested_name(&picked));
                app.overlay = Some(Overlay::AddWallpaper {
                    source: Some(source),
                    picked: Some(picked),
                });
            }
        }
        Action::AddChoose(_) => {
            if let Some(path) = crate::dialogs::pick(
                app.window,
                "Choose a web page",
                false,
                crate::dialogs::PAGES,
            ) {
                let picked = path.display().to_string();
                app.texts
                    .insert(FieldId::AddName, crate::library::suggested_name(&picked));
                app.overlay = Some(Overlay::AddWallpaper {
                    source: Some(Source::Web),
                    picked: Some(picked),
                });
            }
        }
        Action::AddBack => {
            app.overlay = Some(Overlay::AddWallpaper {
                source: None,
                picked: None,
            })
        }
        Action::AddCommit(use_it) => add(app, use_it),
        Action::CopyWallpaper(i) => copy(app, i),
        Action::RemoveWallpaper(i) => remove(app, i),
        _ => {}
    }
}

/// Uses wallpaper `i`: the config's theme, one undoable step, and a toast with Undo.
pub fn use_wallpaper(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else {
        return;
    };
    if w.error.is_some() {
        app.alert(format!("{} can't load", w.name));
        return;
    }
    if w.active {
        return;
    }
    app.apply(
        &format!("Use {}", w.name),
        vec![Op {
            doc: DocId::Config,
            path: "/theme".into(),
            value: Some(json!(w.apply_as)),
        }],
    );
    let mut recent: Vec<Value> = app
        .get(DocId::Studio, "/recent_wallpapers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    recent.retain(|v| v.as_str() != Some(&w.apply_as));
    recent.insert(0, json!(w.apply_as));
    recent.truncate(8);
    app.apply(
        "Recent",
        vec![Op {
            doc: DocId::Studio,
            path: "/recent_wallpapers".into(),
            value: Some(Value::Array(recent)),
        }],
    );
    let text = if w.brings_look {
        format!("Using {} and its look", w.name)
    } else {
        format!("Using {}", w.name)
    };
    app.toast_undo(text);
}

fn add(app: &mut App, use_it: bool) {
    let Some(Overlay::AddWallpaper { source, picked }) = app.overlay.clone() else {
        return;
    };
    let input = match source {
        Some(Source::Web) => picked
            .clone()
            .unwrap_or_else(|| app.text(&FieldId::AddUrl).trim().to_owned()),
        _ => picked.clone().unwrap_or_default(),
    };
    if input.is_empty() {
        app.alert("Type a web address first");
        return;
    }
    if source == Some(Source::Web) && picked.is_none() && app.text(&FieldId::AddName).is_empty() {
        // The address was typed: go on to the details.
        app.texts
            .insert(FieldId::AddName, crate::library::suggested_name(&input));
        app.overlay = Some(Overlay::AddWallpaper {
            source,
            picked: Some(input),
        });
        app.blur_field();
        return;
    }
    let fit = match app.text(&FieldId::Text(DocId::Studio, "/add_fit".into())) {
        "contain" => Fit::Contain,
        "stretch" => Fit::Stretch,
        "center" => Fit::Center,
        _ => Fit::Cover,
    };
    let new = NewWallpaper {
        source: input,
        name: app.text(&FieldId::AddName).to_owned(),
        fit,
        args: app.text(&FieldId::AddArgs).to_owned(),
    };
    let root = match mypaper_config::user_themes_dir() {
        Ok(root) => root,
        Err(e) => return app.alert(format!("{e:#}")),
    };
    match crate::library::create_in(&root, &new) {
        Ok(dir) => {
            app.library =
                crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.overlay = None;
            app.blur_field();
            for id in [FieldId::AddUrl, FieldId::AddName, FieldId::AddArgs] {
                app.texts.remove(&id);
            }
            if let Some(i) = app.library.by_dir(&dir) {
                app.nav.wallpaper = Some(i);
                app.nav.filter = 0;
                if use_it {
                    use_wallpaper(app, i);
                } else {
                    app.toast(format!("Added {}", new.name.trim()), None);
                }
            }
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

fn copy(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else {
        return;
    };
    let root = match mypaper_config::user_themes_dir() {
        Ok(root) => root,
        Err(e) => return app.alert(format!("{e:#}")),
    };
    match crate::library::duplicate_into(&root, &w.dir) {
        Ok(dir) => {
            app.library =
                crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.nav.wallpaper = app.library.by_dir(&dir);
            app.toast(format!("Made {} (copy): yours to change", w.name), None);
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

fn remove(app: &mut App, i: usize) {
    let Some(w) = app.library.items.get(i).cloned() else {
        return;
    };
    if !w.editable() || w.active {
        return;
    }
    match crate::library::recycle(&w.dir) {
        Ok(()) => {
            app.library =
                crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.nav.wallpaper = None;
            app.toast(
                format!("Moved {} to the Recycle Bin", w.name),
                Some(("Show".into(), Action::Open("shell:RecycleBinFolder".into()))),
            );
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}
