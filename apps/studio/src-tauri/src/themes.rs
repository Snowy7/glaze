//! The gallery: every theme in the theme roots, which one is active, and
//! creating new user themes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, LoadedTheme, MANIFEST, Wallpaper};
use serde::{Deserialize, Serialize};

use crate::fsio;
use crate::manifest::{self, SkinEdit, SkinKind, WindowsStyle};
use crate::source::{self, Kind};
use crate::thumbs;

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
    /// A video theme whose thumbnail hasn't been made yet; ask for it with `generate_preview`.
    pub preview_pending: bool,
    /// The theme has a `windows` styling section.
    pub styled: bool,
    /// The theme sets how widgets / the bars look.
    pub widgets_skin: bool,
    pub bars_skin: bool,
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
    let loaded_ok = loaded.as_ref().ok();
    let preview = preview(dir, loaded_ok);
    ThemeEntry {
        id: canonical_dir.display().to_string(),
        name,
        dir: dir.display().to_string(),
        origin,
        kind,
        error,
        preview_pending: preview.is_none() && video_source(loaded_ok).is_some_and(|p| p.is_file()),
        preview: preview.map(|p| p.display().to_string()),
        styled: loaded_ok.is_some_and(|t| t.theme.windows.is_some()),
        widgets_skin: loaded_ok.is_some_and(|t| t.theme.widgets.is_some()),
        bars_skin: loaded_ok.is_some_and(|t| t.theme.bars.is_some()),
        active: active_dir == Some(canonical_dir.as_path()),
        apply_as: if by_name { folder } else { dir.display().to_string() },
    }
}

/// The video file of a video theme.
fn video_source(loaded: Option<&LoadedTheme>) -> Option<PathBuf> {
    let loaded = loaded?;
    match &loaded.theme.wallpaper {
        Wallpaper::Video(v) => Some(loaded.asset(&v.source)),
        _ => None,
    }
}

