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
        "html" | "htm" => Some(Kind::Web),
        "exe" => Some(Kind::App),
        _ => None,
    }
}

fn has_web_scheme(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// A web wallpaper's address: `http(s)://host...` with no spaces. Returns it trimmed.
pub fn check_url(text: &str) -> Result<&str> {
    let url = text.trim();
    if !has_web_scheme(url) {
        bail!("a web address must start with http:// or https://");
    }
    let host = url.split_once("://").map_or("", |(_, rest)| rest);
    let host = host.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.starts_with(':') || url.contains(char::is_whitespace) {
        bail!("{url:?} is not a valid web address");
    }
    Ok(url)
}

/// Where a theme's content comes from: a web address as typed, or an absolute path.
pub struct Resolved {
    pub kind: Kind,
    pub source: String,
}

/// Classifies what the user gave us. Web addresses (`http(s)://`) are checked
/// as URLs, everything else as a picked file or folder.
pub fn resolve(input: &str) -> Result<Resolved> {
    let input = input.trim();
    if has_web_scheme(input) {
        return Ok(Resolved {
            kind: Kind::Web,
            source: check_url(input)?.to_owned(),
        });
    }
    if input.contains("://") {
        bail!("only http:// and https:// addresses can be wallpapers");
    }
    let path = std::path::absolute(input).context("resolving the source path")?;
    let kind = check(&path)?;
    Ok(Resolved {
        kind,
        source: path.to_string_lossy().into_owned(),
    })
}

/// A launch-argument preset for program wallpapers; `{hwnd}` becomes the handle
/// of the window the program should render into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ArgPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub args: &'static [&'static str],
}

pub const UNITY_PLAYER: ArgPreset = ArgPreset {
    id: "unity",
    label: "Unity player",
    args: &["-parentHWND", "{hwnd}", "delayed"],
};

pub const ARG_PRESETS: &[ArgPreset] = &[UNITY_PLAYER];

/// Unity players ship `UnityPlayer.dll` beside the exe, which is a safe tell.
pub fn suggested_preset(exe: &Path) -> Option<ArgPreset> {
    exe.with_file_name("UnityPlayer.dll")
        .is_file()
        .then_some(UNITY_PLAYER)
}

