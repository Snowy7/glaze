//! What a picked file or folder becomes as a theme: its kind, a folder-safe
//! name, and the `theme.json` text. Pure logic, no Tauri.

use std::path::Path;

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, Wallpaper};
use serde::Serialize;
use serde_json::json;

/// The wallpaper kinds, as the frontend sees them (lowercase, like `theme.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Shader,
    Image,
    Slideshow,
    Video,
    Scene,
    Web,
    App,
}

impl Kind {
    pub fn of(wallpaper: &Wallpaper) -> Self {
        match wallpaper {
            Wallpaper::Shader(_) => Self::Shader,
            Wallpaper::Image(_) => Self::Image,
            Wallpaper::Slideshow(_) => Self::Slideshow,
            Wallpaper::Video(_) => Self::Video,
            Wallpaper::Scene(_) => Self::Scene,
            Wallpaper::Web(_) => Self::Web,
            Wallpaper::App(_) => Self::App,
        }
    }

    /// Whether `fit` means anything for this kind (shaders and scenes draw the whole screen).
    pub fn uses_fit(self) -> bool {
        matches!(self, Self::Image | Self::Slideshow | Self::Video)
    }
}

/// Formats WIC can decode, which is what the engine uses for stills.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "heic", "heif",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

pub fn is_image(path: &Path) -> bool {
    extension(path).is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

fn kind_for_extension(ext: &str) -> Option<Kind> {
    match ext {
        e if IMAGE_EXTENSIONS.contains(&e) => Some(Kind::Image),
        e if VIDEO_EXTENSIONS.contains(&e) => Some(Kind::Video),
        "wgsl" => Some(Kind::Shader),
        "glb" | "gltf" => Some(Kind::Scene),
        _ => None,
    }
}

/// A folder is a slideshow; a file is classified by its extension.
pub fn detect(path: &Path) -> Result<Kind> {
    if path.is_dir() {
        return Ok(Kind::Slideshow);
    }
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }
    let ext = extension(path).with_context(|| format!("{} has no file extension", path.display()))?;
    kind_for_extension(&ext).with_context(|| format!("unsupported file type .{ext}"))
}

/// `detect`, plus the checks that need a look inside a folder.
pub fn check(path: &Path) -> Result<Kind> {
    let kind = detect(path)?;
    if kind == Kind::Slideshow && first_image_in(path).is_none() {
        bail!("{} has no images in it", path.display());
    }
    Ok(kind)
}

/// The first image file in a folder (not recursive), or `None`.
pub fn first_image_in(dir: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_file() && is_image(path))
}

/// A readable default theme name: the file or folder name without its extension.
pub fn suggested_name(path: &Path) -> String {
    let stem = if path.is_dir() { path.file_name() } else { path.file_stem() };
    let raw = stem.map(|s| s.to_string_lossy().replace(['_', '-'], " "));
    let name = raw.as_deref().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() { "My theme".into() } else { name }
}

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A lowercase, folder-safe name: letters and digits joined by single dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out = &out[..out.len().min(48)];
    let out = out.trim_end_matches('-');
    match out {
        "" => "theme".into(),
        reserved if RESERVED.contains(&reserved) => format!("theme-{reserved}"),
        ok => ok.into(),
    }
}

fn fit_name(fit: Fit) -> &'static str {
    match fit {
        Fit::Cover => "cover",
        Fit::Contain => "contain",
        Fit::Stretch => "stretch",
        Fit::Center => "center",
    }
}

