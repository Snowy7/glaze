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
    let manifest = WidgetManifest::parse(include_str!("catalog/discord.json"), "discord")
        .expect("built-in schema");
    widgets.insert("discord".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/t3.json"), "t3").expect("built-in schema");
    widgets.insert("t3".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/obs.json"), "obs").expect("built-in schema");
    widgets.insert("obs".into(), Installed { manifest });
    let manifest = WidgetManifest::parse(include_str!("catalog/docker.json"), "docker")
        .expect("built-in schema");
    widgets.insert("docker".into(), Installed { manifest });
    let manifest =
        WidgetManifest::parse(include_str!("catalog/grok.json"), "grok").expect("built-in schema");
    widgets.insert("grok".into(), Installed { manifest });
    for (id, definition) in crate::custom::catalog() {
        match definition.manifest(&id) {
            Ok(manifest) => { widgets.insert(id, Installed { manifest }); }
            Err(error) => log::warn!("Custom widget {id}: {error}"),
        }
    }
    for (id, manifest) in component_faces() {
        widgets.entry(id).or_insert(Installed { manifest });
    }
    Registry { widgets }
}

/// Every WebAssembly component plugin's faces as widgets `<plugin>--<face>` (docs/plugins.md), so
/// the "+" picker, sizing and Studio treat them like any widget; the shell draws them as
/// `Widget::Plugin`. User installs shadow bundled ones.
fn component_faces() -> Vec<(String, WidgetManifest)> {
    let mut out: Vec<(String, WidgetManifest)> = Vec::new();
    let dirs = plugin_roots().into_iter().filter_map(|root| std::fs::read_dir(root).ok()).flat_map(|d| d.flatten().map(|e| e.path()));
    for dir in dirs.filter(|d| d.join(PLUGIN_MANIFEST).is_file()) {
        let Ok(plugin) = PluginManifest::load(&dir) else { continue };
        if plugin.component.is_none() {
            continue;
        }
        for face in &plugin.faces {
            let id = format!("{}--{}", plugin.id, face.id);
            if out.iter().any(|(known, _)| *known == id) {
                continue;
            }
            let size = &face.size;
            let manifest = serde_json::json!({
                "id": id,
                "name": format!("{} · {}", plugin.name, face.name),
                "description": format!("From the {} plugin", plugin.name),
                "category": "Plugins",
                "size": { "default": size.default, "min": size.min.unwrap_or(size.default), "max": size.max },
            });
            match serde_json::from_value::<WidgetManifest>(manifest) {
                Ok(manifest) => out.push((id, manifest)),
                Err(e) => log::warn!("plugin {} face {}: {e}", plugin.id, face.id),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_size_from_min_to_max_is_allowed_one_cell_at_a_time() {
        for installed in super::builtins().widgets.values() {
            let manifest = &installed.manifest;
            // Custom widgets (registered here too) may leave max open or skip the category.
            let max = manifest.size.max.unwrap_or([16; 2]);
            let compiled = ["clock", "calendar", "weather", "t3", "notes", "heatmap"];
            if compiled.contains(&manifest.id.as_str()) {
                assert!(manifest.category.is_some(), "{} has a picker category", manifest.id);
            }
            let sizes: Vec<_> = manifest.size.sizes([16; 2]).collect();
            assert!(sizes.contains(&manifest.size.default));
            let span = (max[0] - manifest.size.min[0] + 1) * (max[1] - manifest.size.min[1] + 1);
            assert_eq!(sizes.len() as u32, span, "{}: dense", manifest.id);
            for size in sizes {
                assert_eq!(manifest.fit_size(size.map(f64::from), size), Some(size));
                assert_eq!(manifest.clamp_size(size), size);
            }
        }
        let registry = super::builtins();
        // The integration widgets step one cell at a time too (T3 at 6×8, between 4×8 and 8×8).
        let t3 = &registry.get("t3").unwrap().manifest;
        assert_eq!(t3.clamp_size([6, 8]), [6, 8]);
        assert_eq!(t3.clamp_size([7, 5]), [7, 5]);
        let weather = &registry.get("weather").unwrap().manifest;
        assert_eq!(weather.fit_size([10.0, 12.0], [8, 10]), Some([8, 10]));
        assert_eq!(weather.fit_size([4.0, 4.0], [2, 2]), None);
        assert_eq!(weather.clamp_size([1, 1]), weather.size.min);
    }
    use super::*;
    #[test]
    fn every_builtin_schema_and_replacement_survives_without_html() {
        let registry = builtins();
        assert!(registry.widgets.len() >= 23);
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
