//! `%APPDATA%\mypaper\bars.json` (docs/bars.md, "Config"). Studio handles the file as the JSON
//! document it is: the page edits it, and what Studio's Rust side adds is the host's own schema
//! (`mypaper-bars-schema`, which compiles `crates/bars`' source) to refuse anything the bars host
//! would refuse. So nothing is mirrored here, and nothing the page doesn't touch is dropped: the
//! document goes out and comes back as `serde_json::Value`, key order included.
//!
//! Also here: the catalog the page merges against (the defaults, the presets and the geometry
//! defaults), so the page shows the same effective values the host computes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mypaper_bars_schema::config::{DockStyle, TopLayout};
use mypaper_bars_schema::presets::{DockDesign, TopDesign};
use mypaper_bars_schema::style::{COLOR_TOKENS, NAMED_CURVES};
use mypaper_bars_schema::{BarsConfig, Preset};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::fsio;

pub const FILE: &str = "bars.json";

pub type Doc = Map<String, Value>;

/// What the Bars page needs to show the file.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarsState {
    /// The file as written (`{ "version": 1 }` when there is none yet).
    pub doc: Doc,
    /// False: no file yet; the first change creates it.
    pub exists: bool,
    pub path: String,
    /// Why the bars host would refuse this file, when it would. The page then shows the problem
    /// instead of the editors: nothing can be saved until the file parses again.
    pub problem: Option<String>,
}

pub fn path() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join(FILE))
}

fn blank() -> Doc {
    Doc::from_iter([("version".to_owned(), Value::from(1))])
}

fn parse_doc(text: &str) -> Result<Doc> {
    match serde_json::from_str(text).context("bars.json is not valid JSON")? {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("bars.json must hold a JSON object"),
    }
}

fn render(doc: &Doc) -> Result<String> {
    Ok(serde_json::to_string_pretty(doc)? + "\n")
}

pub fn load_from(path: &Path) -> Result<BarsState> {
    let text = fsio::read_optional(path)?;
    let (doc, problem) = match &text {
        Some(text) => (
            parse_doc(text).with_context(|| format!("in {}", path.display()))?,
            BarsConfig::parse(text).err().map(|e| format!("{e:#}")),
        ),
        None => (blank(), None),
    };
    Ok(BarsState {
        doc,
        exists: text.is_some(),
        path: path.display().to_string(),
        problem,
    })
}

/// Checks `doc` with the host's schema and writes it. A file that doesn't parse is never replaced
/// (it may be mid-edit in a text editor, or written by a newer version): fix or reset it first.
pub fn save_to(path: &Path, doc: &Doc) -> Result<BarsState> {
    let text = render(doc)?;
    BarsConfig::parse(&text).context("the bars host would refuse these settings")?;
    let existing = fsio::read_optional(path)?;
    if let Some(existing) = &existing {
        BarsConfig::parse(existing).with_context(|| {
            format!(
                "{} can't be read, so Studio won't overwrite it",
                path.display()
            )
        })?;
        // The same document in other formatting isn't a change.
        if parse_doc(existing).is_ok_and(|current| current == *doc) {
            return load_from(path);
        }
    }
    fsio::write_atomic(path, text)?;
    load_from(path)
}

/// Replaces the file with an empty config (all defaults).
pub fn reset_at(path: &Path) -> Result<BarsState> {
    fsio::write_atomic(path, render(&blank())?)?;
    load_from(path)
}

/// Studio-only facts about bars.json the page can't derive: the host's defaults, its presets and
/// the geometry defaults that depend on the layout.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    /// `BarsConfig::default()`, the bottom layer of every merge.
    pub defaults: Value,
    pub presets: Vec<PresetInfo>,
    /// Each bar's designs: a partial `top` (or `dock`) with its look and motion. Studio layers
    /// them as the host does (`presets::layered`): design, preset, then the file.
    pub top_designs: Vec<PresetInfo>,
    pub dock_designs: Vec<PresetInfo>,
    /// Margin, padding, spacing (and size) a missing field takes, per top layout and dock style.
    pub geometry: Value,
    pub named_curves: Vec<&'static str>,
    pub color_tokens: Vec<&'static str>,
    /// For each preset, the `look.background` types a file can choose (see [`background_types`]).
    pub background_types: Value,
}