/// The `theme.json` text for a theme whose asset lives at the absolute `source`.
pub fn manifest(name: &str, kind: Kind, source: &Path, fit: Fit) -> String {
    let source = source.to_string_lossy();
    let fit = fit_name(fit);
    let wallpaper = match kind {
        Kind::Shader => json!({ "kind": "shader", "source": source }),
        Kind::Scene => json!({ "kind": "scene", "source": source }),
        Kind::Image => json!({ "kind": "image", "source": source, "fit": fit }),
        Kind::Video => json!({ "kind": "video", "source": source, "fit": fit }),
        Kind::Slideshow => json!({ "kind": "slideshow", "folder": source, "fit": fit }),
        Kind::Web => json!({ "kind": "web", "url": source }),
        Kind::App => json!({ "kind": "app", "command": source }),
    };
    let manifest = json!({ "name": name, "wallpaper": wallpaper });
    // Serializing a `Value` cannot fail.
    serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n"
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use mypaper_theme::LoadedTheme;

    use super::*;

    #[test]
    fn extensions_map_to_kinds() {
        let kind = |p: &str| detect_by_extension_only(Path::new(p));
        assert_eq!(kind("a.PNG"), Some(Kind::Image));
        assert_eq!(kind("a.jpeg"), Some(Kind::Image));
        assert_eq!(kind("a.mkv"), Some(Kind::Video));
        assert_eq!(kind("a.WebM"), Some(Kind::Video));
        assert_eq!(kind("a.wgsl"), Some(Kind::Shader));
        assert_eq!(kind("a.glb"), Some(Kind::Scene));
        assert_eq!(kind("a.gltf"), Some(Kind::Scene));
        assert_eq!(kind("a.exe"), None);
        assert_eq!(kind("noext"), None);
    }

    fn detect_by_extension_only(path: &Path) -> Option<Kind> {
        kind_for_extension(&extension(path)?)
    }

    #[test]
    fn folders_are_slideshows_and_empty_ones_fail_the_check() {
        let dir = temp_dir("detect");
        assert_eq!(detect(&dir).unwrap(), Kind::Slideshow);
        assert!(check(&dir).is_err());
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        assert!(check(&dir).is_err());
        std::fs::write(dir.join("a.jpg"), "").unwrap();
        assert_eq!(check(&dir).unwrap(), Kind::Slideshow);
        assert!(detect(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn slugs_are_folder_safe() {
        assert_eq!(slug("Neon City 2077!"), "neon-city-2077");
        assert_eq!(slug("  --Hello__World--  "), "hello-world");
        assert_eq!(slug("日本語"), "theme");
        assert_eq!(slug("CON"), "theme-con");
        assert_eq!(slug(&"a".repeat(80)).len(), 48);
        assert!(!slug(&format!("{}-b", "a".repeat(47))).ends_with('-'));
    }

    #[test]
    fn names_come_from_the_path() {
        assert_eq!(suggested_name(Path::new("C:/pics/my_cool-photo.jpg")), "my cool photo");
        assert_eq!(suggested_name(Path::new("")), "My theme");
    }

    #[test]
    fn manifests_load_back_for_every_kind() {
        let source = PathBuf::from(r"C:\Media\thing");
        for (kind, uses_fit) in [
            (Kind::Shader, false),
            (Kind::Image, true),
            (Kind::Slideshow, true),
            (Kind::Video, true),
            (Kind::Scene, false),
        ] {
            let text = manifest("T \"quoted\"", kind, &source, Fit::Contain);
            let loaded = LoadedTheme::parse(PathBuf::from("themes/t"), &text)
                .unwrap_or_else(|e| panic!("{kind:?}: {e:#}\n{text}"));
            assert_eq!(loaded.theme.name, "T \"quoted\"");
            assert_eq!(Kind::of(&loaded.theme.wallpaper), kind);
            assert_eq!(text.contains("\"fit\""), uses_fit, "{kind:?}");
            assert_eq!(kind.uses_fit(), uses_fit);
        }
    }

    #[test]
    fn fit_is_written_for_image_themes() {
        let text = manifest("P", Kind::Image, Path::new(r"C:\a.png"), Fit::Stretch);
        let loaded = LoadedTheme::parse(PathBuf::from("themes/p"), &text).unwrap();
        let Wallpaper::Image(image) = loaded.theme.wallpaper else {
            panic!("expected an image theme");
        };
        assert_eq!(image.fit, Fit::Stretch);
        assert_eq!(image.source, PathBuf::from(r"C:\a.png"));
    }

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mypaper-studio-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
