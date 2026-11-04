//! User icon-pack manifests shared by the native shell and Studio (docs/bars.md).
//! Mapping and CPU decoding live in `crates/shell/src/bars/icons.rs`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "icons.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconPack {
    pub name: String,
    /// Monochrome icons: drawn in the text color (as a mask), not as pictures.
    #[serde(default)]
    pub mono: bool,
    /// For apps no entry matches; missing: the app's own icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
    pub entries: Vec<Entry>,
}

/// One mapping. Exactly one of `umid`, `path`, `extension` and `key` is set.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// An AppUserModelID, compared case-insensitively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umid: Option<String>,
    /// A full path, or a file name (`code.exe`) that matches any folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A file extension, without the dot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// One of the bars' glyph keys (docs/bars.md lists them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// A file in the pack's folder.
    pub icon: String,
    /// Overrides the pack's `mono` for this icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono: Option<bool>,
}

const IMAGE_TYPES: &[&str] = &["svg", "png", "ico", "webp", "jpg", "jpeg"];

/// A file name inside the pack: relative, no `..`, an image type the page can show.
fn check_icon(icon: &str) -> Result<()> {
    let path = Path::new(icon);
    let inside = path.components().all(|c| matches!(c, Component::Normal(_)));
    let typed = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_TYPES.contains(&e.to_ascii_lowercase().as_str()));
    if !inside || !typed {
        bail!(
            "icon {icon:?} must be a file in the pack's folder ({})",
            IMAGE_TYPES.join(", ")
        );
    }
    Ok(())
}

impl IconPack {
    pub fn parse(json: &str) -> Result<Self> {
        let pack: Self = serde_json::from_str(json)?;
        if pack.entries.len() > 4000 {
            bail!("at most 4000 entries");
        }
        if let Some(missing) = &pack.missing {
            check_icon(missing)?;
        }
        for (i, entry) in pack.entries.iter().enumerate() {
            let set = [&entry.umid, &entry.path, &entry.extension, &entry.key]
                .into_iter()
                .filter(|m| m.as_ref().is_some_and(|m| !m.trim().is_empty()))
                .count();
            if set != 1 {
                bail!("entries[{i}]: set exactly one of umid, path, extension and key");
            }
            check_icon(&entry.icon).with_context(|| format!("entries[{i}]"))?;
        }
        Ok(pack)
    }

    /// Loads `<dir>/icons.json`.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("reading {}", manifest.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", manifest.display()))
    }
}

/// The folder of pack `name`: the first of `roots` that has it.
pub fn find(name: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join(name))
        .find(|dir| dir.join(MANIFEST).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_kind_of_entry_and_checks_files() {
        let pack = IconPack::parse(
            r#"{ "name": "Mono", "mono": true, "missing": "app.svg", "entries": [
                { "umid": "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "icon": "terminal.svg" },
                { "path": "code.exe", "icon": "code.png", "mono": false },
                { "extension": "txt", "icon": "sub/text.svg" },
                { "key": "start", "icon": "start.svg" } ] }"#,
        )
        .unwrap();
        assert_eq!(pack.entries.len(), 4);
        assert!(pack.mono);
        for bad in [
            r#"{ "name": "A", "entries": [ { "icon": "a.svg" } ] }"#,
            r#"{ "name": "A", "entries": [ { "key": "a", "path": "b", "icon": "a.svg" } ] }"#,
            r#"{ "name": "A", "entries": [ { "key": "a", "icon": "../a.svg" } ] }"#,
            r#"{ "name": "A", "entries": [ { "key": "a", "icon": "C:\\a.svg" } ] }"#,
            r#"{ "name": "A", "entries": [ { "key": "a", "icon": "a.exe" } ] }"#,
            r#"{ "name": "A", "entries": [], "size": 3 }"#,
        ] {
            assert!(IconPack::parse(bad).is_err(), "accepted {bad}");
        }
    }
}
