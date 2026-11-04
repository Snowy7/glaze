//! Widgets as Studio sees them (docs/widgets.md): the installed list, the
//! placed instances in `%APPDATA%\mypaper\widgets.json`, and the host's edit
//! mode. The layout file belongs to the widget host, which also writes it, so
//! edits go through `serde_json::Value` and touch only what they mean to:
//! the grid, the instances' cells, settings and everything unknown survive.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW,
};
use windows::core::w;

use crate::fsio;
use crate::look::Look;

pub const LAYOUT_FILE: &str = "widgets.json";

// --- installed widgets ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Bundled,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSize {
    pub default: [u32; 2],
    pub min: [u32; 2],
    /// Unbounded when absent.
    pub max: Option<[u32; 2]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub size: WidgetSize,
    pub resizable: bool,
    pub permissions: Vec<String>,
    /// Placement metadata; native widgets are hosted in the desktop band.
    pub preset: String,
    pub origin: Origin,
    pub dir: String,
    /// Empty for native widgets; retained for the Studio IPC shape.
    pub entry: String,
    /// The compiled schema, including the existing per-widget settings form.
    pub manifest: Value,
    /// The manifest doesn't load; the other fields are placeholders.
    pub error: Option<String>,
}

/// The part of `widget.json` the list shows. The host validates the rest.
#[derive(Debug, Deserialize)]
struct Manifest {
    id: String,
    name: String,
    version: Option<String>,
    author: Option<String>,
    description: Option<String>,
    size: ManifestSize,
    #[serde(default = "yes")]
    resizable: bool,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default = "desktop")]
    preset: String,
}

fn desktop() -> String {
    "desktop".to_owned()
}

#[derive(Debug, Deserialize)]
struct ManifestSize {
    default: [u32; 2],
    min: Option<[u32; 2]>,
    max: Option<[u32; 2]>,
}

fn yes() -> bool {
    true
}

/// Native widget schemas stay available to the existing settings inspector. There are no HTML
/// entry points; N7 supplies native previews in Studio.
pub fn list() -> Vec<WidgetInfo> {
    let schemas = [
        include_str!("../../../../crates/widgets/src/catalog/calendar.json"),
        include_str!("../../../../crates/widgets/src/catalog/clock.json"),
        include_str!("../../../../crates/widgets/src/catalog/countdown.json"),
        include_str!("../../../../crates/widgets/src/catalog/crypto.json"),
        include_str!("../../../../crates/widgets/src/catalog/heatmap.json"),
        include_str!("../../../../crates/widgets/src/catalog/launcher.json"),
        include_str!("../../../../crates/widgets/src/catalog/media-pill.json"),
        include_str!("../../../../crates/widgets/src/catalog/notes.json"),
        include_str!("../../../../crates/widgets/src/catalog/now-playing.json"),
        include_str!("../../../../crates/widgets/src/catalog/photo.json"),
        include_str!("../../../../crates/widgets/src/catalog/quick-toggles.json"),
        include_str!("../../../../crates/widgets/src/catalog/quote.json"),
        include_str!("../../../../crates/widgets/src/catalog/rss.json"),
        include_str!("../../../../crates/widgets/src/catalog/system-monitor.json"),
        include_str!("../../../../crates/widgets/src/catalog/timer.json"),
        include_str!("../../../../crates/widgets/src/catalog/todo.json"),
        include_str!("../../../../crates/widgets/src/catalog/weather.json"),
    ];
    let mut widgets: Vec<_> = schemas
        .into_iter()
        .map(|schema| {
            let mut raw: Value = serde_json::from_str(schema).expect("compiled widget schema");
            let m = Manifest::deserialize(&raw).expect("compiled widget metadata");
            raw["native"] = Value::Bool(true);
            WidgetInfo {
                size: WidgetSize {
                    default: m.size.default,
                    min: m.size.min.unwrap_or([1, 1]),
                    max: m.size.max,
                },
                id: m.id,
                name: m.name,
                version: m.version,
                author: m.author,
                description: m.description,
                resizable: m.resizable,
                permissions: m.permissions,
                preset: m.preset,
                origin: Origin::Bundled,
                dir: String::new(),
                entry: String::new(),
                manifest: raw,
                error: None,
            }
        })
        .collect();
    widgets.sort_by_key(|w| w.name.to_lowercase());
    widgets
}

