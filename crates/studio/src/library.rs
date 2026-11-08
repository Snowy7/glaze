//! The wallpaper library (D1: "Wallpapers"; a wallpaper may bring a look): every theme folder
//! in the theme roots, which one is on the desktop, its credit, and adding, copying and
//! removing your own. Ported from the web Studio's `themes.rs` and `source.rs`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, LoadedTheme, MANIFEST, Wallpaper as Source};
use serde::Deserialize;
use serde_json::json;

/// The wallpaper kinds, as `theme.json` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Shader,
    Image,
    Slideshow,
    Video,
    Scene,
    Web,
    App,
    Stack,
}

impl Kind {
    pub fn of(wallpaper: &Source) -> Self {
        match wallpaper {
            Source::Shader(_) => Self::Shader,
            Source::Image(_) => Self::Image,
            Source::Slideshow(_) => Self::Slideshow,
            Source::Video(_) => Self::Video,
            Source::Scene(_) => Self::Scene,
            Source::Web(_) => Self::Web,
            Source::App(_) => Self::App,
            Source::Stack(_) => Self::Stack,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Shader => "Shader",
            Self::Image => "Picture",
            Self::Slideshow => "Slideshow",
            Self::Video => "Video",
            Self::Scene => "3D scene",
            Self::Web => "Web page",
            Self::App => "App",
            Self::Stack => "Composition",
        }
    }

    /// Whether `fit` means anything for this kind (shaders and scenes fill the screen).
    pub fn uses_fit(self) -> bool {
        matches!(self, Self::Image | Self::Slideshow | Self::Video)
    }

    /// Animated kinds say "Live" on their card.
    pub fn live(self) -> bool {
        !matches!(self, Self::Image)
    }
}

/// Where a wallpaper comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// In `%APPDATA%\mypaper\themes`.
    User,
    /// In the bundled `themes/` folder.
    Bundled,
    /// The active one lives elsewhere (an absolute path in the config).
    External,
}

/// A bundled wallpaper's record in `themes/catalog.json`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Credit {
    pub category: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    /// Non-commercial use only.
    pub noncommercial: bool,
}

#[derive(Debug, Clone)]
pub struct Wallpaper {
    pub name: String,
    pub dir: PathBuf,
    /// What `config.theme` says to use it.
    pub apply_as: String,
    /// The folder name: the key of its per-wallpaper engine settings.
    pub id: String,
    pub origin: Origin,
    pub kind: Option<Kind>,
    /// Why `theme.json` doesn't load (the card says "Can't load").
    pub error: Option<String>,
    /// A picture to show: `preview.*`, the image itself, a slideshow's first picture.
    pub preview: Option<PathBuf>,
    /// A video whose still comes from the shell thumbnail cache.
    pub video: Option<PathBuf>,
    pub active: bool,
    /// It brings a look: Windows styling, or a widget or bar skin.
    pub brings_look: bool,
    pub credit: Option<Credit>,
    /// The engine options the theme itself sets (fps, gpu).
    pub engine: mypaper_config::RenderOptions,
    pub interactive: Option<bool>,
    pub hidden: bool,
}

impl Wallpaper {
    pub fn category(&self) -> &str {
        match self.origin {
            Origin::User | Origin::External => "Yours",
            Origin::Bundled => self
                .credit
                .as_ref()
                .and_then(|c| c.category.as_deref())
                .unwrap_or("More"),
        }
    }

    pub fn editable(&self) -> bool {
        self.origin == Origin::User
    }

    /// "Shader · Live · City & rain".
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if let Some(kind) = self.kind {
            parts.push(kind.label());
            if kind.live() {
                parts.push("Live");
            }
        } else {
            parts.push("Can't load");
        }
        parts.join(" · ")
    }

    /// Words the library search matches.
    pub fn haystack(&self) -> String {
        format!(
            "{} {} {} {}",
            self.name,
            self.kind.map_or("", Kind::label),
            self.category(),
            self.credit
                .as_ref()
                .and_then(|c| c.author.as_deref())
                .unwrap_or("")
        )
        .to_lowercase()
    }
}

