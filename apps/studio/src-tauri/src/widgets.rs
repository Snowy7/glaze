//! Widgets as Studio sees them (docs/widgets.md): the installed list, the
//! placed instances in `%APPDATA%\mypaper\widgets.json`, and the host's edit
//! mode. The layout file belongs to the widget host, which also writes it, so
//! edits go through `serde_json::Value` and touch only what they mean to:
//! the grid, the instances' cells, settings and everything unknown survive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW};
use windows::core::w;

use crate::fsio;
use crate::look::Look;

pub const LAYOUT_FILE: &str = "widgets.json";
const MANIFEST: &str = "widget.json";

// --- installed widgets ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Bundled,
    User,
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
    /// Where it lives: `desktop` (below windows), `overlay` (always on top) or `popup` (a bar opens it).
    pub preset: String,
    pub origin: Origin,
    pub dir: String,
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

fn read_widget(dir: &Path, origin: Origin) -> WidgetInfo {
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = dir.join(MANIFEST);
    let parsed = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))
        .and_then(|text| serde_json::from_str::<Manifest>(&text).with_context(|| format!("in {}", path.display())));
    match parsed {
        Ok(m) => WidgetInfo {
            size: WidgetSize { default: m.size.default, min: m.size.min.unwrap_or([1, 1]), max: m.size.max },
            id: m.id,
            name: m.name,
            version: m.version,
            author: m.author,
            description: m.description,
            resizable: m.resizable,
            permissions: m.permissions,
            preset: m.preset,
            origin,
            dir: dir.display().to_string(),
            error: None,
        },
        Err(e) => WidgetInfo {
            id: folder.clone(),
            name: folder,
            version: None,
            author: None,
            description: None,
            size: WidgetSize { default: [1, 1], min: [1, 1], max: None },
            resizable: true,
            permissions: Vec::new(),
            preset: desktop(),
            origin,
            dir: dir.display().to_string(),
            error: Some(format!("{e:#}")),
        },
    }
}

/// Subfolders of `root` that have a `widget.json` (`widgets/shell` has none).
fn widget_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort();
    dirs
}

/// The `widgets/` folder beside the exe or any parent, like the themes folder.
pub fn bundled_root() -> Option<PathBuf> {
    std::env::current_exe().ok()?.ancestors().skip(1).map(|dir| dir.join("widgets")).find(|dir| dir.is_dir())
}

/// Bundled widgets plus the user's; a user widget wins over a bundled one with the same id.
pub fn list_from(bundled: Option<&Path>, user: Option<&Path>) -> Vec<WidgetInfo> {
    let mut by_id: HashMap<String, WidgetInfo> = HashMap::new();
    for (root, origin) in [(bundled, Origin::Bundled), (user, Origin::User)] {
        for dir in root.map(widget_dirs).unwrap_or_default() {
            let info = read_widget(&dir, origin);
            by_id.insert(info.id.clone(), info);
        }
    }
    let mut widgets: Vec<WidgetInfo> = by_id.into_values().collect();
    widgets.sort_by_key(|w| w.name.to_lowercase());
    widgets
}

pub fn list() -> Vec<WidgetInfo> {
    let user = mypaper_config::dir().ok().map(|dir| dir.join("widgets"));
    list_from(bundled_root().as_deref(), user.as_deref())
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// The user layer (`widgets.json` → `appearance`).
    pub appearance: Look,
    pub instances: Vec<Instance>,
    /// Whether the topmost overlay layer is shown (`widgets.json` → `overlay`, default on).
    pub overlay: bool,
    /// False: there is no `widgets.json` yet.
    pub exists: bool,
}

fn skeleton() -> Map<String, Value> {
    let Value::Object(map) = json!({ "version": 1, "grid": { "cell": 0, "gap": 8, "margin": 16 }, "instances": [] }) else {
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
        return Ok(Layout { appearance: Look::default(), instances: Vec::new(), overlay: true, exists: false });
    };
    let mut doc = parse_object(text)?;
    let overlay = doc.get("overlay").and_then(Value::as_bool).unwrap_or(true);
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
    Ok(Layout { appearance, instances, overlay, exists: true })
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

pub fn layout_path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(LAYOUT_FILE))
}