// --- the layout file ---

/// A placed widget, as the list shows it. Everything else on the instance is left alone.
#[derive(Debug, Serialize, Deserialize)]
pub struct Instance {
    pub instance: String,
    pub widget: String,
    #[serde(default)]
    pub monitor: Option<String>,
    #[serde(default)]
    pub cell: Option<[i64; 2]>,
    #[serde(default)]
    pub size: Option<[i64; 2]>,
    #[serde(default)]
    pub appearance: Look,
    /// The widget's own options (its manifest's `settings`); unset keys take their defaults.
    #[serde(default)]
    pub settings: Map<String, Value>,
}

/// `widgets.json` → `grid` (docs/widgets.md, "Layout"): `cell` 0 means the desktop icon spacing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    #[serde(default)]
    pub cell: f64,
    #[serde(default = "default_gap")]
    pub gap: f64,
    #[serde(default = "default_margin")]
    pub margin: f64,
}

fn default_gap() -> f64 {
    8.0
}

fn default_margin() -> f64 {
    16.0
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            cell: 0.0,
            gap: default_gap(),
            margin: default_margin(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// The user layer (`widgets.json` → `appearance`).
    pub appearance: Look,
    pub instances: Vec<Instance>,
    pub grid: Grid,
    /// What `cell: 0` stands for: the desktop icon spacing in DIPs, `[horizontal, vertical]`.
    pub icon_cell: [f64; 2],
    /// Whether the topmost overlay layer is shown (`widgets.json` → `overlay`, default on).
    pub overlay: bool,
    /// False: there is no `widgets.json` yet.
    pub exists: bool,
}

fn skeleton() -> Map<String, Value> {
    let Value::Object(map) =
        json!({ "version": 1, "grid": { "cell": 0, "gap": 8, "margin": 16 }, "instances": [] })
    else {
        unreachable!("a JSON object literal")
    };
    map
}

fn parse_object(text: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str(text).context("widgets.json is not valid JSON")? {
        Value::Object(map) => Ok(map),
        _ => bail!("widgets.json must hold a JSON object"),
    }
}

fn render(map: Map<String, Value>) -> Result<String> {
    Ok(serde_json::to_string_pretty(&Value::Object(map))? + "\n")
}

pub fn read_layout(text: Option<&str>) -> Result<Layout> {
    let Some(text) = text else {
        return Ok(Layout {
            appearance: Look::default(),
            instances: Vec::new(),
            grid: Grid::default(),
            icon_cell: icon_cell(),
            overlay: true,
            exists: false,
        });
    };
    let mut doc = parse_object(text)?;
    let overlay = doc.get("overlay").and_then(Value::as_bool).unwrap_or(true);
    let grid = match doc.remove("grid") {
        Some(value) => serde_json::from_value(value).context("reading the grid")?,
        None => Grid::default(),
    };
    let appearance = match doc.remove("appearance") {
        Some(value) => serde_json::from_value(value).context("reading the appearance section")?,
        None => Look::default(),
    };
    let instances = match doc.remove("instances") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| serde_json::from_value(item).context("reading an instance"))
            .collect::<Result<_>>()?,
        Some(_) => bail!("instances must be a list"),
        None => Vec::new(),
    };
    Ok(Layout {
        appearance,
        instances,
        grid,
        icon_cell: icon_cell(),
        overlay,
        exists: true,
    })
}

/// The desktop icon grid in DIPs, as the widget host reads it for `cell: 0`:
/// `HKCU\Control Panel\Desktop\WindowMetrics` `IconSpacing` / `IconVerticalSpacing`.
/// Missing values are 96.
fn icon_cell() -> [f64; 2] {
    let read = |name: windows::core::PCWSTR| -> Option<f64> {
        let mut buffer = [0u16; 32];
        let mut size = u32::try_from(std::mem::size_of_val(&buffer)).ok()?;
        // SAFETY: the buffer and its byte size are passed together; RRF_RT_REG_SZ null-terminates.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\Desktop\\WindowMetrics"),
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let end = buffer.iter().position(|&c| c == 0)?;
        spacing_dips(
            String::from_utf16_lossy(&buffer[..end])
                .trim()
                .parse()
                .ok()?,
        )
    };
    [
        read(w!("IconSpacing")).unwrap_or(96.0),
        read(w!("IconVerticalSpacing")).unwrap_or(96.0),
    ]
}

