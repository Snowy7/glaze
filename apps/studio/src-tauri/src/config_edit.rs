//! Edits to `%APPDATA%\mypaper\config.json` that touch only the keys they set. The engine, the
//! tray and Studio all write this file, so Studio reads it as a JSON object, sets its keys, checks
//! the result with the engine's own [`Config`] parser and writes it atomically. A custom
//! desktop-menu entry, a key it didn't touch and the order of the keys all survive, and a file
//! the engine would refuse (a typo, an unknown key) is reported and never overwritten.

use std::path::Path;

use anyhow::{Context, Result, bail};
use mypaper_config::Config;
use mypaper_config::desktop_menu::{DesktopMenu, Entry, default_items};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::fsio;

/// A partial config update; absent fields are left alone.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ConfigPatch {
    pub theme: Option<String>,
    pub paused: Option<bool>,
    pub pause_on_battery: Option<bool>,
    pub pause_on_fullscreen: Option<bool>,
    pub static_snapshot: Option<bool>,
    pub classic_context_menu: Option<bool>,
    pub desktop_menu: Option<MenuPatch>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MenuPatch {
    pub enabled: Option<bool>,
    /// The whole list, top to bottom. The default list is written as "no `items`".
    pub items: Option<Vec<Entry>>,
}

fn set(map: &mut Map<String, Value>, key: &str, value: impl Into<Value>) {
    map.insert(key.to_owned(), value.into());
}

impl ConfigPatch {
    fn apply(self, map: &mut Map<String, Value>) -> Result<()> {
        if let Some(theme) = self.theme {
            set(map, "theme", theme);
        }
        for (key, value) in [
            ("paused", self.paused),
            ("pause_on_battery", self.pause_on_battery),
            ("pause_on_fullscreen", self.pause_on_fullscreen),
            ("static_snapshot", self.static_snapshot),
            ("classic_context_menu", self.classic_context_menu),
        ] {
            if let Some(value) = value {
                set(map, key, value);
            }
        }
        if let Some(menu) = self.desktop_menu {
            let section = map
                .entry("desktop_menu")
                .or_insert_with(|| Value::Object(Map::new()));
            let Value::Object(section) = section else {
                bail!("desktop_menu must be an object")
            };
            if let Some(enabled) = menu.enabled {
                set(section, "enabled", enabled);
            }
            if let Some(items) = menu.items {
                if items == default_items() {
                    section.shift_remove("items");
                } else {
                    set(section, "items", serde_json::to_value(items)?);
                }
            }
        }
        Ok(())
    }
}

/// Applies `patch` to the file at `path` (no file: a new one holding just the patch) and returns
/// the config as the engine will read it.
pub fn update_at(path: &Path, patch: ConfigPatch) -> Result<Config> {
    let existing = fsio::read_optional(path)?;
    let mut map = match &existing {
        Some(text) => {
            Config::parse(text).with_context(|| {
                format!(
                    "{} can't be read, so Studio won't change it",
                    path.display()
                )
            })?;
            match serde_json::from_str(text)? {
                Value::Object(map) => map,
                _ => bail!("{} must hold a JSON object", path.display()),
            }
        }
        None => Map::new(),
    };
    let before = map.clone();
    patch.apply(&mut map)?;
    let text = serde_json::to_string_pretty(&map)? + "\n";
    let config = Config::parse(&text).context("these settings were refused")?;
    // Skip no-op saves: each real save makes the engine reload.
    if map == before && (existing.is_some() || map.is_empty()) {
        return Ok(config);
    }
    fsio::write_atomic(path, text)?;
    Ok(config)
}

/// `config.json` as the page shows it: every setting spelled out, with the defaults the file
/// leaves out filled in.
#[derive(Debug, Serialize)]
pub struct ConfigState {
    pub theme: String,
    pub paused: bool,
    pub pause_on_battery: bool,
    pub pause_on_fullscreen: bool,
    pub static_snapshot: bool,
    pub classic_context_menu: bool,
    pub desktop_menu: MenuState,
    /// The menu without `items`, for "Reset order".
    pub desktop_menu_default: Vec<Entry>,
}

/// `desktop_menu` with its items always present (the config type leaves a default list out).
#[derive(Debug, Serialize)]
pub struct MenuState {
    pub enabled: bool,
    pub items: Vec<Entry>,
}

impl From<Config> for ConfigState {
    fn from(config: Config) -> Self {
        let DesktopMenu { enabled, items } = config.desktop_menu;
        Self {
            theme: config.theme,
            paused: config.paused,
            pause_on_battery: config.pause_on_battery,
            pause_on_fullscreen: config.pause_on_fullscreen,
            static_snapshot: config.static_snapshot,
            classic_context_menu: config.classic_context_menu,
            desktop_menu: MenuState { enabled, items },
            desktop_menu_default: default_items(),
        }
    }
}

#[cfg(test)]
mod tests {
    use mypaper_config::desktop_menu::{Builtin, Custom};