/// The library's sections, in order.
pub const CATEGORIES: [&str; 7] = ["Yours", "City & rain", "Nature", "Space", "Cozy", "Abstract", "More"];

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub items: Vec<Wallpaper>,
}

#[derive(Debug, Default, Deserialize)]
struct Catalog {
    #[serde(default)]
    themes: Vec<CatalogTheme>,
}

#[derive(Debug, Deserialize)]
struct CatalogTheme {
    id: String,
    category: Option<String>,
    author: Option<String>,
    license: Option<String>,
    source: Option<String>,
    #[serde(default)]
    noncommercial: bool,
    #[serde(default)]
    hidden: bool,
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl Library {
    /// Every wallpaper in the theme roots (yours first), with `active` the config's theme.
    pub fn scan(active: Option<&str>) -> Self {
        let roots = mypaper_theme::theme_roots();
        let user = mypaper_config::user_themes_dir().ok().map(|d| canonical(&d));
        Self::scan_in(&roots, user.as_deref(), active)
    }

    /// `scan` over given roots (goldens use the repo's `themes/`).
    pub fn scan_in(roots: &[PathBuf], user: Option<&Path>, active: Option<&str>) -> Self {
        let active_dir = active
            .and_then(|a| mypaper_theme::locate(Some(a)).ok())
            .map(|d| canonical(&d));
        let mut items = Vec::new();
        for root in roots {
            let origin = if user == Some(canonical(root).as_path()) {
                Origin::User
            } else {
                Origin::Bundled
            };
            let catalog = read_catalog(root);
            for dir in theme_dirs(root) {
                let entry = entry(&dir, origin, &catalog, active_dir.as_deref());
                if !entry.hidden || entry.active {
                    items.push(entry);
                }
            }
        }
        if let Some(dir) = &active_dir
            && !items.iter().any(|w| canonical(&w.dir) == *dir)
        {
            items.insert(0, entry(dir, Origin::External, &Catalog::default(), Some(dir)));
        }
        // Yours first, then the catalogue's order of categories; broken ones sink.
        items.sort_by_key(|w| {
            (
                CATEGORIES.iter().position(|c| *c == w.category()).unwrap_or(CATEGORIES.len()),
                w.error.is_some(),
                w.name.to_lowercase(),
            )
        });
        Self { items }
    }

    pub fn active(&self) -> Option<usize> {
        self.items.iter().position(|w| w.active)
    }

    /// Marks the wallpaper the config now names as on the desktop.
    pub fn set_active(&mut self, theme: Option<&str>) {
        let dir = theme
            .and_then(|t| mypaper_theme::locate(Some(t)).ok())
            .map(|d| canonical(&d));
        for w in &mut self.items {
            w.active = dir.as_deref() == Some(canonical(&w.dir).as_path());
        }
    }

    pub fn by_dir(&self, dir: &Path) -> Option<usize> {
        let dir = canonical(dir);
        self.items.iter().position(|w| canonical(&w.dir) == dir)
    }
}

fn read_catalog(root: &Path) -> Catalog {
    std::fs::read_to_string(root.join("catalog.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Subfolders of `root` with a `theme.json` (examples are developer themes: left out).
fn theme_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".draft-")))
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    dirs
}

