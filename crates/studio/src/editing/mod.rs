//! Shared editing actions, field controls and catalog data. Studio pages live in `panel`.

pub mod bars;
pub mod first_run;
pub mod home;
pub mod integrations;
pub mod look;
pub mod plugins;
pub mod settings;
pub mod wallpapers;
pub mod widgets;

use glaze_ui::Point;

use crate::actions::{Action, Page};
use crate::app::{App, Drag};

/// Actions the app core leaves to the pages.
pub fn run(app: &mut App, action: Action) {
    match &action {
        Action::SelectWallpaper(_)
        | Action::UseWallpaper(_)
        | Action::WallpaperFilter(_)
        | Action::AddWallpaper
        | Action::AddSource(_)
        | Action::AddChoose(_)
        | Action::AddCommit(_)
        | Action::AddBack
        | Action::CopyWallpaper(_)
        | Action::RemoveWallpaper(_)
        | Action::WallpapersOwn => wallpapers::run(app, action),
        Action::LookTab(_)
        | Action::LookScope(_)
        | Action::SetStyle(_)
        | Action::WindowsLook(_)
        | Action::LookReset
        | Action::StageReal(_)
        | Action::IconPackNew
        | Action::IconPackAdd
        | Action::IconPackRemove(_) => look::run(app, action),
        Action::Display(i) => {
            app.nav.display = *i;
            app.dirty = true;
        }
        Action::SelectWidget(_)
        | Action::AddWidgetSheet
        | Action::AddWidget(_)
        | Action::DuplicateWidget
        | Action::RemoveWidget
        | Action::ResizeWidget(..)
        | Action::ArrangeOnDesktop
        | Action::ListAdd(_)
        | Action::ListRemove(..)
        | Action::PlacePick(_)
        | Action::PlaceAuto
        | Action::PickFile(..) => widgets::run(app, action),
        Action::SelectBar(_)
        | Action::SelectModule(_)
        | Action::SelectPanel(_)
        | Action::Preset(_)
        | Action::AddModule(_)
        | Action::InsertModule(..)
        | Action::RemoveModule(_)
        | Action::MoveModule(..)
        | Action::DeleteCustom(_)
        | Action::ResetBar(_)
        | Action::StartBars
        | Action::PinAdd(_)
        | Action::PinRemove(..)
        | Action::PinMove(..)
        | Action::PinsOwn(_) => bars::run(app, action),
        Action::ToggleAutostart(_)
        | Action::MenuReset
        | Action::MenuAdd(_)
        | Action::MenuRemove(_)
        | Action::MenuMove(..)
        | Action::StartOver(_)
        | Action::ShowFile(_)
        | Action::RestartStudio => settings::run(app, action),
        Action::SelectIntegration(i) => {
            app.go(Page::Integrations);
            app.nav.plugins = false;
            app.nav.integration = *i;
        }
        Action::FirstRunPick(_)
        | Action::FirstRunTaskbar(_)
        | Action::FirstRunNext
        | Action::FirstRunSkip => crate::panel::run_first_run(app, action),
        _ => {}
    }
}