#[derive(Debug, Serialize)]
pub struct PresetInfo {
    pub id: String,
    /// The preset's partial `bars.json`.
    pub bundle: Value,
}

/// Each entry's name (its serialized id) with its bundle.
fn infos<T: Serialize>(all: &[T], bundle: impl Fn(&T) -> Value) -> Result<Vec<PresetInfo>> {
    all.iter()
        .map(|entry| {
            let Value::String(id) = serde_json::to_value(entry)? else {
                anyhow::bail!("a preset or design serializes to its name")
            };
            Ok(PresetInfo {
                id,
                bundle: bundle(entry),
            })
        })
        .collect()
}

pub fn catalog() -> Result<Catalog> {
    Ok(Catalog {
        defaults: serde_json::to_value(BarsConfig::default())?,
        presets: infos(&Preset::ALL, |p| p.bundle())?,
        top_designs: infos(&TopDesign::ALL, |d| d.bundle())?,
        dock_designs: infos(&DockDesign::ALL, |d| d.bundle())?,
        geometry: geometry()?,
        named_curves: NAMED_CURVES.to_vec(),
        color_tokens: COLOR_TOKENS.to_vec(),
        background_types: background_types()?,
    })
}

/// Which background types each preset can take, asked of the host's parser rather than restating
/// its merge rules. Since a background of another `type` replaces the preset's whole, that is
/// every type today; the UI still reads this so a future restriction shows up by itself.
fn background_types() -> Result<Value> {
    let samples = [
        ("none", json!({ "type": "none" })),
        ("solid", json!({ "type": "solid", "color": "#000000" })),
        (
            "gradient",
            json!({ "type": "gradient", "stops": ["#000000", "#ffffff"] }),
        ),
        (
            "image",
            json!({ "type": "image", "path": "background.png" }),
        ),
        ("wallpaper", json!({ "type": "wallpaper" })),
        ("blur", json!({ "type": "blur" })),
    ];
    let mut by_preset = Map::new();
    for preset in Preset::ALL {
        let Value::String(id) = serde_json::to_value(preset)? else {
            anyhow::bail!("a preset serializes to its name")
        };
        let allowed: Vec<&str> = samples
            .iter()
            .filter(|(_, background)| {
                let doc = json!({ "preset": id, "look": { "background": background } });
                BarsConfig::parse(&doc.to_string()).is_ok()
            })
            .map(|(name, _)| *name)
            .collect();
        by_preset.insert(id, json!(allowed));
    }
    Ok(Value::Object(by_preset))
}

/// What `BarsConfig::resolve` fills in for each layout and dock style, asked of the host's code.
fn geometry() -> Result<Value> {
    let resolved = |layout: TopLayout, style: DockStyle| -> Result<Value> {
        let mut config = BarsConfig::default();
        config.top.layout = layout;
        config.dock.style = style;
        config.resolve(Vec::new);
        Ok(serde_json::to_value(&config)?)
    };
    let top = |layout: TopLayout| -> Result<Value> {
        let config = resolved(layout, DockStyle::Floating)?;
        Ok(json!({
            "margin": config["top"]["margin"],
            "padding": config["top"]["padding"],
            "spacing": config["top"]["spacing"],
        }))
    };
    let dock = |style: DockStyle| -> Result<Value> {
        let config = resolved(TopLayout::Strip, style)?;
        Ok(json!({
            "size": config["dock"]["size"],
            "margin": config["dock"]["margin"],
            "padding": config["dock"]["padding"],
            "spacing": config["dock"]["spacing"],
        }))
    };
    Ok(json!({
        "top": {
            "strip": top(TopLayout::Strip)?,
            "floating": top(TopLayout::Floating)?,
            "islands": top(TopLayout::Islands)?,
        },
        "dock": {
            "floating": dock(DockStyle::Floating)?,
            "taskbar": dock(DockStyle::Taskbar)?,
        },
    }))
}