fn entry(dir: &Path, origin: Origin, catalog: &Catalog, active: Option<&Path>) -> Wallpaper {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let loaded = LoadedTheme::load(dir);
    let record = (origin == Origin::Bundled)
        .then(|| catalog.themes.iter().find(|t| t.id == folder))
        .flatten();
    let by_name = origin != Origin::External
        && mypaper_theme::locate(Some(&folder)).is_ok_and(|d| canonical(&d) == canonical(dir));
    let ok = loaded.as_ref().ok();
    let video = ok.and_then(|t| match &t.theme.wallpaper {
        Source::Video(v) => Some(t.asset(&v.source)),
        _ => None,
    });
    Wallpaper {
        // The licence shows as a badge; the name doesn't repeat it.
        name: ok.map_or(folder.clone(), |t| {
            t.theme.name.trim_end_matches(" (non-commercial)").to_owned()
        }),
        dir: dir.to_path_buf(),
        apply_as: if by_name {
            folder.clone()
        } else {
            dir.display().to_string()
        },
        id: folder,
        origin,
        kind: ok.map(|t| Kind::of(&t.theme.wallpaper)),
        error: loaded.as_ref().err().map(|e| format!("{e:#}")),
        preview: preview(dir, ok),
        video,
        active: active.is_some_and(|a| a == canonical(dir)),
        brings_look: ok.is_some_and(|t| {
            t.theme.windows.is_some() || t.theme.widgets.is_some() || t.theme.bars.is_some()
        }),
        credit: record.map(|r| Credit {
            category: r.category.clone(),
            author: r.author.clone(),
            license: r.license.clone(),
            source: r.source.clone(),
            noncommercial: r.noncommercial,
        }),
        engine: ok.map(|t| t.theme.engine).unwrap_or_default(),
        interactive: ok.and_then(|t| match &t.theme.wallpaper {
            Source::Web(w) => Some(w.interactive),
            Source::Scene(s) => Some(s.interactive),
            _ => None,
        }),
        hidden: ok.is_some_and(|t| t.theme.hidden) || record.is_some_and(|r| r.hidden),
    }
}

/// `preview.*` in the folder, else the picture itself or a slideshow's first.
fn preview(dir: &Path, loaded: Option<&LoadedTheme>) -> Option<PathBuf> {
    ["preview.png", "preview.jpg", "preview.jpeg", "preview.webp"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
        .or_else(|| match &loaded?.theme.wallpaper {
            Source::Image(i) => Some(loaded?.asset(&i.source)).filter(|p| p.is_file() && is_image(p)),
            Source::Slideshow(s) => first_image_in(&loaded?.asset(&s.folder)),
            _ => None,
        })
}

/// Changes one value of your own wallpaper's `theme.json` (`@both` writes it for both the
/// widgets' and the bars' look; `glass/clarity` is stored as the glass's tint, 1 - clarity),
/// checked by the theme's own parser before it's written.
pub fn edit_theme(dir: &Path, pointer: &str, value: Option<serde_json::Value>) -> Result<()> {
    let user = mypaper_config::user_themes_dir()?;
    if canonical(dir).parent() != Some(canonical(&user).as_path()) {
        bail!("a built-in wallpaper can't be changed: make a copy first");
    }
    let file = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let mut root: serde_json::Value = serde_json::from_str(&text)?;
    let (pointer, value) = match pointer.strip_suffix("/glass/clarity") {
        Some(base) => (format!("{base}/glass/tint"), value.and_then(|v| v.as_f64()).map(|c| json!(((1.0 - c) * 1000.0).round() / 1000.0))),
        None => (pointer.to_owned(), value),
    };
    let targets: Vec<String> = match pointer.strip_prefix("/@both") {
        Some(rest) => vec![format!("/widgets{rest}"), format!("/bars{rest}")],
        None => vec![pointer],
    };
    for path in targets {
        crate::docs::apply_op(&mut root, &crate::docs::Op { doc: crate::docs::DocId::Studio, path, value: value.clone() });
    }
    let new = serde_json::to_string_pretty(&root)? + "\n";
    LoadedTheme::parse(dir.to_path_buf(), &new).context("that change isn't valid")?;
    crate::docs::write_atomic(&file, new.as_bytes())
}

// --- Adding your own ----------------------------------------------------------------------------

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "heic", "heif"];
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
        "html" | "htm" => Some(Kind::Web),
        "exe" => Some(Kind::App),
        _ => None,
    }
}

