//! Bar presets, module editing and pinned app actions shared by Studio pages.

use glaze_bars::{BarsConfig, ModuleRef};

use glaze_ui::controls::Glyph;
use glaze_ui::controls::key;
use serde_json::{Value, json};

use crate::actions::{Action, Bar, Page, Pins, Slot};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op, token};
use crate::theme::glyph;

/// Presets shown as tiles (Neon glass loads as Floating: its decoration is gone).
pub(crate) const PRESETS: [(&str, &str); 9] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("windows11", "Windows 11"),
    ("windows10", "Windows 10"),
    ("chromeos", "ChromeOS"),
    ("islands", "Islands"),
    ("floating", "Floating"),
    ("pill", "Floating pill"),
    ("edge", "Full-width edge"),
];

pub const TOP_DESIGNS: [(&str, &str); 8] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("fluent", "Fluent"),
    ("capsule", "Capsule"),
    ("minimal", "Minimal"),
    ("islands", "Islands"),
    ("floating", "Floating"),
    ("neon", "Neon"),
];

pub const DOCK_DESIGNS: [(&str, &str); 11] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("windows11", "Windows 11"),
    ("windows10", "Windows 10"),
    ("chromeos", "ChromeOS"),
    ("fluent", "Fluent"),
    ("islands", "Islands"),
    ("floating", "Floating"),
    ("neon", "Neon"),
    ("minimal", "Minimal"),
    ("edge", "Full-width edge"),
];

/// Modules, as Studio names them.
const MODULES: [(&str, &str, Glyph); 15] = [
    ("start", "Start", glyph::START),
    ("search", "Search", glyph::SEARCH),
    ("workspaces", "Desktops", glyph::LAYERS),
    ("app", "App", glyph::APP),
    ("clock", "Clock", glyph::CLOCK),
    ("media", "Media", glyph::MUSIC),
    ("tray", "Tray", glyph::CHEVRON_DOWN),
    ("network", "Network", glyph::WIFI),
    ("volume", "Volume", glyph::SOUND),
    ("battery", "Battery", glyph::BATTERY),
    ("quick", "Quick settings", glyph::TOGGLES),
    ("power", "Power", glyph::POWER),
    ("stats", "CPU and memory", glyph::CHIP),
    ("language", "Keyboard language", glyph::KEYBOARD),
    ("notifications", "Notifications", glyph::BELL),
];

fn config(app: &App) -> Option<BarsConfig> {
    BarsConfig::parse(&serde_json::to_string(&app.docs.doc(DocId::Bars).raw).ok()?).ok()
}

