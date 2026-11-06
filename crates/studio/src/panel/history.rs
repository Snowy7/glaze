//! Desktop snapshots. Older Studio loads and watches an explicit filename allowlist
//! (`docs::paths`, `watch::start`), so it ignores our versioned file beside config.json.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;
use crate::docs::{DocId, Op, apply_op};

const SETTINGS_IN_BARS: [&str; 2] = ["/performance", "/animation/reduceMotion"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desktop {
    config: Vec<(String, Option<Value>)>,
    bars: Value,
    widgets: Value,
}

impl Desktop {
    pub fn wallpaper(&self) -> Option<&str> {
        self.config
            .iter()
            .find(|(path, _)| path == "/theme")
            .and_then(|(_, v)| v.as_ref())
            .and_then(Value::as_str)
    }
    pub fn bars(&self) -> &Value {
        &self.bars
    }
    pub fn widgets(&self) -> &Value {
        &self.widgets
    }

    fn read(app: &App) -> Self {
        let mut bars = app.docs.doc(DocId::Bars).raw.clone();
        for path in SETTINGS_IN_BARS {
            apply_op(
                &mut bars,
                &Op {
                    doc: DocId::Bars,
                    path: path.into(),
                    value: None,
                },
            );
        }
        Self {
            config: ["/theme", "/look", "/desktop_menu"]
                .into_iter()
                .map(|path| {
                    (
                        path.into(),
                        app.docs.doc(DocId::Config).raw.pointer(path).cloned(),
                    )
                })
                .collect(),
            bars,
            widgets: app.docs.doc(DocId::Widgets).raw.clone(),
        }
    }

    fn ops(&self, app: &App) -> Vec<Op> {
        // Keep today's Settings and Connections, and avoid projecting Look over saved bars.
        let mut config = app.docs.doc(DocId::Config).raw.clone();
        for (path, value) in &self.config {
            apply_op(
                &mut config,
                &Op {
                    doc: DocId::Config,
                    path: path.clone(),
                    value: value.clone(),
                },
            );
        }
        let mut bars = self.bars.clone();
        for path in SETTINGS_IN_BARS {
            apply_op(
                &mut bars,
                &Op {
                    doc: DocId::Bars,
                    path: path.into(),
                    value: app.docs.doc(DocId::Bars).raw.pointer(path).cloned(),
                },
            );
        }
        vec![
            Op {
                doc: DocId::Config,
                path: String::new(),
                value: Some(config),
            },
            Op {
                doc: DocId::Bars,
                path: String::new(),
                value: Some(bars),
            },
            Op {
                doc: DocId::Widgets,
                path: String::new(),
                value: Some(self.widgets.clone()),
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub summary: String,
    pub at: u64,
    pub desktop: Desktop,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u8,
    entries: Vec<Entry>,
}

#[derive(Debug, Default)]
pub struct History {
    pub entries: Vec<Entry>,
    pub selected: Option<usize>,
    pub error: Option<String>,
    last: Option<Desktop>,
    loaded: bool,
    last_pick: Option<(String, usize)>,
}

impl History {
    pub fn end_picks(&mut self) {
        self.last_pick = None;
    }
}

pub(super) fn path(app: &App) -> Option<PathBuf> {
    (!app.fixture)
        .then(|| {
            app.docs
                .doc(DocId::Config)
                .path
                .as_ref()
                .map(|p| p.with_file_name("studio-panel-history.json"))
        })
        .flatten()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load(app: &App, history: &mut History) {
    if history.loaded {
        return;
    }
    history.loaded = true;
    let Some(path) = path(app) else { return };
    let bytes = match std::fs::read(path) {
        Ok(bytes) if bytes.len() <= 8 << 20 => bytes,
        Ok(_) => {
            history.error =
                Some("History is too large to read. It has been left unchanged.".into());
            return;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("history read: {e}");
            history.error =
                Some("Can't open studio-panel-history.json. Check its permissions.".into());
            return;
        }
    };
    match serde_json::from_slice::<File>(&bytes) {
        Ok(file) if file.version == 1 => history.entries = file.entries,
        Ok(file) if file.version > 1 => history.error = Some(crate::docs::NEWER.into()),
        Ok(_) => history.error = Some("Can't read studio-panel-history.json, line 1".into()),
        Err(error) if error.to_string().contains("unknown field") => {
            history.error = Some(crate::docs::NEWER.into())
        }
        Err(error) => {
            history.error = Some(format!(
                "Can't read studio-panel-history.json, line {}",
                error.line().max(1)
            ))
        }
    }
    let days = app
        .get(DocId::Studio, "/panel_history_days")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 365);
    let cutoff = now().saturating_sub(days * 86400);
    history.entries.retain(|entry| entry.at >= cutoff);
    if history.entries.len() > 100 {
        history.entries.drain(..history.entries.len() - 100);
    }
}

fn persist(app: &App, history: &mut History) {
    if history.error.is_some() {
        return;
    }
    let Some(path) = path(app) else { return };
    let bytes = serde_json::to_vec(&File {
        version: 1,
        entries: history.entries.clone(),
    });
    match bytes
        .map_err(anyhow::Error::from)
        .and_then(|bytes| crate::docs::write_atomic(&path, &bytes))
    {
        Ok(()) => {}
        Err(error) => {
            log::warn!("history save: {error}");
            history.error = Some("Couldn't save".into());
        }
    }
}

fn capture(app: &App, label: Option<&str>, merge: bool) {
    let Some(state) = &app.panel else { return };
    if app.drag.is_some() {
        return;
    }
    let desktop = Desktop::read(app);
    let mut history = state.history.borrow_mut();
    load(app, &mut history);
    if history.last.as_ref() == Some(&desktop) {
        return;
    }
    let mut changed = Vec::new();
    if let Some(last) = &history.last {
        for ((path, value), (_, before)) in desktop.config.iter().zip(&last.config) {
            if value != before {
                changed.push(match path.as_str() {
                    "/theme" => "Wallpaper",
                    "/look" => "Look",
                    _ => "Desktop menu",
                });
            }
        }
        if desktop.bars != last.bars {
            changed.push("Bars");
        }
        if desktop.widgets != last.widgets {
            changed.push("Widgets");
        }
    }
    let title = label.map(str::to_owned).unwrap_or_else(|| {
        if changed.is_empty() {
            "When Studio opened".into()
        } else {
            changed.join(" · ")
        }
    });
    let detail = app.library.active().map_or("Your desktop".to_owned(), |i| {
        app.library.items[i].name.clone()
    });
    let before = if merge && history.entries.len() > 1 {
        history
            .entries
            .get(history.entries.len() - 2)
            .map(|e| &e.desktop)
    } else {
        history.last.as_ref()
    };
    let summary = before.map_or_else(
        || "When Studio opened".into(),
        |before| summary(app, before, &desktop),
    );
    if merge && history.entries.len() > 1 {
        history.entries.pop();
    }
    if history
        .entries
        .last()
        .is_none_or(|entry| entry.desktop != desktop)
    {
        history.entries.push(Entry {
            title,
            detail,
            summary,
            at: now(),
            desktop: desktop.clone(),
        });
    }
    if history.entries.len() > 100 {
        history.entries.remove(0);
    }
    history.last = Some(desktop);
    // Opening a panel or rendering fixtures never writes a file.
    if label.is_some() {
        persist(app, &mut history);
    }
}

fn summary(app: &App, before: &Desktop, after: &Desktop) -> String {
    let value = |desktop: &Desktop, path: &str| {
        desktop
            .config
            .iter()
            .find(|(key, _)| key == path)
            .and_then(|(_, v)| v.as_ref())
            .cloned()
            .unwrap_or(Value::Null)
    };
    if before.wallpaper() != after.wallpaper() {
        let name = |id: Option<&str>| {
            app.library
                .items
                .iter()
                .find(|w| Some(w.apply_as.as_str()) == id || Some(w.id.as_str()) == id)
                .map_or("Your desktop", |w| w.name.as_str())
        };
        return format!(
            "Wallpaper: {} → {}",
            name(before.wallpaper()),
            name(after.wallpaper())
        );
    }
    let (old_look, new_look) = (value(before, "/look"), value(after, "/look"));
    if old_look != new_look {
        let name = |look: &Value| match look
            .get("style")
            .and_then(Value::as_str)
            .unwrap_or("liquid")
        {
            "oneui" => "One UI",
            "fluent" => "Fluent",
            "solid" => "Solid",
            "mono" => "Mono",
            _ => "Liquid",
        };
        if name(&old_look) != name(&new_look) {
            return format!("Look: {} → {}", name(&old_look), name(&new_look));
        }
        if old_look.get("windows") != new_look.get("windows") {
            return "Windows look changed".into();
        }
        if old_look.get("material") != new_look.get("material") {
            return "Look: material changed".into();
        }
        return "Look: color, text or shape changed".into();
    }
    if before.bars != after.bars {
        let name = |bars: &Value| {
            crate::pages::bars::PRESETS
                .iter()
                .find(|(id, _)| bars.get("preset").and_then(Value::as_str) == Some(id))
                .map_or("Custom", |(_, name)| *name)
        };
        if name(&before.bars) != name(&after.bars) {
            return format!("Bars: {} → {}", name(&before.bars), name(&after.bars));
        }
        for (key, label) in [("top", "Top bar"), ("dock", "Dock")] {
            if before.bars.get(key) != after.bars.get(key) {
                return format!("{label} changed");
            }
        }
        return "Bar modules or panels changed".into();
    }
    if before.widgets != after.widgets {
        let old = before
            .widgets
            .get("instances")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let new = after
            .widgets
            .get("instances")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let widget_name = |instance: &Value| {
            app.registry
                .get(instance["widget"].as_str().unwrap_or(""))
                .map_or("Widget", |w| w.manifest.name.as_str())
        };
        if new.len() > old.len()
            && let Some(item) = new
                .iter()
                .find(|n| !old.iter().any(|o| o["instance"] == n["instance"]))
        {
            return format!("Added {}", widget_name(item));
        }
        if new.len() < old.len()
            && let Some(item) = old
                .iter()
                .find(|o| !new.iter().any(|n| n["instance"] == o["instance"]))
        {
            return format!("Removed {}", widget_name(item));
        }
        if let Some((old, new)) = old.iter().zip(new).find(|(o, n)| o != n) {
            let name = widget_name(new);
            if old["size"] != new["size"] {
                return format!("Resized {name}");
            }
            if old["cell"] != new["cell"] {
                return format!("Moved {name}");
            }
            if let Some(widget) = app.registry.get(new["widget"].as_str().unwrap_or("")) {
                let old_values = widget
                    .manifest
                    .resolve_settings(&old["settings"].as_object().cloned().unwrap_or_default());
                let new_values = widget
                    .manifest
                    .resolve_settings(&new["settings"].as_object().cloned().unwrap_or_default());
                for setting in &widget.manifest.settings {
                    let key = setting.key();
                    if old_values.get(key) == new_values.get(key) {
                        continue;
                    }
                    if let mypaper_widgets::manifest::Setting::Select { options, .. } = setting {
                        let label = |value: Option<&Value>| {
                            let value = value.and_then(Value::as_str).unwrap_or("");
                            options
                                .iter()
                                .find_map(|o| match o {
                                    mypaper_widgets::manifest::SelectOption::Plain(v)
                                        if v == value =>
                                    {
                                        Some(v.as_str())
                                    }
                                    mypaper_widgets::manifest::SelectOption::Detailed(v)
                                        if v.value == value =>
                                    {
                                        Some(v.label.as_deref().unwrap_or(&v.value))
                                    }
                                    _ => None,
                                })
                                .unwrap_or(value)
                                .to_owned()
                        };
                        return format!(
                            "{name}: {} → {}",
                            label(old_values.get(key)),
                            label(new_values.get(key))
                        );
                    }
                    let schema = serde_json::to_value(super::options::presentation(setting))
                        .unwrap_or_default();
                    return format!(
                        "{name}: {} changed",
                        schema["label"].as_str().unwrap_or("options")
                    );
                }
            }
            return format!("{name}: look changed");
        }
        return "Widgets changed".into();
    }
    if value(before, "/desktop_menu") != value(after, "/desktop_menu") {
        return "Desktop menu changed".into();
    }
    "Desktop restored".into()
}

pub fn time_label(at: u64) -> String {
    let stamp = mypaper_shell::preview::widgets::Stamp::at((at.min(253402300799) * 1000) as i64);
    format!("{:02}:{:02}", stamp.hour, stamp.minute)
}

pub fn observe(app: &App) {
    capture(app, None, false);
}

pub fn record_drag(app: &App, label: &str) {
    capture(app, Some(label), false);
}

pub fn record_edit(app: &mut App, label: &str, ops: &[Op], checkpoint: usize) {
    if app.panel.is_none() {
        return;
    }
    if ops.iter().any(|op| op.doc == DocId::Plugins) {
        app.panel.as_ref().unwrap().history.borrow_mut().last = Some(Desktop::read(app));
        return;
    }
    if ops.iter().all(|op| op.doc == DocId::Studio) {
        return;
    }
    let pick = ops.iter().find_map(|op| match (op.doc, op.path.as_str()) {
        (DocId::Config, "/theme" | "/look/style" | "/look/windows") => Some(op.path.clone()),
        (DocId::Config, path)
            if path.starts_with("/look/material/")
                || matches!(path, "/look/appearance" | "/look/accent")
                || path.starts_with("/look/") && path.ends_with("/style") =>
        {
            Some(path.into())
        }
        (DocId::Bars, "/preset" | "/top/design" | "/dock/design") => Some(op.path.clone()),
        (DocId::Bars, path)
            if path.starts_with("/overrides/")
                && (path.ends_with("/design") || path.ends_with("/preset")) =>
        {
            Some(path.into())
        }
        (DocId::Widgets, path)
            if path.ends_with("/variant")
                || path.ends_with("/face")
                || path.ends_with("/layout")
                || path.ends_with("/style")
                || path.ends_with("/look")
                || path.ends_with("/shape")
                || path.ends_with("/display")
                || path.ends_with("/frame")
                || path.ends_with("/appearance/surface")
                || path.ends_with("/appearance/tint") =>
        {
            Some(path.into())
        }
        _ => None,
    });
    let last = app
        .panel
        .as_ref()
        .unwrap()
        .history
        .borrow()
        .last_pick
        .clone();
    let merge = pick.as_ref().is_some_and(|pick| {
        last.as_ref()
            .is_some_and(|(key, count)| key == pick && *count == checkpoint)
    }) && app.docs.coalesce_last_pick(checkpoint);
    if pick.is_some() {
        super::pick_client::record_pick(app, label, ops, checkpoint, merge);
    }
    app.panel.as_ref().unwrap().history.borrow_mut().last_pick =
        pick.map(|key| (key, app.docs.undo_count()));
    capture(app, Some(label), merge);
}

pub fn clear(app: &App) {
    let Some(state) = &app.panel else { return };
    let mut history = state.history.borrow_mut();
    load(app, &mut history);
    if history.error.is_some() {
        return;
    }
    *history = History {
        loaded: true,
        ..Default::default()
    };
    persist(app, &mut history);
}

pub fn retry(app: &App) {
    let Some(state) = &app.panel else {
        return;
    };
    let mut history = state.history.borrow_mut();
    if history.error.as_deref() == Some("Couldn't save") {
        history.error = None;
        persist(app, &mut history);
    } else {
        history.loaded = false;
        history.error = None;
        load(app, &mut history);
    }
}

pub fn restore(app: &mut App) {
    if !can_restore(app) {
        return;
    }
    let selected = app.panel.as_ref().and_then(|state| {
        let history = state.history.borrow();
        history
            .selected
            .and_then(|i| history.entries.get(i))
            .cloned()
    });
    let Some(entry) = selected else { return };
    app.apply("Restore this desktop", entry.desktop.ops(app));
    if Desktop::read(app) == entry.desktop {
        app.panel.as_ref().unwrap().history.borrow_mut().selected = None;
        app.toast_undo("Restored this desktop");
    }
}

pub fn can_restore(app: &App) -> bool {
    let Some(panel) = &app.panel else {
        return false;
    };
    let history = panel.history.borrow();
    history
        .selected
        .and_then(|i| history.entries.get(i).map(|entry| (i, entry)))
        .is_some_and(|(i, entry)| {
            i + 1 < history.entries.len() && entry.desktop != Desktop::read(app)
        })
}