/// A `WindowMetrics` spacing value in DIPs: negative values are twips, positive ones DIPs.
fn spacing_dips(value: i32) -> Option<f64> {
    match value {
        0 => None,
        v if v < 0 => Some(f64::from(-v) / 15.0),
        v => Some(f64::from(v)),
    }
}

/// The document with the overlay layer on or off.
pub fn with_overlay(text: Option<&str>, on: bool) -> Result<String> {
    let mut doc = match text {
        Some(text) => parse_object(text)?,
        None => skeleton(),
    };
    doc.insert("overlay".into(), Value::Bool(on));
    render(doc)
}

/// The document with its global `appearance` replaced (removed when `look` is empty).
pub fn with_appearance(text: Option<&str>, look: &Look) -> Result<String> {
    look.validate()?;
    let mut doc = match text {
        Some(text) => parse_object(text)?,
        None => skeleton(),
    };
    if look.is_empty() {
        doc.shift_remove("appearance");
    } else {
        doc.insert("appearance".into(), Value::Object(look.to_map()?));
    }
    render(doc)
}

fn instances_mut(doc: &mut Map<String, Value>) -> Result<&mut Vec<Value>> {
    match doc.get_mut("instances") {
        Some(Value::Array(items)) => Ok(items),
        _ => bail!("widgets.json has no instances"),
    }
}

fn has_id(item: &Value, id: &str) -> bool {
    item.get("instance").and_then(Value::as_str) == Some(id)
}

/// The document without one placed widget.
pub fn without_instance(text: &str, id: &str) -> Result<String> {
    let mut doc = parse_object(text)?;
    let items = instances_mut(&mut doc)?;
    let before = items.len();
    items.retain(|item| !has_id(item, id));
    if items.len() == before {
        bail!("no placed widget named {id}");
    }
    render(doc)
}

/// The document with one instance's own `appearance` removed, so it follows the global look again.
pub fn with_instance_look_cleared(text: &str, id: &str) -> Result<String> {
    let mut doc = parse_object(text)?;
    let item = instances_mut(&mut doc)?
        .iter_mut()
        .find(|item| has_id(item, id))
        .with_context(|| format!("no placed widget named {id}"))?;
    if let Value::Object(map) = item {
        map.shift_remove("appearance");
    }
    render(doc)
}

/// What `put_instance` writes for one placed widget: only the fields it names change. A new id
/// adds the widget (then `widget` is required).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstancePatch {
    pub instance: String,
    pub widget: Option<String>,
    /// `null` clears it (the primary monitor); absent leaves it.
    #[serde(default, deserialize_with = "some_option")]
    pub monitor: Option<Option<String>>,
    pub cell: Option<[i64; 2]>,
    pub size: Option<[i64; 2]>,
    /// Replaces the instance's settings; empty removes them.
    pub settings: Option<Map<String, Value>>,
    /// Replaces the instance's own look; empty removes it.
    pub appearance: Option<Look>,
}