fn module_key(m: &ModuleRef) -> String {
    serde_json::to_value(m)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Installed component plugins' bar modules (docs/plugins.md, "Bar modules"), as
/// `(custom:<plugin>--<bar>, "Name · Plugin")`; read again each time the Add sheet opens.
static PLUGIN_MODULES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

fn refresh_plugin_modules() {
    use glaze_widgets::manifest::PluginManifest;
    let mut found: Vec<(String, String)> = glaze_widgets::registry::plugin_roots()
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| PluginManifest::load(&entry.path()).ok())
        .filter(|m| m.component.is_some() && !m.id.contains("--"))
        .flat_map(|m| {
            m.bars
                .iter()
                .map(|b| {
                    (
                        format!("custom:{}--{}", m.id, b.id),
                        format!("{} · {}", b.name, m.name),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // A user install (listed first) shadows the bundled copy of the same plugin.
    let mut seen = std::collections::BTreeSet::new();
    found.retain(|(key, _)| seen.insert(key.clone()));
    *PLUGIN_MODULES.lock().expect("plugin modules") = found;
}

fn plugin_module_name(key: &str) -> Option<String> {
    if key.starts_with("custom:")
        && key.contains("--")
        && PLUGIN_MODULES.lock().expect("plugin modules").is_empty()
    {
        refresh_plugin_modules();
    }
    PLUGIN_MODULES
        .lock()
        .expect("plugin modules")
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, n)| n.clone())
}

pub fn module_name(m: &ModuleRef) -> String {
    let k = module_key(m);
    if let Some(name) = plugin_module_name(&k) {
        return name;
    }
    if let Some(id) = k.strip_prefix("custom:") {
        return format!("{id} (yours)");
    }
    if k == "|" {
        return "Gap".into();
    }
    MODULES
        .iter()
        .find(|(id, ..)| *id == k)
        .map_or(k.clone(), |(_, n, _)| (*n).to_owned())
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::SelectBar(bar) => {
            app.go(Page::Bars);
            app.nav.bar = bar;
            app.nav.module = None;
            app.nav.panel = None;
        }
        Action::SelectModule(slot) => {
            app.go(Page::Bars);
            if let Some(s) = &slot {
                app.nav.bar = Some(s.bar);
            }
            app.nav.module = slot;
            app.nav.panel = None;
        }
        Action::SelectPanel(panel) => {
            app.go(Page::Bars);
            app.nav.panel = panel;
            app.nav.module = None;
            if panel.is_some() {
                app.nav.bar = None;
            }
        }
        Action::Preset(id) => {
            // A preset decides both bars' designs and which are on: clear the bars' own choices.
            let mut ops = vec![Op {
                doc: DocId::Bars,
                path: "/preset".into(),
                value: Some(json!(id)),
            }];
            for bar in ["top", "dock"] {
                for field in ["design", "enabled"] {
                    ops.push(Op {
                        doc: DocId::Bars,
                        path: format!("/{bar}/{field}"),
                        value: None,
                    });
                }
            }
            let name = PRESETS
                .iter()
                .find(|(p, _)| *p == id)
                .map_or(id, |(_, n)| n);
            app.apply(&format!("{name} preset"), ops);
            app.toast_undo(format!("Bars: {name}"));
        }
        Action::AddModule(bar) => {
            refresh_plugin_modules();
            app.overlay = Some(Overlay::AddModule { bar });
        }
        Action::InsertModule(bar, list, module) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, bar, list);
            items.push(json!(module));
            app.apply(
                "Add module",
                vec![Op {
                    doc: DocId::Bars,
                    path: format!("/{}/{list}", bar.key()),
                    value: Some(Value::Array(items)),
                }],
            );
            app.overlay = None;
            app.toast_undo(format!(
                "Added {}",
                module_name(&serde_json::from_value(json!(module)).unwrap_or(ModuleRef::Break))
            ));
        }
        Action::RemoveModule(slot) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, slot.bar, slot.list);
            if slot.index < items.len() {
                items.remove(slot.index);
                app.apply(
                    "Remove module",
                    vec![Op {
                        doc: DocId::Bars,
                        path: format!("/{}/{}", slot.bar.key(), slot.list),
                        value: Some(Value::Array(items)),
                    }],
                );
                app.nav.module = None;
                app.toast_undo("Removed the module");
            }
        }
        Action::MoveModule(slot, delta) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, slot.bar, slot.list);
            let to = slot.index as i32 + delta;
            if to >= 0 && (to as usize) < items.len() {
                items.swap(slot.index, to as usize);
                app.apply(
                    "Move module",
                    vec![Op {
                        doc: DocId::Bars,
                        path: format!("/{}/{}", slot.bar.key(), slot.list),
                        value: Some(Value::Array(items)),
                    }],
                );
                let moved = Slot {
                    index: to as usize,
                    ..slot
                };
                app.input.focus = Some(key(("module", moved.bar.key(), moved.list, moved.index)));
                app.input.focus_visible = true;
            }
        }
        Action::DeleteCustom(id) => {
            let Some(config) = config(app) else { return };
            let k = format!("custom:{id}");
            let mut ops = vec![Op {
                doc: DocId::Bars,
                path: format!("/modules/{}", token(&k)),
                value: None,
            }];
            for (bar, lists) in [
                (Bar::Top, &["left", "center", "right"][..]),
                (Bar::Dock, &["left", "right"][..]),
            ] {
                for list in lists {
                    let items: Vec<Value> = list_of(&config, bar, list)
                        .into_iter()
                        .filter(|v| v.as_str() != Some(&k))
                        .collect();
                    ops.push(Op {
                        doc: DocId::Bars,
                        path: format!("/{}/{list}", bar.key()),
                        value: Some(Value::Array(items)),
                    });
                }
            }
            app.apply("Delete module", ops);
            app.nav.module = None;
        }
        Action::ResetBar(bar) => {
            app.apply(
                "Reset the bar",
                vec![
                    Op {
                        doc: DocId::Bars,
                        path: format!("/{}/look", bar.key()),
                        value: None,
                    },
                    Op {
                        doc: DocId::Bars,
                        path: format!("/{}/animation", bar.key()),
                        value: None,
                    },
                ],
            );
            app.toast_undo("Back to its design");
        }
        Action::StartBars => app.run(Action::StartGlaze),
        Action::PinsOwn(which) => {
            let Some(config) = config(app) else { return };
            let start = match which {
                Pins::Dock => glaze_shell::preview::bars::default_pins(),
                Pins::Start => config
                    .dock
                    .pinned
                    .unwrap_or_else(glaze_shell::preview::bars::default_pins),
            };
            set_pins(
                app,
                which,
                "Your own pins",
                start
                    .into_iter()
                    .filter_map(|p| serde_json::to_value(p).ok())
                    .collect(),
            );
        }
        Action::PinAdd(which) => {
            if let Some(path) = crate::dialogs::pick(
                app.window,
                "Choose an app to pin",
                false,
                crate::dialogs::PROGRAMS,
            ) {
                let Some(config) = config(app) else { return };
                let mut pins = pin_values(&config, which);
                pins.push(json!({ "path": path.display().to_string() }));
                set_pins(app, which, "Pin an app", pins);
            }
        }
        Action::PinRemove(which, i) => {
            let Some(config) = config(app) else { return };
            let mut pins = pin_values(&config, which);
            if i < pins.len() && app.docs.is_set(DocId::Bars, which.path()) {
                pins.remove(i);
                set_pins(app, which, "Unpin", pins);
            }
        }
        Action::PinMove(which, i, delta) => {
            let Some(config) = config(app) else { return };
            let mut pins = pin_values(&config, which);
            let to = i as i32 + delta;
            if to >= 0 && (to as usize) < pins.len() {
                pins.swap(i, to as usize);
                set_pins(app, which, "Move pin", pins);
            }
        }
        _ => {}
    }
}

