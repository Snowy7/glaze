//! Desktop snapshots. Older Studio loads and watches an explicit filename allowlist
//! (`docs::paths`, `watch::start`), so it ignores our versioned file beside config.json.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;
use crate::docs::{DocId, Op, apply_op};

const SETTINGS_IN_BARS: [&str; 2] = ["/performance", "/animation/reduceMotion"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desktop {
    config: Vec<(String, Option<Value>)>,
    bars: Value,
    widgets: Value,
    /// Keep the wallpaper's appearance layer even if its theme is edited later.
    #[serde(default)]
    theme: Value,
    #[serde(default)]
    dark: Option<bool>,
}

impl PartialEq for Desktop {
    fn eq(&self, other: &Self) -> bool {
        // Theme pixels/skin and the resolved Windows scheme are frozen drawing inputs.
        // A restore changes the three desktop documents, never Windows or a source theme.
        self.config == other.config && self.bars == other.bars && self.widgets == other.widgets
    }
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
            theme: app.library.active().map_or(Value::Null, |i| {
                super::saving::theme_value(app, &app.library.items[i])
                    .as_ref()
                    .clone()
            }),
            dark: Some(super::specimens::desktop_dark(app)),
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
    pub pictures: std::cell::RefCell<
        std::collections::HashMap<(usize, u32, u32), std::sync::Arc<mypaper_ui::Image>>,
    >,
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
    history.pictures.get_mut().clear();
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
        if before.bars.get("overrides") != after.bars.get("overrides")
            && let Some(monitor) = app.display()
        {
            let resolve = |raw: &Value| {
                mypaper_bars::BarsConfig::parse(&raw.to_string())
                    .ok()?
                    .for_monitor(&monitor.device, monitor.primary)
                    .ok()
            };
            if let (Some(old), Some(new)) = (resolve(&before.bars), resolve(&after.bars)) {
                if old.top.design != new.top.design {
                    return format!("Display {}: top bar design changed", app.nav.display + 1);
                }
                if old.dock.design != new.dock.design {
                    return format!("Display {}: dock design changed", app.nav.display + 1);
                }
            }
            return format!("Display {}: bars changed", app.nav.display + 1);
        }
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
    // Builds (including hover frames) may initialize once, but never compare or record.
    if app
        .panel
        .as_ref()
        .is_some_and(|state| state.history.borrow().last.is_none())
    {
        capture(app, None, false);
    }
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
                || path.ends_with("/appearance")
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
            i + 1 < history.entries.len() && history.last.as_ref() != Some(&entry.desktop)
        })
}