pub fn first_image_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && is_image(p))
}

/// What a picked file, folder or typed address becomes: its kind and the source to write.
pub fn resolve(input: &str) -> Result<(Kind, String)> {
    let input = input.trim();
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let host = input.split_once("://").map_or("", |(_, r)| r);
        let host = host.split(['/', '?', '#']).next().unwrap_or("");
        if host.is_empty() || input.contains(char::is_whitespace) {
            bail!("{input:?} is not a valid web address");
        }
        return Ok((Kind::Web, input.to_owned()));
    }
    if input.contains("://") {
        bail!("only http:// and https:// addresses can be wallpapers");
    }
    let path = std::path::absolute(input).context("resolving the path")?;
    let kind = if path.is_dir() {
        if first_image_in(&path).is_none() {
            bail!("{} has no pictures in it", path.display());
        }
        Kind::Slideshow
    } else {
        if !path.exists() {
            bail!("{} doesn't exist", path.display());
        }
        let ext = extension(&path).with_context(|| format!("{} has no file extension", path.display()))?;
        kind_for_extension(&ext).with_context(|| format!("mypaper can't show .{ext} files"))?
    };
    Ok((kind, path.to_string_lossy().into_owned()))
}

/// A readable default name: the file or folder name, or a web page's host.
pub fn suggested_name(input: &str) -> String {
    let input = input.trim();
    if let Some((_, rest)) = input.split_once("://") {
        let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
        let host = host.strip_prefix("www.").unwrap_or(host);
        return if host.is_empty() { "Web page".into() } else { host.into() };
    }
    let path = Path::new(input);
    let stem = if path.is_dir() { path.file_name() } else { path.file_stem() };
    let name = stem
        .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() { "My wallpaper".into() } else { name }
}

/// A lowercase, folder-safe name.
pub fn slug(name: &str) -> String {
    const RESERVED: &[&str] = &["con", "prn", "aux", "nul", "com1", "lpt1"];
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out = out[..out.len().min(48)].trim_end_matches('-');
    match out {
        "" => "wallpaper".into(),
        r if RESERVED.contains(&r) => format!("wallpaper-{r}"),
        ok => ok.into(),
    }
}

/// Splits an app's argument line (quotes group words).
pub fn split_args(text: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
                any = false;
            }
            c => current.push(c),
        }
    }
    if quoted {
        bail!("a quote isn't closed");
    }
    if any || !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

/// Unity players render into the desktop with these arguments.
pub const UNITY_ARGS: &str = "-parentHWND {hwnd} delayed";

/// A new wallpaper of your own.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWallpaper {
    pub source: String,
    pub name: String,
    pub fit: Fit,
    pub args: String,
}

