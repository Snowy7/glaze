//! The pages: each draws its body (stage and inspector), its page-bar title and actions, and
//! handles the actions and keys that are its own.

pub mod bars;
pub mod first_run;
pub mod home;
pub mod integrations;
pub mod plugins;
pub mod look;
pub mod settings;
pub mod wallpapers;
pub mod widgets;

use mypaper_ui::{Point, Rect};

use crate::actions::{Action, Page};
use crate::app::{App, Drag};
use crate::ui::B;

pub fn view(app: &App, b: &mut B, body: Rect) {
    match app.nav.page() {
        Page::Integrations if app.nav.plugins => plugins::view(app, b, body),
        Page::Home => home::view(app, b, body),
        Page::Wallpapers => wallpapers::view(app, b, body),
        Page::Look => look::view(app, b, body),
        Page::Widgets => widgets::view(app, b, body),
        Page::Bars => bars::view(app, b, body),
        Page::Integrations => integrations::view(app, b, body),
        Page::Settings => settings::view(app, b, body),
    }
}

/// The page bar's title and its one line of context.
pub fn title(app: &App) -> (String, String) {
    match app.nav.page() {
        Page::Integrations if app.nav.plugins => ("Plugins".into(), String::new()),
        Page::Home => home::title(app),
        Page::Wallpapers => wallpapers::title(app),
        Page::Look => ("Look".into(), String::new()),
        Page::Widgets => widgets::title(app),
        Page::Bars => bars::title(app),
        Page::Integrations => integrations::title(app),
        Page::Settings => ("Settings".into(), app.nav.section().title().into()),
    }
}

/// Anything drawn right after the title (Look's scope switch); returns where the detail goes.
pub fn title_extra(app: &App, b: &mut B, rect: Rect) -> f32 {
    match app.nav.page() {
        Page::Integrations => plugins::tabs(app, b, rect),
        Page::Look => look::scope(app, b, rect),
        _ => rect.left,
    }
}

/// The page's own page-bar cluster, right to left from `right`; returns the new right edge.
pub fn bar_actions(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    match app.nav.page() {
        Page::Integrations if app.nav.plugins => plugins::bar(app, b, bar, right),
        Page::Home => home::bar(app, b, bar, right),
        Page::Wallpapers => wallpapers::bar(app, b, bar, right),
        Page::Look => look::bar(app, b, bar, right),
        Page::Widgets => widgets::bar(app, b, bar, right),
        Page::Bars => crate::view::edit_tools(app, b, bar, right),
        Page::Integrations => integrations::bar(app, b, bar, right),
        Page::Settings => right,
    }
}

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
        | Action::IconPackRemove(_) => {
            look::run(app, action)
        }
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
        Action::FirstRunPick(_) | Action::FirstRunTaskbar(_) | Action::FirstRunNext | Action::FirstRunSkip => {
            first_run::run(app, action)
        }
        _ => {}
    }
}