    use super::*;
    use crate::source::tests::temp_dir;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    const FILE: &str = r#"{
  "theme": "photo",
  "paused": false,
  "pause_on_battery": true,
  "pause_on_fullscreen": true,
  "desktop_menu": {
    "enabled": false,
    "items": ["refresh", { "label": "Notes", "command": "notepad.exe", "args": ["D:\\notes.txt"] }, "separator", "more"]
  },
  "classic_context_menu": false,
  "static_snapshot": true
}"#;

    fn file(dir: &Path, text: &str) -> std::path::PathBuf {
        let path = dir.join("config.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_patch_changes_only_the_keys_it_names() {
        let dir = temp_dir("config-patch");
        let path = file(&dir, FILE);

        let config = update_at(
            &path,
            ConfigPatch {
                static_snapshot: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!config.static_snapshot);
        let mut expected = json(FILE);
        expected["static_snapshot"] = false.into();
        assert_eq!(json(&std::fs::read_to_string(&path).unwrap()), expected);

        // Turning the menu on keeps its custom entry and its order.
        let on = ConfigPatch {
            desktop_menu: Some(MenuPatch {
                enabled: Some(true),
                items: None,
            }),
            ..Default::default()
        };
        let config = update_at(&path, on).unwrap();
        assert!(config.desktop_menu.enabled);
        expected["desktop_menu"]["enabled"] = true.into();
        assert_eq!(json(&std::fs::read_to_string(&path).unwrap()), expected);
        assert!(
            matches!(&config.desktop_menu.items[1], Entry::Custom(c) if c.label == "Notes" && c.args == ["D:\\notes.txt"])
        );

        // Reordering rewrites the list; the other keys and their order stay.
        let mut items = config.desktop_menu.items;
        items.swap(0, 3);
        let reorder = ConfigPatch {
            desktop_menu: Some(MenuPatch {
                enabled: None,
                items: Some(items),
            }),
            ..Default::default()
        };
        update_at(&path, reorder).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let written = json(&text);
        assert_eq!(written["desktop_menu"]["items"][0], "more");
        assert_eq!(written["desktop_menu"]["items"][3], "refresh");
        assert_eq!(
            written["desktop_menu"]["items"][1]["command"],
            "notepad.exe"
        );
        assert_eq!(written["theme"], "photo");
        assert!(text.find("\"theme\"").unwrap() < text.find("\"desktop_menu\"").unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_default_menu_is_written_as_no_items_and_a_new_file_holds_only_the_patch() {
        let dir = temp_dir("config-default-menu");
        let path = file(&dir, FILE);
        let reset = ConfigPatch {
            desktop_menu: Some(MenuPatch {
                enabled: None,
                items: Some(default_items()),
            }),
            ..Default::default()
        };
        let config = update_at(&path, reset).unwrap();
        assert!(config.desktop_menu.items == default_items());
        assert!(
            json(&std::fs::read_to_string(&path).unwrap())["desktop_menu"]
                .get("items")
                .is_none()
        );

        let fresh = dir.join("fresh").join("config.json");
        std::fs::create_dir_all(fresh.parent().unwrap()).unwrap();
        update_at(
            &fresh,
            ConfigPatch {
                classic_context_menu: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            json(&std::fs::read_to_string(&fresh).unwrap()),
            json(r#"{ "classic_context_menu": true }"#)
        );

        // Nothing to change writes nothing, not even a file.
        let none = dir.join("none.json");
        update_at(&none, ConfigPatch::default()).unwrap();
        assert!(!none.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_the_engine_would_refuse_is_never_overwritten() {
        let dir = temp_dir("config-refused");
        let newer = r#"{ "theme": "photo", "frosting": 2 }"#;
        let path = file(&dir, newer);
        let error = update_at(
            &path,
            ConfigPatch {
                paused: Some(true),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("won't change it"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);

        // A patch that would make the config invalid is refused before anything is written.
        let good = file(&dir, FILE);
        let custom = Entry::Custom(Custom {
            label: " ".into(),
            command: "x".into(),
            args: Vec::new(),
        });
        let bad = ConfigPatch {
            desktop_menu: Some(MenuPatch {
                enabled: None,
                items: Some(vec![Entry::Builtin(Builtin::More), custom]),
            }),
            ..Default::default()
        };
        assert!(update_at(&good, bad).is_err());
        assert_eq!(std::fs::read_to_string(&good).unwrap(), FILE);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_state_spells_out_defaults_the_file_leaves_out() {
        let state = ConfigState::from(Config::parse("{}").unwrap());
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["static_snapshot"], true);
        assert_eq!(value["classic_context_menu"], false);
        assert_eq!(value["desktop_menu"]["enabled"], false);
        assert_eq!(
            value["desktop_menu"]["items"].as_array().unwrap().len(),
            default_items().len()
        );
        assert_eq!(
            value["desktop_menu_default"],
            value["desktop_menu"]["items"]
        );
    }

    #[test]
    fn patches_deserialize_from_partial_json() {
        let patch: ConfigPatch = serde_json::from_str(
            r#"{ "pause_on_fullscreen": false, "desktop_menu": { "enabled": true } }"#,
        )
        .unwrap();
        assert_eq!(patch.pause_on_fullscreen, Some(false));
        assert!(patch.theme.is_none() && patch.paused.is_none());
        assert!(
            patch
                .desktop_menu
                .is_some_and(|m| m.enabled == Some(true) && m.items.is_none())
        );
    }
}