/// `preview.png` / `preview.jpg` in the theme folder, else the image itself
/// (image themes), the first picture (slideshows) or a cached shell thumbnail
/// (videos). Other kinds have none and get a placeholder.
fn preview(dir: &Path, loaded: Option<&LoadedTheme>) -> Option<PathBuf> {
    ["preview.png", "preview.jpg", "preview.jpeg", "preview.webp"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
        .or_else(|| match &loaded?.theme.wallpaper {
            Wallpaper::Image(i) => Some(loaded?.asset(&i.source)).filter(|p| p.is_file() && source::is_image(p)),
            Wallpaper::Slideshow(s) => source::first_image_in(&loaded?.asset(&s.folder)),
            Wallpaper::Video(_) => thumbs::cached(&video_source(loaded)?),
            _ => None,
        })
}

/// Makes the cached thumbnail for a video theme's file. Shell calls can take
/// a moment, so this is kept out of `list`.
pub fn generate_preview(dir: &Path) -> Result<()> {
    let loaded = LoadedTheme::load(dir)?;
    let video = video_source(Some(&loaded)).context("only video themes get generated previews")?;
    thumbs::generate(&video)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTheme {
    /// A file or folder path, or an `http(s)://` address for web themes.
    pub source: String,
    pub name: String,
    /// Ignored for kinds where it means nothing (shader, scene).
    pub fit: Option<Fit>,
    /// Launch arguments for program themes, as one command-line style string.
    pub args: Option<String>,
}

/// The first of `<base>`, `<base>-2`, `<base>-3`, ... that doesn't exist in `root`.
fn free_dir(root: &Path, base: &str) -> Result<PathBuf> {
    (1u32..)
        .map(|n| root.join(if n == 1 { base.to_owned() } else { format!("{base}-{n}") }))
        .find(|dir| !dir.exists())
        .context("no free theme folder name")
}

/// Creates `<root>\<slug>\theme.json` pointing at the absolute source, and
/// checks it loads. Never overwrites: a taken slug gets `-2`, `-3`, ... Returns the folder.
pub fn create_in(root: &Path, new: &NewTheme) -> Result<PathBuf> {
    let name = new.name.trim();
    if name.is_empty() {
        bail!("give the theme a name");
    }
    let source::Resolved { kind, source } = source::resolve(&new.source)?;
    let args = if kind == Kind::App { source::split_args(new.args.as_deref().unwrap_or(""))? } else { Vec::new() };
    let text = source::manifest(name, kind, &source, new.fit.unwrap_or_default(), &args);

    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let dir = free_dir(root, &source::slug(name))?;
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

/// Copies a theme folder (relative assets included) into `root` as
/// `<folder>-copy`, named "<name> (copy)", and checks the copy loads.
/// Returns the new folder; the original is untouched.
pub fn duplicate_into(root: &Path, src: &Path) -> Result<PathBuf> {
    let original = LoadedTheme::load(src)?;
    let folder = src.file_name().map(|n| n.to_string_lossy().into_owned()).context("the theme folder has no name")?;
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    if canonical(root).starts_with(canonical(src)) {
        bail!("can't copy a theme into its own folder");
    }
    let dir = free_dir(root, &format!("{}-copy", source::slug(&folder)))?;

    let copied = copy_dir(src, &dir).and_then(|()| {
        let path = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&path)?;
        std::fs::write(&path, manifest::with_name(&text, &format!("{} (copy)", original.theme.name))?)?;
        LoadedTheme::load(&dir).map(drop)
    });
    if let Err(e) = copied {
        // The folder was created by `copy_dir` a moment ago, so it only holds our files.
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.context("copying the theme"));
    }
    Ok(dir)
}

/// Recursively copies `src` to the new folder `dst`; symlinks are skipped.
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir(dst).with_context(|| format!("creating {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Only themes directly inside the user themes folder may be edited:
/// bundled themes are read-only, and the webview can't aim writes elsewhere.
pub fn is_user_theme(dir: &Path, user_root: &Path) -> bool {
    canonical(dir).parent() == Some(canonical(user_root).as_path())
}

/// The `windows` section of a theme's `theme.json`, as the editor shows it.
pub fn read_style(dir: &Path) -> Result<WindowsStyle> {
    LoadedTheme::load(dir)?;
    let path = dir.join(MANIFEST);
    manifest::read_style(&std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?)
}

/// Writes `style` into a user theme's `theme.json`, leaving every other field
/// alone. The edited text is validated with the theme loader before it
/// touches the disk, so a bad value is refused and the file is never half-valid
/// (the engine hot-reloads this file).
pub fn save_style(dir: &Path, user_root: &Path, style: &WindowsStyle) -> Result<()> {
    if !is_user_theme(dir, user_root) {
        bail!("only themes in your themes folder can be edited; duplicate this one first");
    }
    let path = dir.join(MANIFEST);
    let original = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if manifest::read_style(&original)? == *style {
        return Ok(());
    }
    let updated = manifest::with_style(&original, style)?;
    LoadedTheme::parse(dir.to_path_buf(), &updated).context("these styling values were refused")?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, updated)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// A `widgets` / `bars` section of a theme's `theme.json`, as the editor shows it.
pub fn read_skin(dir: &Path, kind: SkinKind) -> Result<SkinEdit> {
    LoadedTheme::load(dir)?;
    let path = dir.join(MANIFEST);
    manifest::read_skin(&std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?, kind)
}

/// Writes a skin into a user theme's `theme.json`, leaving every other field
/// alone. Like `save_style`, the edited text is validated by the theme loader
/// first (ranges, colors, a css path that stays inside the theme).
pub fn save_skin(dir: &Path, user_root: &Path, kind: SkinKind, skin: &SkinEdit) -> Result<()> {
    if !is_user_theme(dir, user_root) {
        bail!("only themes in your themes folder can be edited; duplicate this one first");
    }
    let path = dir.join(MANIFEST);
    let original = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if manifest::read_skin(&original, kind)? == *skin {
        return Ok(());
    }
    let updated = manifest::with_skin(&original, kind, skin)?;
    LoadedTheme::parse(dir.to_path_buf(), &updated).with_context(|| format!("these {} values were refused", kind.key()))?;
    fsio::write_atomic(&path, updated)
}

/// Brings a style sheet into a user theme and returns the path to put in the
/// skin's `css`. A file already inside the theme folder is used where it is;
/// any other is copied in as `<kind>.css` (replacing the last one picked).
pub fn import_skin_css(dir: &Path, user_root: &Path, kind: SkinKind, source: &Path) -> Result<String> {
    if !is_user_theme(dir, user_root) {
        bail!("only themes in your themes folder can be edited; duplicate this one first");
    }
    if !source.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("css")) {
        bail!("pick a .css file");
    }
    if let Ok(inside) = canonical(source).strip_prefix(canonical(dir)) {
        return Ok(inside.display().to_string());
    }
    let name = format!("{}.css", kind.key());
    std::fs::copy(source, dir.join(&name)).with_context(|| format!("copying {} into the theme", source.display()))?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    fn image_theme(source: &Path, name: &str) -> NewTheme {
        NewTheme { source: source.to_string_lossy().into_owned(), name: name.into(), fit: Some(Fit::Contain), args: None }
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
        let script = work.join("run.zip");
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

    #[test]
    fn web_and_app_themes_are_created_with_their_options() {
        let work = temp_dir("web-app");
        let root = work.join("themes");
        let web = NewTheme { source: " https://example.com/clock ".into(), name: "Clock".into(), fit: None, args: None };
        let dir = create_in(&root, &web).unwrap();
        let Wallpaper::Web(w) = LoadedTheme::load(&dir).unwrap().theme.wallpaper else { panic!("not a web theme") };
        assert_eq!(w.url, "https://example.com/clock");
        assert!(create_in(&root, &NewTheme { source: "example.com".into(), ..web }).is_err());

        let exe = work.join("Player.exe");
        std::fs::write(&exe, "").unwrap();
        let app = NewTheme {
            source: exe.to_string_lossy().into_owned(),
            name: "Player".into(),
            fit: None,
            args: Some(source::join_args(source::UNITY_PLAYER.args)),
        };
        let dir = create_in(&root, &app).unwrap();
        let Wallpaper::App(a) = LoadedTheme::load(&dir).unwrap().theme.wallpaper else { panic!("not an app theme") };
        assert_eq!(a.args, ["-parentHWND", "{hwnd}", "delayed"]);
        assert!(create_in(&root, &NewTheme { args: Some("\"open".into()), name: "Bad".into(), ..app }).is_err());
        assert_eq!(theme_dirs(&root).len(), 2, "the failed attempts left nothing behind");
        std::fs::remove_dir_all(&work).unwrap();
    }

    /// A bundled-style theme: a shader beside its manifest, plus a nested asset.
    fn shader_theme(root: &Path, folder: &str) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("a.wgsl"), "fn wallpaper(f: vec2f) -> vec4f { return vec4f(0.0); }").unwrap();
        std::fs::write(dir.join("assets").join("noise.png"), [1, 2, 3]).unwrap();
        std::fs::write(dir.join(MANIFEST), r#"{ "name": "Shady", "wallpaper": { "kind": "shader", "source": "a.wgsl" } }"#).unwrap();
        dir
    }

    #[test]
    fn duplicating_copies_the_whole_folder_under_a_free_name() {
        let work = temp_dir("duplicate");
        let bundled = shader_theme(&work.join("bundled"), "shady");
        let mine = work.join("mine");

        let first = duplicate_into(&mine, &bundled).unwrap();
        let second = duplicate_into(&mine, &bundled).unwrap();
        assert_eq!(first.file_name().unwrap(), "shady-copy");
        assert_eq!(second.file_name().unwrap(), "shady-copy-2");
        assert_eq!(std::fs::read(first.join("assets").join("noise.png")).unwrap(), [1, 2, 3]);
        assert!(first.join("a.wgsl").is_file());

        assert_eq!(LoadedTheme::load(&first).unwrap().theme.name, "Shady (copy)");
        assert_eq!(LoadedTheme::load(&bundled).unwrap().theme.name, "Shady", "the original is untouched");

        assert!(duplicate_into(&bundled, &bundled).is_err(), "not into itself");
        assert!(duplicate_into(&mine, &work.join("missing")).is_err());
        assert_eq!(theme_dirs(&mine).len(), 2);
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn only_themes_directly_in_the_user_folder_are_editable() {
        let work = temp_dir("editable");
        let mine = work.join("mine");
        let own = shader_theme(&mine, "own");
        let bundled = shader_theme(&work.join("bundled"), "shady");
        assert!(is_user_theme(&own, &mine));
        assert!(!is_user_theme(&bundled, &mine));
        assert!(!is_user_theme(&mine, &mine));
        assert!(!is_user_theme(&own.join("assets"), &mine));
        assert!(!is_user_theme(&own.join("..").join("..").join("bundled").join("shady"), &mine));

        let style = WindowsStyle { corners: Some("square".into()), ..Default::default() };
        assert!(save_style(&bundled, &mine, &style).is_err());
        assert!(!std::fs::read_to_string(bundled.join(MANIFEST)).unwrap().contains("windows"));
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn saving_a_skin_is_validated_by_the_theme_crate_and_edits_only_its_section() {
        let work = temp_dir("skin");
        let mine = work.join("mine");
        let dir = shader_theme(&mine, "own");
        let before = LoadedTheme::load(&dir).unwrap();

        let skin = SkinEdit {
            appearance: serde_json::from_str(r##"{ "surface": "solid", "tint": "#202020", "radius": 12, "shadow": false }"##).unwrap(),
            css: Some("widgets.css".into()),
        };
        save_skin(&dir, &mine, SkinKind::Widgets, &skin).unwrap();
        assert_eq!(read_skin(&dir, SkinKind::Widgets).unwrap(), skin);
        assert_eq!(read_skin(&dir, SkinKind::Bars).unwrap(), SkinEdit::default());
        let loaded = LoadedTheme::load(&dir).unwrap();
        assert_eq!(loaded.theme.wallpaper, before.theme.wallpaper);
        let widgets = loaded.theme.widgets.unwrap();
        assert_eq!(widgets.appearance.unwrap().surface, Some(mypaper_theme::Surface::Solid));
        assert!(entry(&dir, Origin::User, None).widgets_skin && !entry(&dir, Origin::User, None).bars_skin);

        // Out-of-range and escaping values are refused, and the file stays as it was.
        let good = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
        let loud = SkinEdit { appearance: serde_json::from_str(r#"{ "blur": 500 }"#).unwrap(), css: None };
        let error = format!("{:#}", save_skin(&dir, &mine, SkinKind::Bars, &loud).unwrap_err());
        assert!(error.contains("refused") && error.contains("blur"), "{error}");
        let escaping = SkinEdit { css: Some(r"..\..\x.css".into()), ..Default::default() };
        assert!(save_skin(&dir, &mine, SkinKind::Bars, &escaping).is_err());
        assert_eq!(std::fs::read_to_string(dir.join(MANIFEST)).unwrap(), good);
        assert!(!dir.join("theme.json.studio.tmp").exists());

        // Bundled themes can't be edited.
        let bundled = shader_theme(&work.join("bundled"), "shady");
        assert!(save_skin(&bundled, &mine, SkinKind::Bars, &skin).is_err());

        save_skin(&dir, &mine, SkinKind::Widgets, &SkinEdit::default()).unwrap();
        assert!(!entry(&dir, Origin::User, None).widgets_skin);
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn style_sheets_are_copied_into_the_theme_unless_already_inside() {
        let work = temp_dir("skin-css");
        let mine = work.join("mine");
        let dir = shader_theme(&mine, "own");
        let outside = work.join("fancy.css");
        std::fs::write(&outside, "body{}").unwrap();

        assert_eq!(import_skin_css(&dir, &mine, SkinKind::Bars, &outside).unwrap(), "bars.css");
        assert_eq!(std::fs::read_to_string(dir.join("bars.css")).unwrap(), "body{}");
        std::fs::write(dir.join("mine.css"), "x").unwrap();
        assert_eq!(import_skin_css(&dir, &mine, SkinKind::Bars, &dir.join("mine.css")).unwrap(), "mine.css");

        std::fs::write(work.join("notes.txt"), "").unwrap();
        assert!(import_skin_css(&dir, &mine, SkinKind::Bars, &work.join("notes.txt")).is_err());
        let bundled = shader_theme(&work.join("bundled"), "shady");
        assert!(import_skin_css(&bundled, &mine, SkinKind::Bars, &outside).is_err());
        assert!(!bundled.join("bars.css").exists());
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn saving_a_style_edits_only_the_windows_section_and_refuses_bad_values() {
        let work = temp_dir("style");
        let mine = work.join("mine");
        let dir = shader_theme(&mine, "own");
        let before = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();

        let style = WindowsStyle {
            accent: Some("#ff8800".into()),
            backdrop: Some("acrylic".into()),
            dark_mode: Some(false),
            ..Default::default()
        };
        save_style(&dir, &mine, &style).unwrap();
        assert_eq!(read_style(&dir).unwrap(), style);
        let loaded = LoadedTheme::load(&dir).unwrap();
        assert_eq!(loaded.theme.name, "Shady");
        assert!(matches!(loaded.theme.wallpaper, Wallpaper::Shader(_)));
        assert!(entry(&dir, Origin::User, None).styled);

        let good = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
        let bad = WindowsStyle { accent: Some("orange".into()), ..Default::default() };
        let error = format!("{:#}", save_style(&dir, &mine, &bad).unwrap_err());
        assert!(error.contains("refused"), "{error}");
        assert_eq!(std::fs::read_to_string(dir.join(MANIFEST)).unwrap(), good, "a refused edit leaves the file alone");
        assert!(!dir.join("theme.json.tmp").exists());

        save_style(&dir, &mine, &WindowsStyle::default()).unwrap();
        assert!(!entry(&dir, Origin::User, None).styled);
        let after = LoadedTheme::load(&dir).unwrap();
        assert_eq!(after.theme.wallpaper, LoadedTheme::parse(dir.clone(), &before).unwrap().theme.wallpaper);
        std::fs::remove_dir_all(&work).unwrap();
    }
}