/// Edits that aren't a document's: a wallpaper's own `theme.json` (`/theme:<folder>/…`) and
/// the Add wallpaper sheet's choices. Returns whether it handled `path`.
pub fn intercept(app: &mut App, path: &str, value: Option<serde_json::Value>) -> bool {
    if path.starts_with("/plugins/") { return plugins::intercept(app, path, value); }
    use crate::actions::FieldId;
    use crate::docs::DocId;
    if let Some(rest) = path.strip_prefix("/theme:") {
        let (dir, pointer) = rest.split_once('/').unwrap_or((rest, ""));
        let dir = std::path::PathBuf::from(dir.replace("~1", "/").replace("~0", "~"));
        let pointer = format!("/{pointer}");
        match crate::library::edit_theme(&dir, &pointer, value) {
            Ok(()) => {
                app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
                app.docs.saved = crate::docs::Saved::Done(std::time::Instant::now());
            }
            Err(e) => app.alert(format!("{e:#}")),
        }
        return true;
    }
    if look::intercept_null(app, path) || integrations::intercept(app, path) {
        return true;
    }
    let text = value.as_ref().and_then(|v| v.as_str()).unwrap_or("").to_owned();
    match path {
        "/add_module_list" | "/new_custom" | "/menu_label" | "/menu_command" | "/routine_title" | "/routine_url" => {
            app.texts.insert(FieldId::Text(DocId::Studio, path.into()), text);
        }
        p if p.starts_with("/list_new") => {
            app.texts.insert(FieldId::Text(DocId::Studio, p.into()), text);
        }
        "/new_zone" => {
            if !text.trim().is_empty() {
                let mut zones = app.get(DocId::Bars, "/calendar/zones").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
                zones.push(serde_json::json!({ "zone": text.trim() }));
                app.apply("Add a time zone", vec![crate::docs::Op { doc: DocId::Bars, path: "/calendar/zones".into(), value: Some(serde_json::Value::Array(zones)) }]);
            }
        }
        "/look_scope" => {
            app.nav.look_theme = value.as_ref().and_then(serde_json::Value::as_bool).unwrap_or(false);
        }
        "/add_fit" => {
            app.texts.insert(FieldId::Text(DocId::Studio, "/add_fit".into()), text);
        }
        "/add_args_preset" => {
            app.texts.insert(FieldId::AddArgs, text);
        }
        p if p.starts_with("/module_args/") => {
            // A custom module's arguments, typed as one line.
            let module = p.trim_start_matches("/module_args/");
            let args = crate::library::split_args(&text).unwrap_or_default();
            let value = (!args.is_empty()).then(|| serde_json::json!(args));
            app.apply("Arguments", vec![crate::docs::Op { doc: DocId::Bars, path: format!("/modules/{module}/args"), value }]);
        }
        p if p.starts_with("/app_args/") => {
            let id = p.trim_start_matches("/app_args/").replace("~1", "/").replace("~0", "~");
            if let Some(w) = app.library.items.iter().find(|w| w.id == id).cloned() {
                let args = crate::library::split_args(&text).unwrap_or_default();
                let path = format!("/theme:{}/wallpaper/args", crate::docs::token(&w.dir.display().to_string()));
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

/// The stage this page shows (its frame in window pixels, and what it shows), if any.
pub fn stage(app: &App) -> Option<(Rect, crate::stage::Show)> {
    use crate::stage::Show;
    let body = crate::view::regions(app).body;
    match app.nav.page() {
        Page::Home => {
            let area = home::stage_area(app, body);
            let f = crate::stage::frame(app, area);
            Some((Rect::new(area.left, area.top, f.width(), f.height()), Show::Desktop))
        }
        Page::Look => Some((stage_in(app, body), if app.nav.stage_real { Show::Desktop } else { Show::Specimen })),
        Page::Widgets => Some((stage_in(app, body), Show::Widgets)),
        Page::Bars => Some((stage_in(app, body), Show::Bars)),
        _ => None,
    }
}

/// A stage's frame in the left part of a split page, leaving room for the chips under it.
pub fn stage_in(app: &App, body: Rect) -> Rect {
    let s = app.scale;
    let (left, _) = crate::view::split(app, body);
    let area = Rect::new(left.left + 24.0 * s, left.top + 24.0 * s, left.width() - 48.0 * s, left.height() - 48.0 * s - 44.0 * s);
    crate::stage::frame(app, area)
}

/// The display chips under a stage: one per display, the current one lit. Returns the right
/// edge, for a hint after them.
pub fn display_chips(app: &App, b: &mut B, x: f32, y: f32) -> f32 {
    use mypaper_ui::controls::{Align, Font, key};
    let p = *b.p;
    let mut x = x;
    let current = app.display().map(|m| m.device.clone());
    let caption = b.font(Font::Caption);
    b.group(key("display-chips"), |b| {
        for (i, m) in app.monitors.iter().enumerate() {
            let on = current.as_deref() == Some(m.device.as_str());
            let label = if on {
                format!("Display {} · {}×{}", i + 1, m.rect.width, m.rect.height)
            } else {
                format!("Display {}", i + 1)
            };
            let (tw, _) = b.measure(&label, &caption);
            let r = Rect::new(x, y, tw + b.px(40.0), b.px(28.0));
            let k = key(("display", i));
            b.fill(r, r.height() / 2.0, if on { p.selected } else { p.fill });
            b.wash(k, r, r.height() / 2.0);
            b.icon(Rect::new(r.left + b.px(10.0), r.top, b.px(14.0), r.height()), crate::theme::glyph::MONITOR, 12.0, if on { p.ink1 } else { p.ink2 });
            b.text_in(Rect::new(r.left + b.px(31.0), r.top, tw + b.px(2.0), r.height()), &label, &caption, if on { p.ink1 } else { p.ink2 }, Align::Start);
            b.hit_checked(k, r, mypaper_ui::Role::RadioButton, &label, Some(on), Action::Display(i), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, r.height() / 2.0);
            x = r.right + b.px(10.0);
        }
    });
    x
}
