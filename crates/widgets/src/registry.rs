//! Installed widgets and plugins: the user's folders first, then the bundled
//! ones; the first folder with a given id wins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::{PLUGIN_MANIFEST, PluginManifest, WIDGET_MANIFEST, WidgetManifest};

pub struct Installed {
    pub manifest: WidgetManifest,
    pub dir: PathBuf,
}

/// Every installed widget, by id.
#[derive(Default)]
pub struct Registry {
    pub widgets: BTreeMap<String, Installed>,
}

/// Where widgets live: `%APPDATA%\mypaper\widgets`, then the bundled `widgets/`.
pub fn widget_roots() -> Vec<PathBuf> {
    roots(
        crate::files::user("widgets").ok(),
        crate::files::bundled_widgets(),
    )
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

/// The subfolders of `roots` that hold `manifest`, first root first.
fn folders<'a>(roots: &'a [PathBuf], manifest: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    roots
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flat_map(|entries| {
            let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            dirs.sort();
            dirs
        })
        .filter(move |dir| dir.join(manifest).is_file())
}

impl Registry {
    /// Scans `roots`; broken manifests are logged and skipped.
    pub fn scan(roots: &[PathBuf]) -> Self {
        let mut widgets = BTreeMap::new();
        for dir in folders(roots, WIDGET_MANIFEST) {
            match WidgetManifest::load(&dir) {
                Ok(manifest) => {
                    widgets
                        .entry(manifest.id.clone())
                        .or_insert(Installed { manifest, dir });
                }
                Err(e) => log::warn!("skipping widget: {e:#}"),
            }
        }
        Self { widgets }
    }

    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.widgets.get(id)
    }

    /// The manifest as it is on disk right now: what permission checks use.
    pub fn manifest_on_disk(&self, id: &str) -> Option<WidgetManifest> {
        let installed = self.get(id)?;
        WidgetManifest::load(&installed.dir)
            .inspect_err(|e| log::warn!("{e:#}"))
            .ok()
    }

    /// Which widget a changed path belongs to, if it's inside one of `roots`.
    pub fn widget_of(path: &Path, roots: &[PathBuf]) -> Option<String> {
        roots.iter().find_map(|root| {
            let first = path.strip_prefix(root).ok()?.components().next()?;
            Some(first.as_os_str().to_str()?.to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(root: &Path, id: &str, name: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let json =
            format!(r#"{{ "id": "{id}", "name": "{name}", "size": {{ "default": [1, 1] }} }}"#);
        std::fs::write(dir.join(WIDGET_MANIFEST), json).unwrap();
    }

    #[test]
    fn user_widgets_win_and_broken_ones_are_skipped() {
        let root = crate::files::temp_dir("registry");
        let (user, bundled) = (root.join("user"), root.join("bundled"));
        widget(&bundled, "clock", "Bundled clock");
        widget(&bundled, "notes", "Notes");
        widget(&user, "clock", "My clock");
        std::fs::create_dir_all(bundled.join("shell")).unwrap(); // no manifest: not a widget
        std::fs::create_dir_all(user.join("broken")).unwrap();
        std::fs::write(user.join("broken").join(WIDGET_MANIFEST), "{").unwrap();

        let registry = Registry::scan(&[user.clone(), bundled.clone()]);
        let ids: Vec<&str> = registry.widgets.keys().map(String::as_str).collect();
        assert_eq!(ids, ["clock", "notes"]);
        assert_eq!(registry.get("clock").unwrap().manifest.name, "My clock");

        let changed = bundled.join("notes").join("index.html");
        assert_eq!(
            Registry::widget_of(&changed, &[user, bundled]),
            Some("notes".into())
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
