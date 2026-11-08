//! The command palette (Ctrl K): one field over every setting, wallpaper, widget, integration,
//! action and place. Enter goes there (a setting opens its page and More, scrolls to it and
//! flashes it); Space flips a switch in place. Results are grouped Settings, Actions,
//! Wallpapers, Widgets, Integrations, Go to; with nothing typed, recent ones come first.

use mypaper_ui::controls::Glyph;
use serde_json::{Value, json};

use crate::actions::{Action, Edit, FieldId, Page, Section};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op};
use crate::settings::{Kind, SETTINGS, Setting};
use crate::theme::glyph;

/// One result.
#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub section: String,
    pub glyph: Glyph,
    pub group: &'static str,
    pub action: Action,
    /// A switch setting: Space flips it, and the row shows its state.
    pub toggle: Option<&'static Setting>,
    /// What it's remembered as in Recent.
    pub id: String,
    /// The exact control to scroll to when a panel result opens its page.
    pub reveal: Option<String>,
}

const GROUPS: [&str; 8] = [
    "Settings",
    "On your desktop",
    "Actions",
    "Wallpapers",
    "Widgets",
    "Integrations",
    "History",
    "Go to",
];
const MAX: usize = 9;

fn all(app: &App) -> Vec<Entry> {
    let mut out = Vec::new();
    for s in SETTINGS {
        out.push(Entry {
            title: s.label.into(),
            section: s.place.crumb().into(),
            glyph: setting_glyph(s),
            group: "Settings",
            action: Action::PaletteRun(usize::MAX),
            reveal: None,
            toggle: matches!(s.kind, Kind::Switch { .. }).then_some(s),
            id: format!("setting:{}", s.id),
        });
    }
    let paused = app
        .get(DocId::Config, "/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    for (title, g, action) in [
        (
            if paused {
                "Resume the wallpaper"
            } else {
                "Pause the wallpaper"
            },
            if paused { glyph::PLAY } else { glyph::PAUSE },
            Action::Pause,
        ),
        ("Peek at the desktop", glyph::EYE, Action::Peek),
        (
            "Arrange widgets on the desktop",
            glyph::MOVE,
            Action::ArrangeOnDesktop,
        ),
        ("Add a widget", glyph::ADD, Action::AddWidgetSheet),
        ("Add a wallpaper", glyph::ADD, Action::AddWallpaper),
        ("Restart the shell", glyph::RESET, Action::Restart("shell")),
        (
            "Restart the engine",
            glyph::RESET,
            Action::Restart("engine"),
        ),
        (
            "Restore the Windows desktop",
            glyph::ALERT,
            Action::RestoreDesktop,
        ),
        (
            "Start from a preset",
            glyph::STAR,
            Action::FirstRunPick(usize::MAX),
        ),
        ("Copy diagnostics", glyph::COPY, Action::CopyDiagnostics),
        ("Undo", glyph::UNDO, Action::Undo),
        ("Redo", glyph::REDO, Action::Redo),
    ] {
        if action == Action::ArrangeOnDesktop
            && app.panel.as_ref().is_some_and(|state| state.windowed)
        {
            continue;
        }
        out.push(Entry {
            title: title.into(),
            section: String::new(),
            glyph: g,
            group: "Actions",
            action,
            reveal: None,
            toggle: None,
            id: format!("action:{title}"),
        });
    }
    for (i, w) in app.library.items.iter().enumerate() {
        out.push(Entry {
            title: w.name.clone(),
            section: w.category().into(),
            glyph: glyph::WALLPAPERS,
            group: "Wallpapers",
            action: Action::UseWallpaper(i),
            reveal: None,
            toggle: None,
            id: format!("wallpaper:{}", w.apply_as),
        });
    }
    for (id, installed) in &app.registry.widgets {
        out.push(Entry {
            title: format!("{} widget", installed.manifest.name),
            section: "Add to the desktop".into(),
            glyph: crate::editing::widgets::glyph_of(id),
            group: "Widgets",
            action: Action::AddWidget(id.clone()),
            reveal: None,
            toggle: None,
            id: format!("widget:{id}"),
        });
    }
    for (i, integration) in crate::editing::integrations::LIST.iter().enumerate() {
        out.push(Entry {
            title: integration.name.into(),
            section: integration.adds.into(),
            glyph: integration.glyph,
            group: "Integrations",
            action: Action::SelectIntegration(i),
            reveal: None,
            toggle: None,
            id: format!("integration:{}", integration.name),
        });
    }
    if let Some(panel) = &app.panel {
        let layout = crate::editing::widgets::layout(app);
        for instance in &layout.instances {
            let Some(widget) = app.registry.get(&instance.widget) else {
                continue;
            };
            let monitor = app
                .monitors
                .iter()
                .position(|m| m.device == instance.monitor)
                .unwrap_or(0);
            let location = format!(
                "{} \u{b7} {} \u{d7} {} \u{b7} column {}, row {}",
                crate::panel::display_name(app, monitor),
                instance.size[0],
                instance.size[1],
                instance.cell[0] + 1,
                instance.cell[1] + 1
            );
            out.push(Entry {
                title: widget.manifest.name.clone(),
                section: location.clone(),
                glyph: glyph::WIDGETS,
                group: "On your desktop",
                action: Action::SelectWidget(Some(instance.instance.clone())),
                toggle: None,
                id: format!("desktop-widget:{}", instance.instance),
                reveal: None,
            });
            let values = widget.manifest.resolve_settings(&instance.settings);
            for setting in &widget.manifest.settings {
                let face = [
                    "variant", "face", "style", "layout", "look", "shape", "display", "frame",
                ]
                .into_iter()
                .find_map(|key| values.get(key).and_then(Value::as_str));
                if !setting.visible_on(face, &values) {
                    continue;
                }
                let schema = serde_json::to_value(setting).unwrap_or_default();
                let label = schema["label"]
                    .as_str()
                    .unwrap_or(setting.key())
                    .split(" (")
                    .next()
                    .unwrap_or(setting.key());
                out.push(Entry {
                    title: format!("{} \u{b7} {label}", widget.manifest.name),
                    section: location.clone(),
                    glyph: glyph::WIDGETS,
                    group: "Settings",
                    action: Action::SelectWidget(Some(instance.instance.clone())),
                    toggle: None,
                    id: format!("desktop-setting:{}:{}", instance.instance, setting.key()),
                    reveal: Some(label.into()),
                });
            }
        }
        for (i, plugin) in app.plugins.installed.iter().enumerate() {
            let action = Action::Panel(crate::panel::Command::Connection(
                crate::editing::integrations::LIST.len() + i,
            ));
            out.push(Entry {
                title: plugin.name.clone(),
                section: "Connection".into(),
                glyph: glyph::INTEGRATIONS,
                group: "Integrations",
                action: action.clone(),
                toggle: None,
                id: format!("connection:{}", plugin.id),
                reveal: None,
            });
            for setting in &plugin.settings {
                let Some(key) = setting["key"].as_str() else {
                    continue;
                };
                let label = setting["label"].as_str().unwrap_or(key);
                out.push(Entry {
                    title: format!("{} \u{b7} {label}", plugin.name),
                    section: "Connection options".into(),
                    glyph: glyph::INTEGRATIONS,
                    group: "Integrations",
                    action: action.clone(),
                    toggle: None,
                    id: format!("connection-setting:{}:{key}", plugin.id),
                    reveal: Some(label.into()),
                });
            }
        }
        for (i, item) in crate::editing::integrations::LIST.iter().enumerate() {
            for field in item.fields {
                let crate::editing::integrations::Field::Switch { path, label, .. } = field;
                out.push(Entry {
                    title: format!("{} \u{b7} {label}", item.name),
                    section: "Connection options".into(),
                    glyph: item.glyph,
                    group: "Integrations",
                    action: Action::SelectIntegration(i),
                    toggle: None,
                    id: format!("integration-setting:{path}"),
                    reveal: Some((*label).into()),
                });
            }
        }
        for (i, entry) in panel.history.borrow().entries.iter().enumerate().rev() {
            let summary = if entry.summary.is_empty() {
                &entry.title
            } else {
                &entry.summary
            };
            out.push(Entry {
                title: summary.clone(),
                section: format!(
                    "History \u{b7} {} \u{b7} {}",
                    crate::panel::history_time(entry.at),
                    entry.detail
                ),
                glyph: glyph::CLOCK,
                group: "History",
                action: Action::Panel(crate::panel::Command::History(i)),
                toggle: None,
                id: format!("history:{}:{i}", entry.at),
                reveal: Some(summary.clone()),
            });
        }
    }
    for page in Page::ALL {
        out.push(Entry {
            title: page.title().into(),
            section: "Page".into(),
            glyph: page.glyph(),
            group: "Go to",
            action: Action::Nav(page),
            reveal: None,
            toggle: None,
            id: format!("page:{}", page.key()),
        });
    }
    for section in Section::ALL {
        out.push(Entry {
            title: format!("Settings › {}", section.title()),
            section: String::new(),
            glyph: section.glyph(),
            group: "Go to",
            action: Action::Section(section),
            reveal: None,
            toggle: None,
            id: format!("section:{}", section.title()),
        });
    }
    out
}

