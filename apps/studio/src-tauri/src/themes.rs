//! The gallery: every theme in the theme roots, which one is active, and
//! creating new user themes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, LoadedTheme, MANIFEST, Wallpaper};
use serde::{Deserialize, Serialize};

use crate::source::{self, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// In `%APPDATA%\mypaper\themes`.
    User,
    /// In the `themes/` folder that ships beside the app.
    Bundled,
    /// The active theme lives somewhere else (an absolute path in the config).
    External,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeEntry {
    /// Stable key: the theme folder's canonical path.
    pub id: String,
    pub name: String,
    pub dir: String,
    pub origin: Origin,
    /// `None` when `theme.json` doesn't load; see `error`.
    pub kind: Option<Kind>,
    pub error: Option<String>,
    /// An image file to show on the card.
    pub preview: Option<String>,
    pub active: bool,
    /// What to write to `config.theme` to select this theme.
    pub apply_as: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeList {
    pub themes: Vec<ThemeEntry>,
    /// Where we looked, for the empty state.
    pub roots: Vec<String>,
}

/// Resolves a `config.theme` value to a theme folder, or `None` if it doesn't resolve.
fn resolve(theme: &str) -> Option<PathBuf> {
    mypaper_theme::locate(Some(theme)).ok()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Lists user themes first, then bundled ones, each sorted by folder name.
/// `active` is the current `config.theme`, if the config could be read.
pub fn list(active: Option<&str>) -> ThemeList {
    let roots = mypaper_theme::theme_roots();
    let user = mypaper_config::user_themes_dir().ok();
    let active_dir = active.and_then(resolve).map(|dir| canonical(&dir));

    let mut themes = Vec::new();
    for root in &roots {
        let origin = if user.as_deref() == Some(root.as_path()) { Origin::User } else { Origin::Bundled };
        themes.extend(theme_dirs(root).into_iter().map(|dir| entry(&dir, origin, active_dir.as_deref())));
    }
    if let Some(dir) = &active_dir
        && !themes.iter().any(|t| canonical(Path::new(&t.dir)) == *dir)
    {
        themes.insert(0, entry(dir, Origin::External, active_dir.as_deref()));
    }
    ThemeList {
        themes,
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
    }
}

/// Subfolders of `root` that have a `theme.json`, sorted case-insensitively.
fn theme_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    dirs
}

pub fn entry(dir: &Path, origin: Origin, active_dir: Option<&Path>) -> ThemeEntry {
    let canonical_dir = canonical(dir);
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let loaded = LoadedTheme::load(dir);
    let (name, kind, error) = match &loaded {
        Ok(t) => (t.theme.name.clone(), Some(Kind::of(&t.theme.wallpaper)), None),
        Err(e) => (folder.clone(), None, Some(format!("{e:#}"))),
    };
    // A bare folder name is only safe if it resolves back to this folder
    // (a user theme can shadow a bundled one of the same name).
    let by_name = origin != Origin::External && resolve(&folder).is_some_and(|d| canonical(&d) == canonical_dir);
    ThemeEntry {
        id: canonical_dir.display().to_string(),
        name,
        dir: dir.display().to_string(),
        origin,
        kind,
        error,
        preview: preview(dir, loaded.as_ref().ok()).map(|p| p.display().to_string()),
        active: active_dir == Some(canonical_dir.as_path()),
        apply_as: if by_name { folder } else { dir.display().to_string() },
    }
}

/// `preview.png` / `preview.jpg` in the theme folder, else the image itself
/// (image themes) or the first picture (slideshows). Shaders and videos have none.
fn preview(dir: &Path, loaded: Option<&LoadedTheme>) -> Option<PathBuf> {
    ["preview.png", "preview.jpg", "preview.jpeg", "preview.webp"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
        .or_else(|| match &loaded?.theme.wallpaper {
            Wallpaper::Image(i) => Some(loaded?.asset(&i.source)).filter(|p| p.is_file() && source::is_image(p)),
            Wallpaper::Slideshow(s) => source::first_image_in(&loaded?.asset(&s.folder)),
            _ => None,
        })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTheme {
    pub source: PathBuf,
    pub name: String,
    /// Ignored for kinds where it means nothing (shader, scene).
    pub fit: Option<Fit>,
}

/// Creates `<root>\<slug>\theme.json` pointing at the absolute source, and
/// checks it loads. Never overwrites: a taken slug gets `-2`, `-3`, ... Returns the folder.
pub fn create_in(root: &Path, new: &NewTheme) -> Result<PathBuf> {
    let name = new.name.trim();
    if name.is_empty() {
        bail!("give the theme a name");
    }
    let source = std::path::absolute(&new.source).context("resolving the source path")?;
    let kind = source::check(&source)?;
    let text = source::manifest(name, kind, &source, new.fit.unwrap_or_default());

    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let base = source::slug(name);
    let dir = (1u32..)
        .map(|n| root.join(if n == 1 { base.clone() } else { format!("{base}-{n}") }))
        .find(|dir| !dir.exists())
        .context("no free theme folder name")?;
    std::fs::create_dir(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let written = std::fs::write(dir.join(MANIFEST), text)
        .context("writing theme.json")
        .and_then(|()| LoadedTheme::load(&dir).map(drop));
    if let Err(e) = written {
        // We created this folder a moment ago, so it only holds our own files.
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    fn image_theme(source: &Path, name: &str) -> NewTheme {
        NewTheme { source: source.into(), name: name.into(), fit: Some(Fit::Contain) }
    }

    #[test]
    fn creates_a_loadable_theme_and_never_overwrites() {
        let work = temp_dir("create");
        let photo = work.join("photo.jpg");
        std::fs::write(&photo, "").unwrap();
        let root = work.join("themes");

        let first = create_in(&root, &image_theme(&photo, "My Photo")).unwrap();
        let second = create_in(&root, &image_theme(&photo, "My Photo")).unwrap();
        assert_eq!(first.file_name().unwrap(), "my-photo");
        assert_eq!(second.file_name().unwrap(), "my-photo-2");

        let loaded = LoadedTheme::load(&first).unwrap();
        assert_eq!(loaded.theme.name, "My Photo");
        let Wallpaper::Image(image) = loaded.theme.wallpaper else { panic!("not an image theme") };
        assert_eq!(image.source, photo);
        assert_eq!(image.fit, Fit::Contain);
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn bad_input_leaves_nothing_behind() {
        let work = temp_dir("create-bad");
        let root = work.join("themes");
        let photo = work.join("photo.jpg");
        std::fs::write(&photo, "").unwrap();

        assert!(create_in(&root, &image_theme(&photo, "   ")).is_err());
        assert!(create_in(&root, &image_theme(&work.join("missing.png"), "x")).is_err());
        let script = work.join("run.exe");
        std::fs::write(&script, "").unwrap();
        assert!(create_in(&root, &image_theme(&script, "x")).is_err());
        assert_eq!(theme_dirs(&root).len(), 0);
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn previews_prefer_preview_files_then_the_image_itself() {
        let work = temp_dir("preview");
        let photo = work.join("photo.jpg");
        std::fs::write(&photo, "").unwrap();
        let dir = create_in(&work.join("themes"), &image_theme(&photo, "P")).unwrap();

        let loaded = LoadedTheme::load(&dir).unwrap();
        assert_eq!(preview(&dir, Some(&loaded)), Some(photo));
        std::fs::write(dir.join("preview.png"), "").unwrap();
        assert_eq!(preview(&dir, Some(&loaded)), Some(dir.join("preview.png")));
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn entries_report_broken_themes_instead_of_hiding_them() {
        let work = temp_dir("broken");
        let dir = work.join("oops");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), r#"{ "name": "Oops" }"#).unwrap();
        let entry = entry(&dir, Origin::User, None);
        assert_eq!(entry.name, "oops");
        assert!(entry.kind.is_none() && entry.error.is_some());
        std::fs::remove_dir_all(&work).unwrap();
    }
}
