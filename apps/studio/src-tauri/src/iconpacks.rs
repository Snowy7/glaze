//! Icon packs (docs/bars.md, "Icon packs"): the packs Studio can offer (the user's in
//! `%APPDATA%\mypaper\icons\` and the one the bars page bundles), and the per-app overrides, which
//! are `path` entries of a user pack. `bars.json` → `icons` only names the pack in use; the pack's
//! own `icons.json` is checked by the host's `IconPack` type before anything is written.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_bars_schema::icon_pack::{IconPack, MANIFEST};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::fsio;

const IMAGE_TYPES: &[&str] = &["svg", "png", "ico", "webp", "jpg", "jpeg"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    /// The folder name: what `bars.json` → `icons` takes.
    pub name: String,
    pub title: String,
    /// Ships with the bars page (read-only), rather than living in the user's folder.
    pub bundled: bool,
    pub mono: bool,
    pub entries: usize,
    /// How many entries are for apps (a `path` or an `umid`).
    pub apps: usize,
    /// A few of the pack's icon files, to show what it looks like.
    pub previews: Vec<String>,
    /// The pack's `icons.json` doesn't load.
    pub error: Option<String>,
}

/// `%APPDATA%\mypaper\icons`.
pub fn user_root() -> Result<PathBuf> {
    Ok(mypaper_config::dir()?.join("icons"))
}

/// The packs that ship with the bars page, beside the exe or any parent (built, or its sources).
pub fn bundled_root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .ancestors()
        .skip(1)
        .find_map(|dir| {
            ["bars/shell/dist/icons", "bars/shell/public/icons"]
                .into_iter()
                .map(|rel| dir.join(rel))
                .find(|path| path.is_dir())
        })
}

fn pack_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST).is_file())
        .collect();
    dirs.sort_by_key(|dir| {
        dir.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    dirs
}

fn info(dir: &Path, bundled: bool) -> PackInfo {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match IconPack::load(dir) {
        Ok(pack) => PackInfo {
            title: pack.name.clone(),
            mono: pack.mono,
            entries: pack.entries.len(),
            apps: pack
                .entries
                .iter()
                .filter(|e| e.umid.is_some() || e.path.is_some())
                .count(),
            previews: pack
                .entries
                .iter()
                .map(|e| dir.join(&e.icon))
                .filter(|file| file.is_file())
                .take(4)
                .map(|file| file.display().to_string())
                .collect(),
            error: None,
            name,
            bundled,
        },
        Err(e) => PackInfo {
            title: name.clone(),
            mono: false,
            entries: 0,
            apps: 0,
            previews: Vec::new(),
            error: Some(format!("{e:#}")),
            name,
            bundled,
        },
    }
}

/// The bundled packs, then the user's; a user pack hides a bundled one of the same name, as the
/// host resolves it.
pub fn list_from(bundled: Option<&Path>, user: Option<&Path>) -> Vec<PackInfo> {
    let mut packs: Vec<PackInfo> = Vec::new();
    for (root, is_bundled) in [(user, false), (bundled, true)] {
        for dir in root.map(pack_dirs).unwrap_or_default() {
            let pack = info(&dir, is_bundled);
            if !packs
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(&pack.name))
            {
                packs.push(pack);
            }
        }
    }
    packs.sort_by_key(|p| (!p.bundled, p.name.to_lowercase()));
    packs
}

/// The folder of a user pack, refusing names the host would refuse for `bars.json` → `icons`.
pub fn user_pack_dir(root: &Path, name: &str) -> Result<PathBuf> {
    let bad = !(1..=64).contains(&name.len())
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || " _.-".contains(c)));
    if bad {
        bail!("a pack name is 1-64 letters, digits, spaces, '.', '_' or '-'");
    }
    Ok(root.join(name))
}

/// Makes an empty pack named `name` (its title is the same), unless one is there.
pub fn create(root: &Path, name: &str) -> Result<PathBuf> {
    let dir = user_pack_dir(root, name)?;
    if dir.join(MANIFEST).exists() {
        return Ok(dir);
    }
    let manifest = json!({ "name": name, "mono": false, "entries": [] });
    fsio::write_atomic(
        &dir.join(MANIFEST),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    Ok(dir)
}

/// One app's icon in a pack.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppIcon {
    /// Position among the pack's entries (what `remove_app_icon` takes).
    pub index: usize,
    /// `path` or `umid`.
    pub kind: &'static str,
    pub target: String,
    /// The image's absolute path.
    pub file: String,
}