/// A snapshot owns every appearance input; current Look and current wallpaper never enter it.
/// Rasterize at the actual picture size so a long history retains only small visible pictures.
pub fn picture(
    app: &App,
    desktop: &Desktop,
    size: (u32, u32),
    at: u64,
) -> Option<std::sync::Arc<mypaper_ui::Image>> {
    use mypaper_shell::preview::{bars as bars_view, widgets as widgets_view};
    use mypaper_ui::{DisplayList, Op, Rect, Rgba, Ui};
    let rect = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let wallpaper = desktop.wallpaper().and_then(|id| {
        app.library
            .items
            .iter()
            .find(|w| w.id == id || w.apply_as == id)
    });
    let backdrop = wallpaper
        .and_then(|w| w.preview.as_ref().or(w.video.as_ref()))
        .and_then(|path| app.images.borrow_mut().get(path, size));
    if wallpaper.is_some_and(|w| w.preview.is_some() || w.video.is_some()) && backdrop.is_none() {
        return None;
    }
    let mut ui = Ui::isolated().ok()?;
    ui.text.set_capacity(24);
    let mut list = DisplayList::default();
    list.ops.push(Op::Fill {
        rect,
        radius: 0.0,
        color: super::tokens::hex(0x243044),
    });
    if let Some(image) = backdrop.as_ref() {
        let aspect = rect.width() / rect.height();
        let width = (image.height as f32 * aspect).min(image.width as f32);
        let height = width / aspect;
        list.ops.push(Op::Image {
            rect,
            image: image.clone(),
            opacity: 1.0,
            source: Some(Rect::new(
                (image.width as f32 - width) / 2.0,
                (image.height as f32 - height) / 2.0,
                width,
                height,
            )),
        });
    }
    let look = desktop
        .config
        .iter()
        .find(|(path, _)| path == "/look")
        .and_then(|(_, v)| v.as_ref());
    let light = !desktop.dark.unwrap_or_else(|| {
        look.and_then(|look| look.get("appearance"))
            .and_then(Value::as_str)
            != Some("light")
    });
    let display = app.display()?;
    let dip_width = display.rect.width as f32 / (display.dpi as f32 / 96.0);
    let scale = size.0 as f32 / dip_width;
    let theme = serde_json::from_value::<mypaper_theme::Theme>(desktop.theme.clone()).ok();
    if let Ok(mut config) = mypaper_bars::BarsConfig::parse(&desktop.bars.to_string())
        .and_then(|config| config.for_monitor(&display.device, display.primary))
    {
        config.resolve(Vec::new);
        if let Some(base) = theme
            .as_ref()
            .and_then(|t| t.bars.as_ref())
            .and_then(|s| s.appearance.as_ref())
        {
            config.appearance = config.appearance.over(base);
        }
        let system = mypaper_bars::accent::System {
            accent: std::array::from_fn(|_| "#5b8def".to_owned()),
            scheme: if light {
                mypaper_bars::accent::Scheme::Light
            } else {
                mypaper_bars::accent::Scheme::Dark
            },
            animations: false,
        };
        let model = bars_view::sample_model(
            windows::Win32::Foundation::SYSTEMTIME {
                wHour: 10,
                wMinute: 42,
                wYear: 2026,
                wMonth: 11,
                wDay: 6,
                ..Default::default()
            },
            "Files",
        );
        let mut images = bars_view::Images::default();
        let mut append = |frame: bars_view::Frame, x: f32, y: f32, tokens: &bars_view::Tokens| {
            for (shape, radius) in &frame.shapes {
                list.ops.push(Op::Fill {
                    rect: shape.offset(x, y),
                    radius: *radius,
                    color: tokens.material.map_or(tokens.theme.color.surface, |m| {
                        m.tint.with_alpha((1.0 - m.clarity).max(0.25))
                    }),
                });
            }
            list.ops.extend(crate::stage::offset(&frame.list.ops, x, y));
        };
        if config.top.enabled {
            let tokens = bars_view::Tokens::resolve(&config.top.look, &config.appearance, &system);
            let frame = bars_view::top(
                size.0,
                &config,
                &model,
                &tokens,
                scale,
                &mut ui,
                &mut images,
                None,
            );
            append(frame, 0.0, 16.0 * scale, &tokens);
        }
        if config.dock.enabled {
            let tokens = bars_view::Tokens::resolve(&config.dock.look, &config.appearance, &system);
            let apps: Vec<_> = config
                .dock
                .pinned
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|pin| mypaper_bars::apps::Item {
                    key: mypaper_bars::apps::pin_key(&pin.path),
                    name: pin
                        .name
                        .unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path)),
                    path: pin.path,
                    icon: None,
                    pinned: true,
                    windows: vec![],
                    aumid: None,
                    exe: None,
                })
                .collect();
            let frame = bars_view::dock(
                &config,
                &model,
                &apps,
                &tokens,
                scale,
                size,
                &[],
                &[],
                &mut ui,
                &mut images,
                None,
            );
            let x = (size.0 as f32 - frame.size.0 as f32) / 2.0;
            let y = size.1 as f32 - frame.size.1 as f32 - 16.0 * scale;
            append(frame, x, y, &tokens);
        }
    }
    if let Ok(layout) =
        serde_json::from_value::<mypaper_widgets::layout::Layout>(desktop.widgets.clone())
    {
        let grid = crate::stage::grid_for(&layout, display);
        let mut base = theme
            .as_ref()
            .and_then(|t| t.widgets.as_ref())
            .and_then(|skin| skin.appearance.clone())
            .unwrap_or_default()
            .over(&mypaper_widgets::look::defaults());
        if light && layout.appearance.text.is_none() {
            base.text = Some(mypaper_theme::TextTone::Dark);
        }
        let now = widgets_view::Stamp::at((at.min(253402300799) * 1000) as i64);
        let mut storage = mypaper_widgets::storage::Storage::new(
            std::env::temp_dir().join("mypaper-panel-snapshot"),
        );
        for instance in crate::pages::widgets::here(app, &layout) {
            let Some(widget) = app.registry.get(&instance.widget) else {
                continue;
            };
            let appearance = mypaper_widgets::look::resolve(
                &instance.appearance,
                &layout.appearance,
                &base,
                mypaper_widgets::look::DEFAULT_TINT,
            );
            let placed =
                mypaper_widgets::layout::place(&instance, &widget.manifest, &grid, &appearance);
            let position = grid.rect(placed.cell, placed.size);
            let frame = Rect::new(
                position.x as f32 * scale,
                position.y as f32 * scale,
                position.width as f32 * scale,
                position.height as f32 * scale,
            );
            let Ok(mut model) = widgets_view::Model::new(placed, &mut storage, now) else {
                continue;
            };
            model.photo.current = backdrop.clone();
            if appearance.surface != Some(mypaper_theme::Surface::None) {
                list.ops.push(Op::Fill {
                    rect: frame,
                    radius: appearance.radius.unwrap_or(22.0) * scale,
                    color: Rgba::rgb8(
                        appearance
                            .tint
                            .unwrap_or(mypaper_widgets::look::DEFAULT_TINT)
                            .0,
                        appearance.opacity.unwrap_or(0.46),
                    ),
                });
            }
            let appearance = mypaper_widgets::look::json_of(&appearance);
            let widget_list = widgets_view::view(
                &model,
                &widgets_view::tokens(&appearance),
                widgets_view::Frame {
                    size: (
                        frame.width().ceil().max(1.0) as u32,
                        frame.height().ceil().max(1.0) as u32,
                    ),
                    scale,
                    now,
                    instant: std::time::Instant::now(),
                    editing: false,
                },
                &mut ui.text,
            );
            list.ops.extend(crate::stage::offset(
                &widget_list.ops,
                frame.left,
                frame.top,
            ));
        }
    }
    ui.text.clear_cache();
    let picture = mypaper_ui::render(&mut ui, &list, size.0, size.1)
        .ok()
        .map(std::sync::Arc::new);
    drop(ui);
    drop(list);
    super::measure::raster_finished(app);
    picture
}

