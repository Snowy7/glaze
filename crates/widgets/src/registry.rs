//! Installed widgets and plugins: the user's folders first, then the bundled
//! ones; the first folder with a given id wins.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::manifest::{PLUGIN_MANIFEST, PluginManifest, WidgetManifest};

pub struct Installed {
    pub manifest: WidgetManifest,
}

/// Every installed widget, by id.
#[derive(Default)]
pub struct Registry {
    pub widgets: BTreeMap<String, Installed>,
}

/// Where plugins live: `%APPDATA%\mypaper\plugins`, then the bundled `plugins/`.
pub fn plugin_roots() -> Vec<PathBuf> {
    roots(
        crate::files::user("plugins").ok(),
        crate::files::bundled_plugins(),
    )
}

fn roots(user: Option<PathBuf>, bundled: Option<PathBuf>) -> Vec<PathBuf> {
    user.into_iter()
        .chain(bundled)
        .filter(|dir| dir.is_dir())
        .collect()
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.widgets.get(id)
    }

    /// The installed widget that replaced `old` (its manifest's `replaces`), with the settings
    /// an instance of `old` gets. None while `old` itself is installed: a folder wins.
    pub fn replacement(&self, old: &str) -> Option<(&str, &Map<String, Value>)> {
        if self.widgets.contains_key(old) {
            return None;
        }
        self.widgets.iter().find_map(|(id, installed)| {
            let settings = installed.manifest.replaces.get(old)?;
            Some((id.as_str(), settings))
        })
    }
}

/// Finds an installed plugin by id, reading its manifest from disk.
pub fn find_plugin(id: &str) -> Option<(PluginManifest, PathBuf)> {
    let roots = plugin_roots();
    roots.iter().map(|root| root.join(id)).find_map(|dir| {
        if !dir.join(PLUGIN_MANIFEST).is_file() {
            return None;
        }
        PluginManifest::load(&dir)
            .inspect_err(|e| log::warn!("{e:#}"))
            .ok()
            .map(|manifest| (manifest, dir))
    })
}

/// Built-ins are compiled settings schemas, never executable HTML. Studio uses the same schemas.
pub fn builtins() -> Registry {
    let mut widgets = BTreeMap::new();
    let manifest = WidgetManifest::parse(include_str!("catalog/calendar.json"), "calendar")
        .expect("built-in schema");
    widgets.insert("calendar".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/clock.json"), "clock")
        .expect("built-in schema");
    widgets.insert("clock".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/countdown.json"), "countdown")
        .expect("built-in schema");
    widgets.insert("countdown".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/crypto.json"), "crypto")
        .expect("built-in schema");
    widgets.insert("crypto".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/heatmap.json"), "heatmap")
        .expect("built-in schema");
    widgets.insert("heatmap".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/launcher.json"), "launcher")
        .expect("built-in schema");
    widgets.insert("launcher".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/media-pill.json"), "media-pill")
        .expect("built-in schema");
    widgets.insert("media-pill".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/notes.json"), "notes")
        .expect("built-in schema");
    widgets.insert("notes".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/now-playing.json"), "now-playing")
        .expect("built-in schema");
    widgets.insert("now-playing".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/photo.json"), "photo")
        .expect("built-in schema");
    widgets.insert("photo".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/quick-toggles.json"), "quick-toggles")
            .expect("built-in schema");
    widgets.insert("quick-toggles".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/quote.json"), "quote")
        .expect("built-in schema");
    widgets.insert("quote".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/rss.json"), "rss").expect("built-in schema");
    widgets.insert("rss".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(
        include_str!("catalog/system-monitor.json"),
        "system-monitor",
    )
    .expect("built-in schema");
    widgets.insert("system-monitor".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/timer.json"), "timer")
        .expect("built-in schema");
    widgets.insert("timer".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/todo.json"), "todo").expect("built-in schema");
    widgets.insert("todo".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/weather.json"), "weather")
        .expect("built-in schema");
    widgets.insert("weather".into(), Installed { manifest });
    Registry { widgets }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_native_view_declares_sizes_and_weather_can_show_a_full_forecast() {
        for installed in super::builtins().widgets.values() {
            let manifest = &installed.manifest;
            assert!(!manifest.size.allowed.is_empty(), "{}", manifest.id);
            assert!(manifest.size.allowed.contains(&manifest.size.default));
            for size in &manifest.size.allowed {
                assert_eq!(manifest.fit_size(size.map(f64::from), *size), Some(*size));
            }
        }
        let registry = super::builtins();
        let weather = &registry.get("weather").unwrap().manifest;
        for size in [[2, 2], [4, 2], [4, 4], [4, 5], [5, 5], [5, 6]] {
            assert_eq!(weather.clamp_size(size), size);
        }
        let timer = &registry.get("timer").unwrap().manifest;
        assert_eq!(
            timer.clamp_size([4, 3]),
            [4, 4],
            "an undeclared intermediate size is never persisted"
        );
        assert_eq!(weather.fit_size([5.0, 6.0], [4, 5]), Some([4, 5]));
        assert_eq!(weather.fit_size([2.0, 2.0], [1, 1]), None);
    }
    use super::*;
    #[test]
    fn every_builtin_schema_and_replacement_survives_without_html() {
        let registry = builtins();
        assert_eq!(registry.widgets.len(), 17);
        assert_eq!(registry.replacement("analog-clock").unwrap().0, "clock");
        assert_eq!(
            registry
                .get("notes")
                .unwrap()
                .manifest
                .resolve_settings(&Map::new())["fontScale"]
                .as_f64(),
            Some(1.0)
        );
    }
}