#[cfg(test)]
mod tests {
    use mypaper_bars_schema::config::{
        Action, BuiltinAction, Dimension, IndicatorStyle, LauncherLayout, ModuleRef, Monitors,
        Running,
    };

    use super::*;

    /// Writes the catalog for the screenshot harness (apps/studio/tools/shots/catalog.json):
    /// `STUDIO_CATALOG_OUT=<path> cargo test write_catalog -- --ignored`.
    #[test]
    #[ignore = "a tool, not a check"]
    fn write_catalog_for_the_screenshot_harness() {
        let out = std::env::var("STUDIO_CATALOG_OUT").expect("set STUDIO_CATALOG_OUT");
        std::fs::write(out, serde_json::to_string(&catalog().unwrap()).unwrap()).unwrap();
    }
    use crate::source::tests::temp_dir;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// A `bars.json` that uses every feature of the schema, as the docs and presets spell them.
    const EVERYTHING: &str = r##"{
  "version": 1,
  "preset": "floating",
  "taskbar": { "mode": "hide", "alignment": "left" },
  "top": {
    "enabled": true, "monitors": ["\\\\.\\DISPLAY2", "DISPLAY3"], "layout": "islands", "position": "bottom",
    "height": 36, "margin": 6, "padding": 5, "spacing": 3, "width": "60%",
    "left": ["start", "search", "|", "workspaces", "app"],
    "center": ["clock", "custom:weather"],
    "right": ["media", "|", "tray", "language", "notifications", "network", "volume", "battery", "stats", "quick", "power"],
    "clock": { "seconds": true, "format": "HH:mm\nd/M/yyyy" }
  },
  "dock": {
    "enabled": true, "monitors": "all", "style": "taskbar", "position": "left",
    "size": 28, "margin": 2, "padding": 10, "spacing": 5, "autohide": true,
    "magnify": { "enabled": true, "scale": 1.7, "range": 4 }, "running": "monitor",
    "split": true, "counter": false, "titles": true, "indicator": "pill",
    "preview": { "enabled": true, "delay": 300 }, "bounce": false, "attention": "bounce",
    "extras": ["desktop", "trash"], "align": "start",
    "left": ["start", "search", "|", "custom:notes"], "right": ["tray", "language", "clock"],
    "pinned": [
      { "path": "C:\\Windows\\explorer.exe", "name": "File Explorer" },
      { "path": "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App" }
    ]
  },
  "launcher": { "winKey": false, "layout": "dashboard", "files": false, "pinned": [ { "path": "C:\\Tools\\thing.exe", "name": "Thing" } ] },
  "tray": { "host": true },
  "calendar": { "zones": [ { "zone": "America/New_York" }, { "zone": "Asia/Tokyo", "label": "Tokyo office" } ], "firstDay": "monday" },
  "audio": { "switchOutput": true },
  "look": {
    "scheme": "dark",
    "background": { "type": "gradient", "kind": "radial", "angle": 200, "stops": ["#101820", { "color": "accent", "at": 0.6 }, "var(--tone)"] },
    "tint": 0.5, "saturation": 1.4, "noise": 0.04, "radius": 14, "corners": "squircle",
    "border": { "rest": { "width": 1, "color": { "kind": "conic", "angle": 120, "stops": ["#ff2bd6", "#00e5ff"] } }, "hover": { "width": 1.5 }, "active": { "color": "system-accent-light-2" } },
    "glow": { "rest": { "color": "#a04cff", "size": 10, "opacity": 0.38 }, "hover": { "color": "accent", "size": 14, "opacity": 0.6 } },
    "shadow": { "rest": { "x": 0, "y": 3, "blur": 10, "spread": -2, "color": "#00000052" }, "active": { "y": 1 } },
    "items": {
      "radius": 8,
      "rest": { "color": "text", "scale": 1 },
      "hover": { "background": "#ffffff1a", "scale": 1.04, "glow": { "color": "#ff2bd6", "size": 14, "opacity": 0.55 } },
      "active": { "background": { "angle": 90, "stops": ["#ff2bd640", "#00e5ff40"] }, "border": { "width": 1, "color": "#ffffff33" }, "color": "#ffffff" }
    },
    "indicator": { "running": "#ffffff73", "active": "accent", "attention": "#ffb340" }
  },
  "animation": {
    "enabled": true, "duration": 200, "curve": "smooth", "reduceMotion": "always",
    "bar": {
      "style": "slide",
      "top": { "duration": 240, "hover": { "enabled": false } },
      "dock": {
        "curve": { "mass": 1, "stiffness": 220, "damping": 20 },
        "magnify": { "duration": 70, "curve": "linear" }, "bounce": { "duration": 520, "curve": [0.3, 0, 0.4, 1] },
        "attention": { "curve": "ease-in-out" }, "hover": { "enabled": false }, "press": { "curve": "spring-bouncy" }, "open": { "style": "scale" }
      }
    },
    "hover": { "duration": 120 }, "press": { "curve": "snappy" }, "open": { "style": "pop", "duration": 160, "curve": "overshoot" }
  },
  "modules": {
    "clock": {
      "click": {
        "left": "notifications", "middle": { "exec": "notepad.exe", "args": ["todo.txt"] },
        "right": { "open": "ms-settings:dateandtime" }, "wheelUp": "volume-up", "wheelDown": "default"
      },
      "style": { "color": "accent", "background": "#ffffff10", "border": { "width": 1, "color": "#ffffff20" }, "padding": 12, "radius": 8, "size": 13, "weight": 650 }
    },
    "quick": { "icon": "C:\\Icons\\sliders.svg" },
    "search": { "click": { "left": "none" } },
    "custom:weather": {
      "command": "curl.exe", "args": ["-s", "wttr.in/?format=%c+%t"], "interval": 900,
      "click": { "left": { "open": "https://wttr.in" } }
    },
    "custom:notes": { "command": "powershell.exe", "icon": "notes" }
  },
  "icons": "My icons",
  "performance": "auto",
  "vars": { "--glow": "#00ffaa", "--glow-size": 18, "--labels": true, "--shape": "sharp" },
  "appearance": { "surface": "glass", "tint": "#101010", "opacity": 0.5, "blur": 30, "radius": 12, "border": true, "text": "light", "accent": "auto", "font": "Segoe UI Variable", "scale": 1, "shadow": false },
  "overrides": {
    "secondary": { "top": { "enabled": false } },
    "DISPLAY3": { "preset": "macos", "dock": { "enabled": true, "position": "right", "size": 40 }, "performance": "on" }
  }
}"##;

    fn write(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join(FILE);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_file_using_every_feature_is_read_and_written_back_exactly() {
        let dir = temp_dir("bars-everything");
        let path = write(&dir, EVERYTHING);

        let state = load_from(&path).unwrap();
        assert_eq!(state.problem, None);
        assert!(state.exists);
        assert_eq!(Value::Object(state.doc.clone()), json(EVERYTHING));

        // The host's own types understand every part of it.
        let config = BarsConfig::parse(EVERYTHING).unwrap();
        assert_eq!(config.top.width, Some(Dimension::Percent(60.0)));
        assert_eq!(config.top.center[1], ModuleRef::Custom("weather".into()));
        assert_eq!(config.top.left[2], ModuleRef::Break);
        assert_eq!(config.dock.running, Running::Monitor);
        assert_eq!(config.dock.indicator, IndicatorStyle::Pill);
        assert_eq!(config.launcher.layout, LauncherLayout::Dashboard);
        assert!(config.tray.host && config.audio.switch_output);
        assert!(matches!(config.dock.monitors, Monitors::All));
        let click = &config.modules[&ModuleRef::Builtin(mypaper_bars_schema::Module::Clock)].click;
        assert_eq!(
            click.left,
            Some(Action::Builtin(BuiltinAction::Notifications))
        );
        assert!(matches!(click.middle, Some(Action::Exec { .. })));
        assert!(matches!(click.right, Some(Action::Open { .. })));
        assert_eq!(config.custom_modules().count(), 2);

        // Saving what was read changes nothing: not the document, not the bytes on disk.
        let saved = save_to(&path, &state.doc).unwrap();
        assert_eq!(Value::Object(saved.doc), json(EVERYTHING));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EVERYTHING);

        // An edit touches only what it set, including inside the deepest sections.
        let mut doc = state.doc;
        doc["look"]["tint"] = 0.3.into();
        doc["animation"]["bar"]["dock"]["bounce"]["duration"] = 480.into();
        save_to(&path, &doc).unwrap();
        let mut expected = json(EVERYTHING);
        expected["look"]["tint"] = 0.3.into();
        expected["animation"]["bar"]["dock"]["bounce"]["duration"] = 480.into();
        assert_eq!(json(&std::fs::read_to_string(&path).unwrap()), expected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_preset_is_a_valid_file_and_survives_a_round_trip() {
        let dir = temp_dir("bars-presets");
        for preset in Preset::ALL {
            let mut doc = Doc::new();
            doc.insert("version".into(), 1.into());
            doc.insert("preset".into(), serde_json::to_value(preset).unwrap());
            let path = write(&dir, &render(&doc).unwrap());
            assert_eq!(load_from(&path).unwrap().problem, None, "{preset:?}");

            // The bundle itself, written out as the user's own file.
            let Value::Object(mut bundle) = preset.bundle() else {
                panic!("{preset:?} is an object")
            };
            // Merged over the default preset, a bundle would keep that preset's leftover keys, so
            // as a standalone file it names its own preset.
            bundle.insert("preset".into(), serde_json::to_value(preset).unwrap());
            let saved = save_to(&path, &bundle).unwrap();
            assert_eq!(saved.problem, None, "{preset:?}");
            assert_eq!(
                Value::Object(saved.doc),
                Value::Object(bundle),
                "{preset:?}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The JSON blocks of docs/bars.md that are `bars.json` documents or fragments of one.
    fn doc_examples() -> Vec<Value> {
        let docs = include_str!("../../../../docs/bars.md");
        let mut blocks = Vec::new();
        let mut parts = docs.split("```json");
        parts.next();
        for part in parts {
            let Some((block, _)) = part.split_once("```") else {
                continue;
            };
            let block = block.trim();
            // A bars.json fragment is `"key": …`; a whole file starts with `{ "version"`.
            let text = if block.starts_with('"') {
                format!("{{ {block} }}")
            } else {
                block.to_owned()
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if value.get("version").is_some() || block.starts_with('"') {
                blocks.push(value);
            }
        }
        blocks
    }

    #[test]
    fn the_examples_in_the_docs_are_valid_files() {
        let examples = doc_examples();
        assert!(
            examples.len() >= 5,
            "found only {} examples in docs/bars.md",
            examples.len()
        );
        let dir = temp_dir("bars-docs");
        for example in examples {
            let path = write(&dir, &serde_json::to_string_pretty(&example).unwrap());
            let state = load_from(&path).unwrap();
            assert_eq!(state.problem, None, "{example}");
            assert_eq!(Value::Object(state.doc.clone()), example);
            save_to(&path, &state.doc).unwrap();
            assert_eq!(json(&std::fs::read_to_string(&path).unwrap()), example);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_the_host_would_refuse_is_reported_and_never_overwritten() {
        let dir = temp_dir("bars-refused");
        // A field from a newer version: the host rejects the file, Studio shows why and keeps it.
        let newer = r#"{ "version": 1, "top": { "height": 30, "frosting": 3 } }"#;
        let path = write(&dir, newer);
        let state = load_from(&path).unwrap();
        assert!(
            state
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("frosting")),
            "{:?}",
            state.problem
        );
        assert_eq!(state.doc["top"]["frosting"], 3);
        assert!(save_to(&path, &state.doc).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);

        // A value the host checks.
        let good = write(&dir, r#"{ "version": 1 }"#);
        let mut doc = load_from(&good).unwrap().doc;
        doc.insert("look".into(), json(r#"{ "tint": 3 }"#));
        let error = format!("{:#}", save_to(&good, &doc).unwrap_err());
        assert!(error.contains("look.tint"), "{error}");
        doc.insert(
            "look".into(),
            json(r#"{ "background": { "type": "image", "path": "" } }"#),
        );
        assert!(save_to(&good, &doc).is_err());
        doc.insert(
            "top".into(),
            json(r#"{ "left": ["clock"], "right": ["clock"] }"#),
        );
        doc.remove("look");
        assert!(format!("{:#}", save_to(&good, &doc).unwrap_err()).contains("twice"));
        assert_eq!(
            std::fs::read_to_string(&good).unwrap(),
            r#"{ "version": 1 }"#
        );

        // Text that isn't JSON at all is an error, not a "problem": there is no document to show.
        let broken = write(&dir, "{ nope");
        assert!(load_from(&broken).is_err());
        assert_eq!(reset_at(&broken).unwrap().problem, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_file_shows_an_empty_config_and_the_first_save_creates_it() {
        let dir = temp_dir("bars-missing");
        let path = dir.join("mypaper").join(FILE);
        let state = load_from(&path).unwrap();
        assert!(!state.exists && state.problem.is_none());
        assert_eq!(
            Value::Object(state.doc.clone()),
            json(r#"{ "version": 1 }"#)
        );

        let mut doc = state.doc;
        doc.insert("dock".into(), json(r#"{ "enabled": true }"#));
        let saved = save_to(&path, &doc).unwrap();
        assert!(saved.exists);
        assert!(!path.with_file_name("bars.json.studio.tmp").exists());
        assert_eq!(
            json(&std::fs::read_to_string(&path).unwrap()),
            json(r#"{ "version": 1, "dock": { "enabled": true } }"#)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_catalog_carries_the_hosts_defaults_presets_and_geometry() {
        let catalog = catalog().unwrap();
        let ids: Vec<&str> = catalog.presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "monochrome",
                "macos",
                "windows11",
                "windows10",
                "chromeos",
                "islands",
                "neon-glass",
                "floating"
            ]
        );
        assert_eq!(catalog.top_designs.len(), TopDesign::ALL.len());
        assert_eq!(catalog.dock_designs[2].id, "windows11");
        assert_eq!(catalog.dock_designs[2].bundle["style"], "taskbar");
        assert_eq!(catalog.defaults["top"]["height"], 32.0);
        assert_eq!(catalog.defaults["dock"]["magnify"]["scale"], 1.5);
        assert_eq!(catalog.geometry["top"]["floating"]["margin"], 8.0);
        assert_eq!(catalog.geometry["top"]["islands"]["padding"], 4.0);
        assert_eq!(catalog.geometry["dock"]["taskbar"]["size"], 24.0);
        assert_eq!(catalog.geometry["dock"]["floating"]["padding"], 5.0);
        assert!(
            catalog.named_curves.contains(&"spring-bouncy")
                && catalog.color_tokens.contains(&"accent")
        );
    }

    #[test]
    fn every_background_type_is_available_over_every_preset() {
        // A background of another type replaces the preset's whole (the host's merge treats a
        // changed `type` tag as a new variant), so nothing the preset left behind gets in the way.
        let background = |preset: &str, background: &str| {
            BarsConfig::parse(&format!(
                r#"{{ "preset": "{preset}", "look": {{ "background": {background} }} }}"#
            ))
            .is_ok()
        };
        assert!(background(
            "monochrome",
            r#"{ "type": "wallpaper", "brightness": 1 }"#
        ));
        assert!(background(
            "monochrome",
            r##"{ "type": "solid", "color": "#000" }"##
        ));
        assert!(background("islands", r#"{ "type": "wallpaper" }"#));

        let catalog = background_types().unwrap();
        let all = json(r#"["none", "solid", "gradient", "image", "wallpaper", "blur"]"#);
        for preset in [
            "monochrome",
            "macos",
            "windows11",
            "islands",
            "neon-glass",
            "floating",
        ] {
            assert_eq!(catalog[preset], all, "{preset}");
        }
    }

    /// The quoted strings of `export const <name> = [ … ] as const` in the page's schema file.
    fn ts_list(name: &str) -> Vec<String> {
        let source = include_str!("../../src/lib/barsSchema.ts");
        let start = source
            .find(&format!("export const {name} = ["))
            .unwrap_or_else(|| panic!("{name} is not in barsSchema.ts"));
        let body = &source[start..];
        let body = &body[body.find('[').unwrap() + 1..body.find("] as const").unwrap()];
        body.split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn the_pages_lists_only_name_things_the_schema_accepts() {
        let parses = |what: &str, doc: String| {
            let result = BarsConfig::parse(&doc);
            assert!(result.is_ok(), "{what}: {:#}", result.unwrap_err());
        };
        for module in ts_list("BUILTIN_MODULES") {
            parses(
                &module,
                format!(r#"{{ "top": {{ "left": ["{module}"], "center": [], "right": [] }} }}"#),
            );
        }
        for action in ts_list("BUILTIN_ACTIONS") {
            parses(
                &action,
                format!(
                    r#"{{ "modules": {{ "clock": {{ "click": {{ "left": "{action}" }} }} }} }}"#
                ),
            );
        }
        for curve in ts_list("NAMED_CURVES") {
            assert!(NAMED_CURVES.contains(&curve.as_str()), "{curve}");
        }
        assert_eq!(ts_list("NAMED_CURVES"), NAMED_CURVES);
        for (key, values) in [
            ("layout", ts_list("TOP_LAYOUTS")),
            ("position", ts_list("TOP_POSITIONS")),
        ] {
            for value in values {
                parses(&value, format!(r#"{{ "top": {{ "{key}": "{value}" }} }}"#));
            }
        }
        for (key, values) in [
            ("style", ts_list("DOCK_STYLES")),
            ("position", ts_list("DOCK_POSITIONS")),
            ("indicator", ts_list("INDICATORS")),
            ("attention", ts_list("ATTENTIONS")),
        ] {
            for value in values {
                parses(&value, format!(r#"{{ "dock": {{ "{key}": "{value}" }} }}"#));
            }
        }
        for value in ts_list("LAUNCHER_LAYOUTS") {
            parses(
                &value,
                format!(r#"{{ "launcher": {{ "layout": "{value}" }} }}"#),
            );
        }
        for value in ts_list("CORNERS") {
            parses(
                &value,
                format!(r#"{{ "look": {{ "corners": "{value}" }} }}"#),
            );
        }
        for value in ts_list("MOTION_STYLES") {
            parses(
                &value,
                format!(r#"{{ "animation": {{ "bar": {{ "style": "{value}" }} }} }}"#),
            );
        }
        for value in ts_list("TOP_DESIGNS") {
            parses(&value, format!(r#"{{ "top": {{ "design": "{value}" }} }}"#));
        }
        for value in ts_list("DOCK_DESIGNS") {
            parses(
                &value,
                format!(r#"{{ "dock": {{ "design": "{value}" }} }}"#),
            );
        }
        for value in ts_list("GLASS_STYLES") {
            parses(
                &value,
                format!(r#"{{ "top": {{ "look": {{ "glass": "{value}" }} }} }}"#),
            );
        }
        for value in ts_list("FAMILIES") {
            parses(
                &value,
                format!(r#"{{ "dock": {{ "look": {{ "family": "{value}" }} }} }}"#),
            );
        }
        for value in ts_list("COLOR_TOKENS") {
            assert!(COLOR_TOKENS.contains(&value.as_str()), "{value}");
        }
    }
}