/// A present field, `null` included, is `Some`: tells "clear it" from "leave it".
fn some_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// Instance ids are `[a-z0-9-]` (docs/widgets.md, "Layout").
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The document with one instance added or changed (only the patch's fields).
pub fn with_instance(text: Option<&str>, patch: &InstancePatch) -> Result<String> {
    if !valid_id(&patch.instance) {
        bail!(
            "{:?} isn't a widget instance id (a-z, 0-9 and -)",
            patch.instance
        );
    }
    if let Some(look) = &patch.appearance {
        look.validate()?;
    }
    let mut doc = match text {
        Some(text) => parse_object(text)?,
        None => skeleton(),
    };
    if !matches!(doc.get("instances"), Some(Value::Array(_))) {
        doc.insert("instances".into(), Value::Array(Vec::new()));
    }
    let items = instances_mut(&mut doc)?;
    let index = match items.iter().position(|item| has_id(item, &patch.instance)) {
        Some(index) => index,
        None => {
            let widget = patch
                .widget
                .as_deref()
                .with_context(|| format!("no placed widget named {}", patch.instance))?;
            items.push(json!({ "instance": patch.instance, "widget": widget }));
            items.len() - 1
        }
    };
    let Some(Value::Object(item)) = items.get_mut(index) else {
        bail!("instance {} isn't an object", patch.instance);
    };
    if let Some(monitor) = &patch.monitor {
        match monitor {
            Some(device) => item.insert("monitor".into(), Value::String(device.clone())),
            None => item.shift_remove("monitor"),
        };
    }
    if let Some(cell) = patch.cell {
        item.insert("cell".into(), json!(cell));
    }
    if let Some(size) = patch.size {
        item.insert("size".into(), json!(size));
    }
    if let Some(settings) = &patch.settings {
        if settings.is_empty() {
            item.shift_remove("settings");
        } else {
            item.insert("settings".into(), Value::Object(settings.clone()));
        }
    }
    if let Some(look) = &patch.appearance {
        if look.is_empty() {
            item.shift_remove("appearance");
        } else {
            item.insert("appearance".into(), Value::Object(look.to_map()?));
        }
    }
    render(doc)
}

pub fn layout_path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(LAYOUT_FILE))
}

pub fn load() -> Result<Layout> {
    let path = layout_path()?;
    read_layout(fsio::read_optional(&path)?.as_deref())
        .with_context(|| format!("in {}", path.display()))
}

/// Reads the file, applies `edit`, and writes if the text changed. The host
/// writes this file too, so the window between read and write is kept short.
fn update(edit: impl FnOnce(Option<&str>) -> Result<String>) -> Result<Layout> {
    let path = layout_path()?;
    let current = fsio::read_optional(&path)?;
    let next = edit(current.as_deref()).with_context(|| format!("in {}", path.display()))?;
    if current.as_deref() != Some(next.as_str()) {
        fsio::write_atomic(&path, &next)?;
    }
    load()
}

pub fn set_appearance(look: &Look) -> Result<Layout> {
    update(|text| with_appearance(text, look))
}

pub fn remove_instance(id: &str) -> Result<Layout> {
    update(|text| without_instance(text.context("there is no widgets.json yet")?, id))
}

pub fn put_instance(patch: &InstancePatch) -> Result<Layout> {
    update(|text| with_instance(text, patch))
}

pub fn clear_instance_look(id: &str) -> Result<Layout> {
    update(|text| with_instance_look_cleared(text.context("there is no widgets.json yet")?, id))
}

/// Turns the overlay layer on or off. The running host hears it through its registered message
/// (`mypaper.widgets.overlay`, `wParam` 1 or 0) and applies it at once; the choice is also written
/// to `widgets.json`, so it holds when the host isn't running.
pub fn set_overlay(on: bool) -> Result<Layout> {
    let layout = update(|text| with_overlay(text, on))?;
    // No host running is fine: it reads the file when it starts.
    let _ = post_to_host(w!("mypaper.widgets.overlay"), usize::from(on));
    Ok(layout)
}

// --- the host's edit mode ---

/// Posts the registered message `name` to the host's message-only window.
fn post_to_host(name: windows::core::PCWSTR, wparam: usize) -> Result<()> {
    // SAFETY: plain Win32 calls on constant strings; the posted message carries no pointers.
    unsafe {
        let message = RegisterWindowMessageW(name);
        if message == 0 {
            bail!("couldn't register the widget host's message");
        }
        let host = FindWindowExW(Some(HWND_MESSAGE), None, w!("mypaper.widgets.host"), None)
            .ok()
            .filter(|hwnd| *hwnd != HWND::default())
            .context("the widget host isn't running (mypaper-widgets.exe)")?;
        PostMessageW(Some(host), message, WPARAM(wparam), LPARAM(0))
            .context("posting to the widget host")?;
    }
    Ok(())
}