pub fn starter_picture(
    app: &App,
    index: usize,
    size: (u32, u32),
    taskbar: usize,
) -> Option<std::sync::Arc<mypaper_ui::Image>> {
    let (wallpaper, preset, items) = crate::pages::first_run::panel_starter(index)?;
    let wallpaper = app.library.items.iter().find(|w| w.id == wallpaper)?;
    let style = ["liquid", "fluent", "oneui", "mono"].get(index)?;
    let mut desktop = Desktop::read(app);
    let mut look = app
        .get(DocId::Config, "/look")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    look["style"] = serde_json::json!(style);
    desktop.config = vec![
        ("/theme".into(), Some(serde_json::json!(wallpaper.apply_as))),
        ("/look".into(), Some(look)),
    ];
    desktop.theme = super::saving::theme_value(app, wallpaper).as_ref().clone();
    desktop.bars = serde_json::json!({"version":1,"preset":preset,"look":{"style":style}});
    if matches!(preset, "macos" | "islands") {
        desktop.bars["dock"] = serde_json::json!({"enabled":true});
    }
    if taskbar != 0 {
        desktop.bars["top"] = serde_json::json!({"enabled":false});
        desktop.bars["dock"] = serde_json::json!({"enabled":false});
    }
    let mut row = 1;
    let instances: Vec<_> = if taskbar == 2 {
        vec![]
    } else {
        items.iter().enumerate().map(|(i,(id,size))| {
        let instance=serde_json::json!({"instance":format!("starter-{i}"),"widget":id,"monitor":"","cell":[20,row],"size":size});
        row+=size[1];
        instance
    }).collect()
    };
    desktop.widgets = serde_json::json!({"version":2,"instances":instances,"appearance":{
        "style":if *style == "liquid" {"liquid"} else {"minimal"},
        "surface":"glass","radius":if *style == "oneui" {32} else {16},
        "opacity":if *style == "liquid" {0.15} else {0.7}}});
    picture(app, &desktop, size, 1793958138)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_finishes_when_the_snapshot_has_a_different_windows_scheme() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        observe(&app);
        app.panel.as_ref().unwrap().history.borrow_mut().entries[0]
            .desktop
            .dark = Some(false);
        app.run(crate::actions::Action::Preset("islands"));
        app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(0);
        assert!(can_restore(&app));
        restore(&mut app);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .history
                .borrow()
                .selected
                .is_none()
        );
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|toast| toast.text == "Restored this desktop")
        );
    }
    #[test]
    fn rebuilds_do_not_record_or_compare_changed_documents() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        observe(&app);
        app.docs
            .edit(
                "External change",
                &[Op {
                    doc: DocId::Config,
                    path: "/look/style".into(),
                    value: Some(serde_json::json!("solid")),
                }],
            )
            .unwrap();
        observe(&app);
        assert_eq!(
            app.panel.as_ref().unwrap().history.borrow().entries.len(),
            1
        );
        record_drag(&app, "Committed change");
        assert_eq!(
            app.panel.as_ref().unwrap().history.borrow().entries.len(),
            2
        );
    }

    #[test]
    fn a_snapshot_picture_is_independent_of_todays_look_and_wallpaper() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        // No external image files are needed to check appearance ownership.
        for wallpaper in &mut app.library.items {
            wallpaper.preview = None;
            wallpaper.video = None;
        }
        let desktop = Desktop::read(&app);
        let before = picture(&app, &desktop, (216, 122), 1793958138).unwrap();
        app.run(crate::actions::Action::SetStyle(Some(
            mypaper_config::look::Style::Solid,
        )));
        for wallpaper in &mut app.library.items {
            wallpaper.active = false;
        }
        let after = picture(&app, &desktop, (216, 122), 1793958138).unwrap();
        assert_eq!(before.pixels, after.pixels);
        let current = Desktop::read(&app);
        assert_ne!(
            before.pixels,
            picture(&app, &current, (216, 122), 1793958138)
                .unwrap()
                .pixels
        );
    }
    #[test]
    fn history_retains_pictures_only_for_visible_rows_and_releases_them_on_leave() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.size = (480, 360);
        for wallpaper in &mut app.library.items {
            wallpaper.preview = None;
            wallpaper.video = None;
        }
        observe(&app);
        let entry = app.panel.as_ref().unwrap().history.borrow().entries[0].clone();
        app.panel.as_ref().unwrap().history.borrow_mut().entries = vec![entry; 30];
        app.panel.as_mut().unwrap().page = super::super::Page::History;
        super::super::build(&app, &mut ui.text);
        let count = app
            .panel
            .as_ref()
            .unwrap()
            .history
            .borrow()
            .pictures
            .borrow()
            .len();
        assert!(count > 0 && count <= 4, "visible picture count: {count}");
        super::super::run(
            &mut app,
            super::super::Command::Open(super::super::Page::Root),
        );
        super::super::build(&app, &mut ui.text);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .history
                .borrow()
                .pictures
                .borrow()
                .is_empty()
        );
    }
}