/// Edits that aren't a document's: a wallpaper's own `theme.json` (`/theme:<folder>/…`) and
/// the Add wallpaper sheet's choices. Returns whether it handled `path`.
pub fn intercept(app: &mut App, path: &str, value: Option<serde_json::Value>) -> bool {
    if path.starts_with("/plugins/") {
        return plugins::intercept(app, path, value);
    }
    use crate::actions::FieldId;
    use crate::docs::DocId;
    if let Some(rest) = path.strip_prefix("/theme:") {
        let (dir, pointer) = rest.split_once('/').unwrap_or((rest, ""));
        let dir = std::path::PathBuf::from(dir.replace("~1", "/").replace("~0", "~"));
        let pointer = format!("/{pointer}");
        crate::panel::remember_theme_edit(app, path, value.as_ref());
        app.edit_theme("Wallpaper look", &dir, &pointer, value);
        return true;
    }
    if look::intercept_null(app, path) || integrations::intercept(app, path) {
        return true;
    }
    let text = value
        .as_ref()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    match path {
        "/add_module_list" | "/new_custom" | "/menu_label" | "/menu_command" => {
            app.texts
                .insert(FieldId::Text(DocId::Studio, path.into()), text);
        }
        p if p.starts_with("/list_new") => {
            app.texts
                .insert(FieldId::Text(DocId::Studio, p.into()), text);
        }
        "/new_zone" => {
            if !text.trim().is_empty() {
                let mut zones = app
                    .get(DocId::Bars, "/calendar/zones")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                zones.push(serde_json::json!({ "zone": text.trim() }));
                app.apply(
                    "Add a time zone",
                    vec![crate::docs::Op {
                        doc: DocId::Bars,
                        path: "/calendar/zones".into(),
                        value: Some(serde_json::Value::Array(zones)),
                    }],
                );
            }
        }
        "/look_scope" => {
            app.nav.look_theme = value
                .as_ref()
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
        }
        "/add_fit" => {
            app.texts
                .insert(FieldId::Text(DocId::Studio, "/add_fit".into()), text);
        }
        "/add_args_preset" => {
            app.texts.insert(FieldId::AddArgs, text);
        }
        p if p.starts_with("/module_args/") => {
            // A custom module's arguments, typed as one line.
            let module = p.trim_start_matches("/module_args/");
            let args = crate::library::split_args(&text).unwrap_or_default();
            let value = (!args.is_empty()).then(|| serde_json::json!(args));
            app.apply(
                "Arguments",
                vec![crate::docs::Op {
                    doc: DocId::Bars,
                    path: format!("/modules/{module}/args"),
                    value,
                }],
            );
        }
        p if p.starts_with("/app_args/") => {
            let id = p
                .trim_start_matches("/app_args/")
                .replace("~1", "/")
                .replace("~0", "~");
            if let Some(w) = app.library.items.iter().find(|w| w.id == id).cloned() {
                let args = crate::library::split_args(&text).unwrap_or_default();
                let path = format!(
                    "/theme:{}/wallpaper/args",
                    crate::docs::token(&w.dir.display().to_string())
                );
                let value = (!args.is_empty()).then(|| serde_json::json!(args));
                return intercept(app, &path, value);
            }
        }
        _ => return false,
    }
    true
}

/// Esc with nothing open, or a back link: up one level.
pub fn back(app: &mut App) {
    app.dirty = true;
    match app.nav.page() {
        Page::Widgets if app.nav.widget.is_some() => app.nav.widget = None,
        Page::Bars if app.nav.module.is_some() => app.nav.module = None,
        Page::Bars if app.nav.panel.is_some() => app.nav.panel = None,
        Page::Bars if app.nav.bar.is_some() => app.nav.bar = None,
        Page::Wallpapers if app.nav.wallpaper.is_some() => app.nav.wallpaper = None,
        _ => app.input.focus = None,
    }
}

/// Keys a page handles itself (the widget canvas's arrows, Delete). `true` when used.
pub fn key(app: &mut App, vk: u16, shift: bool) -> bool {
    if let Some(crate::app::Overlay::Palette { .. }) = app.overlay {
        match vk {
            0x26 => crate::search::step(app, -1),
            0x28 => crate::search::step(app, 1),
            _ => return false,
        }
        return true;
    }
    match app.nav.page() {
        Page::Widgets => widgets::on_key(app, vk, shift),
        Page::Bars => bars::on_key(app, vk),
        _ => false,
    }
}

pub fn drag_to(app: &mut App, drag: Drag, p: Point) {
    match drag {
        Drag::Widget { .. } => widgets::drag_to(app, drag, p),
        Drag::Module { .. } => bars::drag_to(app, drag, p),
        _ => {}
    }
}

pub fn drop_drag(app: &mut App, drag: Drag) {
    match drag {
        Drag::Widget { .. } => widgets::drop_drag(app, drag),
        Drag::Module { .. } => bars::drop_drag(app, drag),
        _ => {}
    }
}