/// Posts the registered `mypaper.widgets.edit` message (`wParam` 2 = toggle) to
/// the host's message-only window.
pub fn toggle_edit_mode() -> Result<()> {
    post_to_host(w!("mypaper.widgets.edit"), 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: &str = r##"{
  "version": 1,
  "grid": { "cell": 96, "gap": 6, "margin": 20 },
  "appearance": { "surface": "glass", "blur": 30 },
  "instances": [
    { "instance": "clock-1", "widget": "clock", "monitor": "\\\\.\\DISPLAY1", "cell": [0, 0], "size": [2, 1],
      "settings": { "format24": true }, "appearance": { "text": "dark", "radius": 4 }, "pinned": true },
    { "instance": "notes-1", "widget": "notes", "cell": [2, 0], "size": [2, 2] }
  ],
  "extra": { "keep": "me" }
}"##;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_layout_is_read_for_the_list_and_missing_means_empty() {
        let layout = read_layout(Some(LAYOUT)).unwrap();
        assert!(layout.exists);
        assert_eq!(layout.instances.len(), 2);
        assert_eq!(layout.instances[0].cell, Some([0, 0]));
        assert!(
            !layout.instances[0].appearance.is_empty() && layout.instances[1].appearance.is_empty()
        );
        assert_eq!(layout.appearance.0.blur, Some(30.0));

        let none = read_layout(None).unwrap();
        assert!(!none.exists && none.instances.is_empty() && none.appearance.is_empty());
        assert!(read_layout(Some("[]")).is_err());
        assert!(read_layout(Some(r#"{ "appearance": { "glow": 1 } }"#)).is_err());
    }

    #[test]
    fn editing_the_global_look_keeps_instances_grid_and_unknown_fields() {
        let look: Look = serde_json::from_str(
            r##"{ "surface": "solid", "tint": "#202020", "opacity": 0.8, "accent": "auto" }"##,
        )
        .unwrap();
        let text = with_appearance(Some(LAYOUT), &look).unwrap();

        let mut expected = json(LAYOUT);
        expected["appearance"] = json(
            r##"{ "surface": "solid", "tint": "#202020", "opacity": 0.8, "accent": "auto" }"##,
        );
        assert_eq!(json(&text), expected, "{text}");
        assert!(
            text.find("\"grid\"").unwrap() < text.find("\"appearance\"").unwrap(),
            "key order is kept"
        );

        // Clearing every field removes the section.
        let cleared = with_appearance(Some(&text), &Look::default()).unwrap();
        let mut without = json(LAYOUT);
        without.as_object_mut().unwrap().remove("appearance");
        assert_eq!(json(&cleared), without);
    }

    #[test]
    fn the_overlay_layer_defaults_on_and_the_switch_edits_only_its_key() {
        assert!(read_layout(None).unwrap().overlay && read_layout(Some(LAYOUT)).unwrap().overlay);

        let off = with_overlay(Some(LAYOUT), false).unwrap();
        assert!(!read_layout(Some(&off)).unwrap().overlay);
        let mut expected = json(LAYOUT);
        expected["overlay"] = false.into();
        assert_eq!(json(&off), expected);

        let on = with_overlay(Some(&off), true).unwrap();
        assert!(read_layout(Some(&on)).unwrap().overlay);
        assert_eq!(json(&with_overlay(None, false).unwrap())["overlay"], false);

        // A style in the global look is part of the same layer.
        let look: Look =
            serde_json::from_str(r#"{ "style": "aero", "surface": "glass" }"#).unwrap();
        let styled = with_appearance(Some(&on), &look).unwrap();
        assert_eq!(json(&styled)["appearance"]["style"], "aero");
        assert_eq!(read_layout(Some(&styled)).unwrap().appearance, look);
        assert!(serde_json::from_str::<Look>(r#"{ "style": "glossy" }"#).is_err());
    }

    #[test]
    fn a_new_file_starts_from_the_documented_skeleton() {
        let look: Look = serde_json::from_str(r#"{ "scale": 1.25 }"#).unwrap();
        let value = json(&with_appearance(None, &look).unwrap());
        assert_eq!(value["version"], 1);
        assert_eq!(value["instances"], json("[]"));
        assert_eq!(value["appearance"], json(r#"{ "scale": 1.25 }"#));
    }

    #[test]
    fn out_of_range_looks_are_refused() {
        let look: Look = serde_json::from_str(r#"{ "scale": 5 }"#).unwrap();
        assert!(with_appearance(Some(LAYOUT), &look).is_err());
        assert!(with_appearance(Some("not json"), &Look::default()).is_err());
    }

    #[test]
    fn removing_and_resetting_touch_only_one_instance() {
        let removed = json(&without_instance(LAYOUT, "clock-1").unwrap());
        assert_eq!(removed["instances"].as_array().unwrap().len(), 1);
        assert_eq!(removed["instances"][0], json(LAYOUT)["instances"][1]);
        assert_eq!(removed["extra"], json(LAYOUT)["extra"]);
        assert!(without_instance(LAYOUT, "nope-9").is_err());

        let reset = json(&with_instance_look_cleared(LAYOUT, "clock-1").unwrap());
        let mut expected = json(LAYOUT);
        expected["instances"][0]
            .as_object_mut()
            .unwrap()
            .remove("appearance");
        assert_eq!(
            reset, expected,
            "settings and unknown fields stay on the instance"
        );
        assert!(with_instance_look_cleared(LAYOUT, "nope-9").is_err());
    }

    #[test]
    fn the_grid_and_instance_settings_are_read() {
        let layout = read_layout(Some(LAYOUT)).unwrap();
        assert_eq!(
            layout.grid,
            Grid {
                cell: 96.0,
                gap: 6.0,
                margin: 20.0
            }
        );
        assert_eq!(layout.instances[0].settings["format24"], true);
        assert!(layout.instances[1].settings.is_empty());
        assert_eq!(read_layout(Some("{}")).unwrap().grid, Grid::default());
        assert_eq!(
            (spacing_dips(-1125), spacing_dips(80), spacing_dips(0)),
            (Some(75.0), Some(80.0), None)
        );
    }

    fn patch(text: &str) -> InstancePatch {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn putting_an_instance_changes_only_what_it_names() {
        let moved = json(
            &with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "clock-1", "cell": [4, 2], "size": [3, 2] }"#),
            )
            .unwrap(),
        );
        let mut expected = json(LAYOUT);
        expected["instances"][0]["cell"] = json("[4, 2]");
        expected["instances"][0]["size"] = json("[3, 2]");
        assert_eq!(moved, expected, "settings, look and unknown fields stay");

        let set = json(
            &with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "clock-1", "settings": { "variant": "flip" }, "appearance": {} }"#),
            )
            .unwrap(),
        );
        assert_eq!(
            set["instances"][0]["settings"],
            json(r#"{ "variant": "flip" }"#)
        );
        assert!(
            set["instances"][0].get("appearance").is_none(),
            "an empty look is removed"
        );

        let cleared = json(
            &with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "clock-1", "monitor": null, "settings": {} }"#),
            )
            .unwrap(),
        );
        assert!(
            cleared["instances"][0].get("monitor").is_none()
                && cleared["instances"][0].get("settings").is_none()
        );

        let added = json(
            &with_instance(
                None,
                &patch(r#"{ "instance": "notes-2", "widget": "notes", "monitor": "\\\\.\\DISPLAY2", "cell": [0, 3], "size": [2, 2] }"#),
            )
            .unwrap(),
        );
        assert_eq!(
            added["instances"],
            json(
                r#"[{ "instance": "notes-2", "widget": "notes", "monitor": "\\\\.\\DISPLAY2", "cell": [0, 3], "size": [2, 2] }]"#
            )
        );
        assert_eq!(
            added["grid"],
            json(r#"{ "cell": 0, "gap": 8, "margin": 16 }"#)
        );

        assert!(
            with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "nope-1", "cell": [0, 0] }"#)
            )
            .is_err(),
            "a new instance needs its widget"
        );
        assert!(
            with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "Bad Id", "widget": "clock" }"#)
            )
            .is_err()
        );
        assert!(
            with_instance(
                Some(LAYOUT),
                &patch(r#"{ "instance": "clock-1", "appearance": { "scale": 9 } }"#)
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<InstancePatch>(r#"{ "instance": "a", "pinned": true }"#)
                .is_err()
        );
    }

    #[test]
    fn compiled_widget_schemas_keep_the_existing_settings_contract() {
        let widgets = list();
        assert_eq!(widgets.len(), 17);
        let clock = widgets.iter().find(|w| w.id == "clock").unwrap();
        assert_eq!(clock.manifest["native"], true);
        assert!(clock.manifest["settings"].is_array());
        assert!(clock.dir.is_empty() && clock.entry.is_empty());
    }
}