/// Creates `<root>\<slug>\theme.json` (never overwriting: `-2`, `-3`…) and checks it loads.
pub fn create_in(root: &Path, new: &NewWallpaper) -> Result<PathBuf> {
    let name = new.name.trim();
    if name.is_empty() {
        bail!("give the wallpaper a name");
    }
    let (kind, source) = resolve(&new.source)?;
    let args = if kind == Kind::App { split_args(&new.args)? } else { Vec::new() };
    let fit = match new.fit {
        Fit::Cover => "cover",
        Fit::Contain => "contain",
        Fit::Stretch => "stretch",
        Fit::Center => "center",
    };
    let wallpaper = match kind {
        Kind::Shader => json!({ "kind": "shader", "source": source }),
        Kind::Scene => json!({ "kind": "scene", "source": source }),
        Kind::Image => json!({ "kind": "image", "source": source, "fit": fit }),
        Kind::Video => json!({ "kind": "video", "source": source, "fit": fit }),
        Kind::Slideshow => json!({ "kind": "slideshow", "folder": source, "fit": fit }),
        Kind::Web => json!({ "kind": "web", "url": source }),
        Kind::App if args.is_empty() => json!({ "kind": "app", "command": source }),
        Kind::App => json!({ "kind": "app", "command": source, "args": args }),
        Kind::Stack => bail!("use Create to compose a wallpaper"),
    };
    let text = serde_json::to_string_pretty(&json!({ "name": name, "wallpaper": wallpaper }))? + "\n";
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let dir = free_dir(root, &slug(name))?;
    std::fs::create_dir(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let written = std::fs::write(dir.join(MANIFEST), text)
        .context("writing theme.json")
        .and_then(|()| LoadedTheme::load(&dir).map(drop));
    if let Err(e) = written {
        // Made a moment ago: only our own file is in it.
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

fn free_dir(root: &Path, base: &str) -> Result<PathBuf> {
    (1u32..)
        .map(|n| root.join(if n == 1 { base.to_owned() } else { format!("{base}-{n}") }))
        .find(|dir| !dir.exists())
        .context("no free folder name")
}

/// Copies a wallpaper folder into `root` as "<name> (copy)", so its look can be edited.
pub fn duplicate_into(root: &Path, src: &Path) -> Result<PathBuf> {
    let original = LoadedTheme::load(src)?;
    let folder = src.file_name().map(|n| n.to_string_lossy().into_owned()).context("no folder name")?;
    std::fs::create_dir_all(root)?;
    if canonical(root).starts_with(canonical(src)) {
        bail!("can't copy a wallpaper into its own folder");
    }
    let dir = free_dir(root, &format!("{}-copy", slug(&folder)))?;
    let copied = copy_dir(src, &dir).and_then(|()| {
        let path = dir.join(MANIFEST);
        let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        value["name"] = json!(format!("{} (copy)", original.theme.name));
        std::fs::write(&path, serde_json::to_string_pretty(&value)? + "\n")?;
        LoadedTheme::load(&dir).map(drop)
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.context("copying the wallpaper"));
    }
    Ok(dir)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Moves one of your wallpaper folders to the Recycle Bin (Windows can restore it).
pub fn recycle(dir: &Path) -> Result<()> {
    use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW};
    let mut from: Vec<u16> = dir.as_os_str().encode_wide_vec();
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    // SAFETY: a double-NUL terminated path list that outlives the call.
    let status = unsafe { SHFileOperationW(&mut op) };
    if status != 0 || op.fAnyOperationsAborted.as_bool() {
        bail!("couldn't move {} to the Recycle Bin", dir.display());
    }
    Ok(())
}

trait WideVec {
    fn encode_wide_vec(&self) -> Vec<u16>;
}

impl WideVec for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repos_wallpapers_are_listed_by_category_with_credits() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let library = Library::scan_in(&[root], None, None);
        let rain = library.items.iter().find(|w| w.id == "rain-city").expect("rain-city");
        assert_eq!(rain.category(), "City & rain");
        assert!(rain.credit.as_ref().is_some_and(|c| c.noncommercial));
        assert!(rain.preview.is_some(), "a picture to show");
        let order: Vec<_> = library.items.iter().map(|w| w.category()).collect();
        let mut sorted = order.clone();
        sorted.sort_by_key(|c| CATEGORIES.iter().position(|x| x == c));
        assert_eq!(order, sorted, "grouped in the catalogue's order");
    }

    #[test]
    fn names_slugs_and_arguments() {
        assert_eq!(suggested_name("https://www.example.com/a"), "example.com");
        assert_eq!(slug("Rain / City!"), "rain-city");
        assert_eq!(slug("con"), "wallpaper-con");
        assert_eq!(split_args(r#"-parentHWND {hwnd} "a b""#).unwrap(), ["-parentHWND", "{hwnd}", "a b"]);
        assert!(split_args(r#""open"#).is_err());
        assert!(resolve("ftp://x").is_err());
        assert_eq!(resolve("https://example.com").unwrap().0, Kind::Web);
    }
}