pub fn apps(dir: &Path) -> Result<Vec<AppIcon>> {
    let pack = IconPack::load(dir)?;
    Ok(pack
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let (kind, target) = match (&entry.path, &entry.umid) {
                (Some(path), _) => ("path", path.clone()),
                (_, Some(umid)) => ("umid", umid.clone()),
                _ => return None,
            };
            Some(AppIcon {
                index,
                kind,
                target,
                file: dir.join(&entry.icon).display().to_string(),
            })
        })
        .collect())
}

fn read_manifest(dir: &Path) -> Result<Map<String, Value>> {
    let path = dir.join(MANIFEST);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    match serde_json::from_str(&text).with_context(|| format!("in {}", path.display()))? {
        Value::Object(map) => Ok(map),
        _ => bail!("{} must hold a JSON object", path.display()),
    }
}

/// Checks the edited manifest with the host's type, then writes it.
fn write_manifest(dir: &Path, manifest: &Map<String, Value>) -> Result<()> {
    let text = serde_json::to_string_pretty(manifest)? + "\n";
    IconPack::parse(&text).context("the icon pack would be refused")?;
    fsio::write_atomic(&dir.join(MANIFEST), text)
}

fn entries_mut(manifest: &mut Map<String, Value>) -> Result<&mut Vec<Value>> {
    match manifest.get_mut("entries") {
        Some(Value::Array(entries)) => Ok(entries),
        _ => bail!("the pack has no entries list"),
    }
}

/// A file name for `image` inside the pack that no other entry's icon uses.
fn free_name(dir: &Path, image: &Path, taken: &[String]) -> Result<String> {
    let extension = image
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .filter(|e| IMAGE_TYPES.contains(&e.as_str()));
    let Some(extension) = extension else {
        bail!("pick an image ({})", IMAGE_TYPES.join(", "))
    };
    let stem: String = image
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = if stem.is_empty() {
        "icon".to_owned()
    } else {
        stem
    };
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{stem}.{extension}")
            } else {
                format!("{stem}-{n}.{extension}")
            }
        })
        .find(|name| !taken.contains(name) && !dir.join(name).exists())
        .context("no free file name")
}

/// Gives the app at `target` (a full path) the picture in `image`: the file is copied into the
/// pack and its `path` entry set, replacing the app's earlier one.
pub fn set_app_icon(dir: &Path, target: &str, image: &Path) -> Result<()> {
    if target.trim().is_empty() {
        bail!("an app needs a path");
    }
    let mut manifest = read_manifest(dir)?;
    let taken: Vec<String> = entries_mut(&mut manifest)?
        .iter()
        .filter_map(|e| e["icon"].as_str().map(str::to_owned))
        .collect();
    let name = free_name(dir, image, &taken)?;
    let copied = dir.join(&name);
    std::fs::copy(image, &copied)
        .with_context(|| format!("copying {} into the pack", image.display()))?;

    let entries = entries_mut(&mut manifest)?;
    let entry = json!({ "path": target, "icon": name });
    let replaced = entries.iter_mut().find(|e| {
        e["path"]
            .as_str()
            .is_some_and(|p| p.eq_ignore_ascii_case(target))
    });
    let old = match replaced {
        Some(slot) => Some(std::mem::replace(slot, entry)),
        None => {
            entries.push(entry);
            None
        }
    };
    if let Err(e) = write_manifest(dir, &manifest) {
        let _ = std::fs::remove_file(&copied);
        return Err(e);
    }
    if let Some(old) = old {
        drop_unused_image(dir, &manifest, old["icon"].as_str());
    }
    Ok(())
}

/// Removes the app entry at `index` (as `apps` numbers them) and its picture when nothing else uses it.
pub fn remove_app_icon(dir: &Path, index: usize) -> Result<()> {
    let mut manifest = read_manifest(dir)?;
    let entries = entries_mut(&mut manifest)?;
    if index >= entries.len()
        || !(entries[index].get("path").is_some() || entries[index].get("umid").is_some())
    {
        bail!("there is no app icon at {index}");
    }
    let removed = entries.remove(index);
    write_manifest(dir, &manifest)?;
    drop_unused_image(dir, &manifest, removed["icon"].as_str());
    Ok(())
}