pub fn load() -> Result<Layout> {
    let path = layout_path()?;
    read_layout(fsio::read_optional(&path)?.as_deref()).with_context(|| format!("in {}", path.display()))
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
        PostMessageW(Some(host), message, WPARAM(wparam), LPARAM(0)).context("posting to the widget host")?;
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
    use crate::source::tests::temp_dir;

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
        assert!(!layout.instances[0].appearance.is_empty() && layout.instances[1].appearance.is_empty());
        assert_eq!(layout.appearance.0.blur, Some(30.0));

        let none = read_layout(None).unwrap();
        assert!(!none.exists && none.instances.is_empty() && none.appearance.is_empty());
        assert!(read_layout(Some("[]")).is_err());
        assert!(read_layout(Some(r#"{ "appearance": { "glow": 1 } }"#)).is_err());
    }

    #[test]
    fn editing_the_global_look_keeps_instances_grid_and_unknown_fields() {
        let look: Look = serde_json::from_str(r##"{ "surface": "solid", "tint": "#202020", "opacity": 0.8, "accent": "auto" }"##).unwrap();
        let text = with_appearance(Some(LAYOUT), &look).unwrap();

        let mut expected = json(LAYOUT);
        expected["appearance"] = json(r##"{ "surface": "solid", "tint": "#202020", "opacity": 0.8, "accent": "auto" }"##);
        assert_eq!(json(&text), expected, "{text}");
        assert!(text.find("\"grid\"").unwrap() < text.find("\"appearance\"").unwrap(), "key order is kept");

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
        let look: Look = serde_json::from_str(r#"{ "style": "aero", "surface": "glass" }"#).unwrap();
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
        expected["instances"][0].as_object_mut().unwrap().remove("appearance");
        assert_eq!(reset, expected, "settings and unknown fields stay on the instance");
        assert!(with_instance_look_cleared(LAYOUT, "nope-9").is_err());
    }

    fn write_widget(root: &Path, folder: &str, manifest: &str) {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest).unwrap();
    }

    #[test]
    fn installed_widgets_merge_bundled_and_user_with_user_winning() {
        let work = temp_dir("widgets-list");
        let (bundled, user) = (work.join("bundled"), work.join("user"));
        write_widget(
            &bundled,
            "clock",
            r#"{ "id": "clock", "name": "Clock", "size": { "default": [2, 1], "max": [4, 2] }, "permissions": ["launch"], "settings": [] }"#,
        );
        write_widget(&bundled, "notes", r#"{ "id": "notes", "name": "Notes", "size": { "default": [2, 2] }, "resizable": false }"#);
        write_widget(&bundled, "broken", r#"{ "id": "broken" }"#);
        std::fs::create_dir_all(bundled.join("shell")).unwrap(); // not a widget
        write_widget(&user, "clock", r#"{ "id": "clock", "name": "My Clock", "size": { "default": [1, 1] } }"#);

        let widgets = list_from(Some(&bundled), Some(&user));
        let names: Vec<_> = widgets.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["broken", "My Clock", "Notes"]);
        let clock = &widgets[1];
        assert_eq!(clock.origin, Origin::User);
        assert_eq!(clock.size.min, [1, 1]);
        let notes = &widgets[2];
        assert!(!notes.resizable && notes.size.max.is_none() && notes.origin == Origin::Bundled);
        assert!(widgets[0].error.is_some(), "a broken manifest is listed with its error");

        let bundled_clock = list_from(Some(&bundled), None);
        let clock = bundled_clock.iter().find(|w| w.id == "clock").unwrap();
        assert_eq!((clock.size.default, clock.size.max, clock.permissions.as_slice()), ([2, 1], Some([4, 2]), ["launch".to_string()].as_slice()));
        std::fs::remove_dir_all(&work).unwrap();
    }
}