fn setting_glyph(s: &Setting) -> Glyph {
    match s.place {
        crate::settings::Place::Look { windows: true } => glyph::APP,
        crate::settings::Place::Look { .. } => glyph::LOOK,
        crate::settings::Place::Bars(_) | crate::settings::Place::BarsPanel(_) => glyph::BARS,
        crate::settings::Place::Settings(section) => section.glyph(),
        crate::settings::Place::Page(page) => page.glyph(),
    }
}

/// How well `entry` matches `query` (higher is better), or `None`.
fn score(entry: &Entry, keywords: &str, query: &str) -> Option<u32> {
    let title = entry.title.to_lowercase();
    if title.starts_with(query) {
        return Some(300);
    }
    if title.split([' ', '›']).any(|w| w.starts_with(query)) {
        return Some(200);
    }
    if title.contains(query) {
        return Some(150);
    }
    let rest = format!("{} {}", entry.section.to_lowercase(), keywords);
    if rest.split(' ').any(|w| w.starts_with(query)) {
        return Some(100);
    }
    rest.contains(query).then_some(50)
}

/// The results for what's typed, in display order.
pub fn results(app: &App) -> Vec<Entry> {
    let query = app.text(&FieldId::Palette).trim().to_lowercase();
    let entries = all(app);
    if query.is_empty() {
        let recent: Vec<String> = app
            .get(DocId::Studio, "/recent")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut out: Vec<Entry> = recent
            .iter()
            .filter_map(|id| entries.iter().find(|e| &e.id == id).cloned())
            .map(|mut e| {
                e.group = "Recent";
                e
            })
            .take(5)
            .collect();
        out.extend(entries.into_iter().filter(|e| e.group == "Go to").take(MAX));
        return out;
    }
    // Every word must match somewhere.
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(usize, u32, Entry)> = entries
        .into_iter()
        .filter_map(|e| {
            let keywords = e.toggle.map_or("", |s| s.keywords).to_owned()
                + SETTINGS
                    .iter()
                    .find(|s| e.id == format!("setting:{}", s.id))
                    .map_or("", |s| s.keywords);
            let mut total = 0;
            for w in &words {
                total += score(&e, &keywords, w)?;
            }
            Some((
                GROUPS.iter().position(|g| *g == e.group).unwrap_or(9),
                total,
                e,
            ))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    // A few per group, so one kind doesn't push out the rest.
    let mut out = Vec::new();
    let mut per_group = std::collections::HashMap::new();
    for (_, _, e) in scored {
        let n = per_group.entry(e.group).or_insert(0);
        if *n < 4 {
            *n += 1;
            out.push(e);
        }
    }
    out.truncate(12);
    out
}

/// Runs result `i` (Enter), or flips it (`Space` on a switch).
pub fn run(app: &mut App, i: usize) {
    let list = results(app);
    let Some(entry) = list.get(i).cloned() else {
        return;
    };
    remember(app, &entry.id);
    app.blur_field();
    app.overlay = None;
    if let Some(id) = entry.id.strip_prefix("setting:") {
        if let Some(s) = Setting::get(id) {
            app.reveal(s);
        }
        return;
    }
    if entry.action == Action::FirstRunPick(usize::MAX) {
        app.overlay = Some(Overlay::FirstRun {
            step: 0,
            starter: None,
            taskbar: 0,
        });
        return;
    }
    app.run(entry.action);
}

/// Space on a focused switch result: flip it, keep the palette open. `false` when the result
/// isn't a switch.
pub fn toggle(app: &mut App, i: usize) -> bool {
    let Some(s) = results(app).get(i).and_then(|e| e.toggle) else {
        return false;
    };
    let on = app.value(s).as_bool().unwrap_or(false);
    app.set(&Edit::new(s.doc, s.path, s.label), json!(!on));
    true
}

fn remember(app: &mut App, id: &str) {
    let mut recent: Vec<Value> = app
        .get(DocId::Studio, "/recent")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    recent.retain(|v| v.as_str() != Some(id));
    recent.insert(0, json!(id));
    recent.truncate(8);
    app.apply(
        "Recent",
        vec![Op {
            doc: DocId::Studio,
            path: "/recent".into(),
            value: Some(Value::Array(recent)),
        }],
    );
}

/// Up and Down in the palette.
pub fn step(app: &mut App, delta: i32) {
    let n = results(app).len().max(1) as i32;
    if let Some(Overlay::Palette { selected }) = &mut app.overlay {
        *selected = (*selected as i32 + delta).rem_euclid(n) as usize;
    }
    app.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{Command, Page as PanelPage, State};
    fn fixture(ui: &mypaper_ui::Ui) -> App {
        let mut app = crate::goldens::app(true, ui);
        app.panel = Some(State::default());
        app.size = (480, 760);
        crate::editing::plugins::fixture(&mut app.plugins);
        app
    }
    #[test]
    fn panel_search_routes_to_the_exact_bar_panel() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        let setting = SETTINGS
            .iter()
            .find(|s| {
                matches!(s.place, crate::settings::Place::BarsPanel(_))
                    && !matches!(s.kind, Kind::Custom)
            })
            .unwrap();
        app.texts.insert(FieldId::Palette, setting.label.into());
        let index = results(&app)
            .iter()
            .position(|entry| entry.id == format!("setting:{}", setting.id))
            .unwrap();
        crate::panel::run(&mut app, Command::Search(index));
        assert_eq!(app.panel.as_ref().unwrap().page, PanelPage::BarPanel);
        let built = crate::panel::build(&app, &mut ui.text);
        assert!(app.anchors.borrow().contains_key(setting.id));
        assert!(!built.targets.is_empty());
    }
    #[test]
    fn panel_search_includes_plugin_connections_desktop_widgets_and_history() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        crate::panel::record_drag(&app, "First desktop");
        let entries = all(&app);
        assert!(entries.iter().any(|e| e.id.starts_with("desktop-widget:")
            && matches!(e.action, Action::SelectWidget(Some(_)))));
        assert!(
            entries
                .iter()
                .any(|e| e.id.starts_with("desktop-setting:") && e.reveal.is_some())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.title == "Docker" && e.id.starts_with("connection:"))
        );
        assert!(entries.iter().any(|e| e.group == "History"));
        let setting = entries
            .iter()
            .find(|e| {
                e.id.starts_with("desktop-setting:clock-")
                    && e.reveal.as_deref() == Some("Show seconds")
            })
            .unwrap_or_else(|| {
                entries
                    .iter()
                    .find(|e| e.id.starts_with("desktop-setting:clock-"))
                    .unwrap()
            });
        app.texts.insert(FieldId::Palette, setting.title.clone());
        let index = results(&app)
            .iter()
            .position(|e| e.id == setting.id)
            .unwrap();
        let version = app.docs.version;
        crate::panel::run(&mut app, Command::Search(index));
        assert_eq!(app.panel.as_ref().unwrap().page, PanelPage::Widget);
        assert_eq!(app.docs.version, version);
        assert!(app.panel.as_ref().unwrap().search_match.is_some());
        crate::panel::build(&app, &mut ui.text);
        assert!(app.anchors.borrow().contains_key("panel-search-match"));
    }
}