/// Deletes `icon` from the pack's folder when no entry (nor `missing`) still names it.
fn drop_unused_image(dir: &Path, manifest: &Map<String, Value>, icon: Option<&str>) {
    let Some(icon) = icon else { return };
    let used = manifest.get("missing").and_then(Value::as_str) == Some(icon)
        || manifest
            .get("entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| entries.iter().any(|e| e["icon"].as_str() == Some(icon)));
    if !used {
        let _ = std::fs::remove_file(dir.join(icon));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    fn picture(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        path
    }

    #[test]
    fn packs_are_listed_with_what_they_hold_and_the_users_win() {
        let work = temp_dir("packs-list");
        let (bundled, user) = (work.join("bundled"), work.join("user"));
        for (root, name, mono) in [
            (&bundled, "mono", true),
            (&bundled, "plain", false),
            (&user, "MONO", false),
        ] {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("a.svg"), "<svg/>").unwrap();
            std::fs::write(
                dir.join(MANIFEST),
                format!(r#"{{ "name": "{name} pack", "mono": {mono}, "entries": [ {{ "key": "start", "icon": "a.svg" }}, {{ "path": "code.exe", "icon": "a.svg" }} ] }}"#),
            )
            .unwrap();
        }
        std::fs::create_dir_all(user.join("broken")).unwrap();
        std::fs::write(
            user.join("broken").join(MANIFEST),
            r#"{ "name": "B", "entries": [ { "icon": "x.svg" } ] }"#,
        )
        .unwrap();

        let packs = list_from(Some(&bundled), Some(&user));
        let names: Vec<(&str, bool)> = packs.iter().map(|p| (p.name.as_str(), p.bundled)).collect();
        // Bundled first; the user's MONO hides the bundled mono.
        assert_eq!(names, [("plain", true), ("broken", false), ("MONO", false)]);
        let plain = &packs[0];
        assert_eq!((plain.entries, plain.apps, plain.mono), (2, 1, false));
        assert_eq!(plain.previews.len(), 2);
        assert!(
            packs[1]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("exactly one")),
            "{:?}",
            packs[1].error
        );
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn app_icons_are_added_replaced_and_removed_with_their_pictures() {
        let work = temp_dir("packs-apps");
        let root = work.join("icons");
        assert!(
            user_pack_dir(&root, "../escape").is_err() && user_pack_dir(&root, ".hidden").is_err()
        );
        let dir = create(&root, "My icons").unwrap();
        assert!(IconPack::load(&dir).unwrap().entries.is_empty());
        assert_eq!(
            create(&root, "My icons").unwrap(),
            dir,
            "creating twice keeps the first"
        );

        let images = work.join("images");
        std::fs::create_dir_all(&images).unwrap();
        set_app_icon(&dir, r"C:\Tools\code.exe", &picture(&images, "code.svg")).unwrap();
        set_app_icon(&dir, r"C:\Tools\term.exe", &picture(&images, "code.svg")).unwrap();
        let listed = apps(&dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            (listed[0].kind, listed[0].target.as_str()),
            ("path", r"C:\Tools\code.exe")
        );
        assert_ne!(
            listed[0].file, listed[1].file,
            "two apps never share a copy that one removal could delete"
        );
        assert!(Path::new(&listed[0].file).is_file() && Path::new(&listed[1].file).is_file());

        // Picking again for the same app (any case) replaces its entry and drops the old copy.
        set_app_icon(&dir, r"c:\tools\CODE.exe", &picture(&images, "better.png")).unwrap();
        let listed = apps(&dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].file.ends_with("better.png"));
        assert!(!dir.join("code.svg").exists() && dir.join("code-2.svg").exists());

        remove_app_icon(&dir, 1).unwrap();
        assert_eq!(apps(&dir).unwrap().len(), 1);
        assert!(!dir.join("code-2.svg").exists());
        assert!(remove_app_icon(&dir, 9).is_err());

        // Not an image, not an app: refused, and nothing is left half-written.
        assert!(set_app_icon(&dir, r"C:\a.exe", &picture(&images, "notes.txt")).is_err());
        assert!(set_app_icon(&dir, " ", &picture(&images, "x.svg")).is_err());
        assert_eq!(IconPack::load(&dir).unwrap().entries.len(), 1);
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            2,
            "icons.json and better.png only"
        );
        std::fs::remove_dir_all(&work).unwrap();
    }
}