/// Splits a command-line style string: whitespace separates arguments and
/// double quotes group words. Backslashes are literal, so Windows paths survive.
pub fn split_args(text: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if quoted {
        bail!("unclosed quote in the arguments");
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// The inverse of `split_args`, for showing arguments in a text field.
pub fn join_args<S: AsRef<str>>(args: &[S]) -> String {
    let quote = |arg: &str| {
        if arg.is_empty() || arg.contains(char::is_whitespace) {
            format!("\"{arg}\"")
        } else {
            arg.to_owned()
        }
    };
    args.iter()
        .map(|arg| quote(arg.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A folder is a slideshow; a file is classified by its extension.
pub fn detect(path: &Path) -> Result<Kind> {
    if path.is_dir() {
        return Ok(Kind::Slideshow);
    }
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }
    let ext =
        extension(path).with_context(|| format!("{} has no file extension", path.display()))?;
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
    let stem = if path.is_dir() {
        path.file_name()
    } else {
        path.file_stem()
    };
    let raw = stem.map(|s| s.to_string_lossy().replace(['_', '-'], " "));
    let name = raw
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        "My theme".into()
    } else {
        name
    }
}

/// A theme name for a web address: its host without `www.`.
pub fn url_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "Web page".into()
    } else {
        host.into()
    }
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

/// The `theme.json` text for a theme whose content is at `source` (an absolute
/// path, or a web address for `Kind::Web`). `args` only matter for programs.
pub fn manifest(name: &str, kind: Kind, source: &str, fit: Fit, args: &[String]) -> String {
    let fit = fit_name(fit);
    let wallpaper = match kind {
        Kind::Shader => json!({ "kind": "shader", "source": source }),
        Kind::Scene => json!({ "kind": "scene", "source": source }),
        Kind::Image => json!({ "kind": "image", "source": source, "fit": fit }),
        Kind::Video => json!({ "kind": "video", "source": source, "fit": fit }),
        Kind::Slideshow => json!({ "kind": "slideshow", "folder": source, "fit": fit }),
        Kind::Web => json!({ "kind": "web", "url": source }),
        Kind::App if args.is_empty() => json!({ "kind": "app", "command": source }),
        Kind::App => json!({ "kind": "app", "command": source, "args": args }),
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
        assert_eq!(kind("a.HTML"), Some(Kind::Web));
        assert_eq!(kind("a.exe"), Some(Kind::App));
        assert_eq!(kind("a.zip"), None);
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
        assert_eq!(
            suggested_name(Path::new("C:/pics/my_cool-photo.jpg")),
            "my cool photo"
        );
        assert_eq!(suggested_name(Path::new("")), "My theme");
        assert_eq!(
            url_name("https://www.example.com:8080/clock?x=1"),
            "example.com"
        );
        assert_eq!(url_name("http://localhost"), "localhost");
    }

    #[test]
    fn manifests_load_back_for_every_kind() {
        let source = r"C:\Media\thing";
        for (kind, uses_fit) in [
            (Kind::Shader, false),
            (Kind::Image, true),
            (Kind::Slideshow, true),
            (Kind::Video, true),
            (Kind::Scene, false),
            (Kind::Web, false),
            (Kind::App, false),
        ] {
            let text = manifest("T \"quoted\"", kind, source, Fit::Contain, &[]);
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
        let text = manifest("P", Kind::Image, r"C:\a.png", Fit::Stretch, &[]);
        let loaded = LoadedTheme::parse(PathBuf::from("themes/p"), &text).unwrap();
        let Wallpaper::Image(image) = loaded.theme.wallpaper else {
            panic!("expected an image theme");
        };
        assert_eq!(image.fit, Fit::Stretch);
        assert_eq!(image.source, PathBuf::from(r"C:\a.png"));
    }

    #[test]
    fn web_addresses_must_be_http_with_a_host() {
        assert_eq!(
            check_url("  https://example.com/a?b=1 ").unwrap(),
            "https://example.com/a?b=1"
        );
        assert!(check_url("HTTP://localhost:8080").is_ok());
        for bad in [
            "example.com",
            "ftp://example.com",
            "https://",
            "https:///path",
            "https://:80",
            "https://a b.com",
            "",
        ] {
            assert!(check_url(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(resolve("file:///C:/x.html").is_err());
        let web = resolve("https://example.com").unwrap();
        assert_eq!(
            (web.kind, web.source.as_str()),
            (Kind::Web, "https://example.com")
        );
    }

    #[test]
    fn a_picked_html_file_becomes_a_web_theme_with_its_path() {
        let dir = temp_dir("html");
        let page = dir.join("clock.html");
        std::fs::write(&page, "").unwrap();
        let resolved = resolve(page.to_str().unwrap()).unwrap();
        assert_eq!(resolved.kind, Kind::Web);
        let text = manifest("Clock", resolved.kind, &resolved.source, Fit::Cover, &[]);
        let loaded = LoadedTheme::parse(PathBuf::from("themes/clock"), &text).unwrap();
        let Wallpaper::Web(web) = loaded.theme.wallpaper else {
            panic!("expected a web theme")
        };
        assert_eq!(PathBuf::from(web.url), page);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn args_split_like_a_command_line_and_join_back() {
        let split = |text: &str| split_args(text).unwrap();
        assert_eq!(
            split("-parentHWND {hwnd} delayed"),
            ["-parentHWND", "{hwnd}", "delayed"]
        );
        assert_eq!(split("  a   b "), ["a", "b"]);
        assert_eq!(
            split(r#"--dir "C:\My Games\x" --flag"#),
            ["--dir", r"C:\My Games\x", "--flag"]
        );
        assert_eq!(split(r#"--name="a b""#), ["--name=a b"]);
        assert_eq!(split(r#""" x"#), ["", "x"]);
        assert!(split("").is_empty());
        assert!(split_args(r#"--dir "oops"#).is_err());

        let args = ["-x", "two words", "", "{hwnd}"];
        assert_eq!(split(&join_args(&args)), args);
    }

    #[test]
    fn the_unity_preset_matches_the_documented_arguments() {
        assert_eq!(join_args(UNITY_PLAYER.args), "-parentHWND {hwnd} delayed");
        assert!(ARG_PRESETS.contains(&UNITY_PLAYER));
    }

    #[test]
    fn unity_players_are_recognised_by_their_dll() {
        let dir = temp_dir("unity");
        let exe = dir.join("Game.exe");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(suggested_preset(&exe), None);
        std::fs::write(dir.join("UnityPlayer.dll"), "").unwrap();
        assert_eq!(suggested_preset(&exe), Some(UNITY_PLAYER));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn app_manifests_carry_their_arguments() {
        let args = split_args(&join_args(UNITY_PLAYER.args)).unwrap();
        let text = manifest("Game", Kind::App, r"C:\Games\Game.exe", Fit::Cover, &args);
        let loaded = LoadedTheme::parse(PathBuf::from("themes/game"), &text).unwrap();
        let Wallpaper::App(app) = loaded.theme.wallpaper else {
            panic!("expected an app theme")
        };
        assert_eq!(app.command, PathBuf::from(r"C:\Games\Game.exe"));
        assert_eq!(app.args, ["-parentHWND", "{hwnd}", "delayed"]);
        assert!(!manifest("Game", Kind::App, r"C:\g.exe", Fit::Cover, &[]).contains("args"));
    }

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mypaper-studio-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