/// A list's own pins (empty while it follows another list).
fn own_pins(config: &BarsConfig, which: Pins) -> Vec<glaze_bars::config::Pinned> {
    match which {
        Pins::Dock => config.dock.pinned.clone(),
        Pins::Start => config.launcher.pinned.clone(),
    }
    .unwrap_or_default()
}

fn pin_values(config: &BarsConfig, which: Pins) -> Vec<Value> {
    own_pins(config, which)
        .into_iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .collect()
}

fn set_pins(app: &mut App, which: Pins, label: &str, pins: Vec<Value>) {
    app.apply(
        label,
        vec![Op {
            doc: DocId::Bars,
            path: which.path().into(),
            value: Some(Value::Array(pins)),
        }],
    );
}

fn list_of(config: &BarsConfig, bar: Bar, list: &str) -> Vec<Value> {
    let items: &[ModuleRef] = match (bar, list) {
        (Bar::Top, "left") => &config.top.left,
        (Bar::Top, "center") => &config.top.center,
        (Bar::Top, _) => &config.top.right,
        (Bar::Dock, "left") => &config.dock.left,
        (Bar::Dock, _) => &config.dock.right,
    };
    items
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect()
}

/// Delete removes the selected module.
pub fn on_key(app: &mut App, vk: u16) -> bool {
    if vk == 0x2E
        && app.field.is_none()
        && let Some(slot) = app.nav.module.clone()
    {
        app.run(Action::RemoveModule(slot));
        return true;
    }
    false
}

