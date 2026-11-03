//! A theme is a folder: `theme.json` plus the assets it references.
//!
//! ```json
//! { "name": "Aurora", "wallpaper": { "kind": "shader", "source": "aurora.wgsl", "fps": 60 } }
//! ```
//!
//! Wallpaper kinds: `shader`, `image`, `slideshow`, `video`, `scene`.
//! Relative asset paths resolve inside the theme folder and may not use
//! `..`; absolute paths (e.g. your own pictures or videos) are allowed.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "theme.json";

/// Used when no theme is given on the command line.
pub const DEFAULT_THEME: &str = "aurora";

/// Finds a theme folder and returns its absolute path. `name` is a path
/// (absolute or relative to the current directory) or a bare theme name,
/// looked up in the user themes folder (`%APPDATA%\mypaper\themes`) and
/// then in a `themes/` folder beside the executable or any parent, so a
/// double-clicked `target/debug/*.exe` finds the repo themes just like an
/// installed copy finds its own.
pub fn locate(name: Option<&str>) -> Result<PathBuf> {
    let user = mypaper_config::user_themes_dir().ok();
    locate_from(
        name.unwrap_or(DEFAULT_THEME),
        user.as_deref(),
        &std::env::current_exe()?,
    )
}

/// Every folder that holds themes: the user folder (if it exists), then
/// the bundled `themes/` folder beside the executable or a parent.
pub fn theme_roots() -> Vec<PathBuf> {
    let user = mypaper_config::user_themes_dir()
        .ok()
        .filter(|dir| dir.is_dir());
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors()
            .skip(1)
            .map(|dir| dir.join("themes"))
            .find(|dir| dir.is_dir())
    });
    user.into_iter().chain(bundled).collect()
}

fn locate_from(name: &str, user: Option<&Path>, exe: &Path) -> Result<PathBuf> {
    let direct = PathBuf::from(name);
    let in_user = user.map(|dir| dir.join(name));
    let beside_exe = exe
        .ancestors()
        .skip(1)
        .flat_map(|dir| [dir.join("themes").join(name), dir.join(name)]);
    let mut tried = Vec::new();
    for candidate in std::iter::once(direct).chain(in_user).chain(beside_exe) {
        if candidate.join(MANIFEST).is_file() {
            return Ok(std::path::absolute(&candidate)?);
        }
        tried.push(candidate.display().to_string());
    }
    bail!(
        "theme {name:?} not found; looked in:\n  {}",
        tried.join("\n  ")
    )
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub wallpaper: Wallpaper,
    /// Native window styling (accent, borders, corners, backdrop) applied
    /// to allowlisted Windows windows while this theme is active.
    #[serde(default)]
    pub windows: Option<WindowsStyle>,
    /// How desktop widgets look while this theme is active (docs/widgets.md).
    #[serde(default)]
    pub widgets: Option<Skin>,
    /// How the top bar and dock look while this theme is active (docs/bars.md).
    #[serde(default)]
    pub bars: Option<Skin>,
}

/// A theme's look for a family of mypaper UI (widgets, bars): appearance
/// values plus an optional style sheet loaded after the built-in styles.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Skin {
    #[serde(default)]
    pub appearance: Option<Appearance>,
    /// CSS, relative to the theme folder. Use the documented CSS variables
    /// and `data-*` attributes; it applies on top of `appearance`.
    #[serde(default)]
    pub css: Option<PathBuf>,
}

/// The look of a widget or bar. Every field is optional; layers are merged
/// with [`Appearance::over`] (built-in defaults < theme < user < instance).
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// `none`: no background at all, text sits on the wallpaper.
    pub surface: Option<Surface>,
    /// The surface color.
    pub tint: Option<Color>,
    /// Surface opacity, 0..=1.
    pub opacity: Option<f32>,
    /// Background blur in px, 0..=64 (glass only).
    pub blur: Option<f32>,
    /// Corner radius in px, 0..=48.
    pub radius: Option<f32>,
    /// A hairline border around the surface.
    pub border: Option<bool>,
    /// Light text for dark wallpapers, dark text for light ones.
    pub text: Option<TextTone>,
    /// Highlight color, or `auto` (from the wallpaper palette).
    pub accent: Option<Paint>,
    /// A CSS font-family list.
    pub font: Option<String>,
    /// Content scale, 0.5..=2.
    pub scale: Option<f32>,
    /// A soft text shadow, for legibility without a surface.
    pub shadow: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    None,
    Glass,
    Solid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextTone {
    Light,
    Dark,
}

impl Appearance {
    /// `self` layered over `base`: each field set here wins.
    pub fn over(&self, base: &Appearance) -> Appearance {
        Appearance {
            surface: self.surface.or(base.surface),
            tint: self.tint.or(base.tint),
            opacity: self.opacity.or(base.opacity),
            blur: self.blur.or(base.blur),
            radius: self.radius.or(base.radius),
            border: self.border.or(base.border),
            text: self.text.or(base.text),
            accent: self.accent.or(base.accent),
            font: self.font.clone().or_else(|| base.font.clone()),
            scale: self.scale.or(base.scale),
            shadow: self.shadow.or(base.shadow),
        }
    }

    /// Range checks, for theme and layout files.
    pub fn validate(&self) -> Result<()> {
        let check =
            |name: &str, value: Option<f32>, range: std::ops::RangeInclusive<f32>| match value {
                Some(v) if !range.contains(&v) => bail!(
                    "appearance.{name} must be between {} and {}, got {v}",
                    range.start(),
                    range.end()
                ),
                _ => Ok(()),
            };
        check("opacity", self.opacity, 0.0..=1.0)?;
        check("blur", self.blur, 0.0..=64.0)?;
        check("radius", self.radius, 0.0..=48.0)?;
        check("scale", self.scale, 0.5..=2.0)
    }
}

/// An sRGB color, written `"#rrggbb"` in theme files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub [u8; 3]);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("expected \"#rrggbb\", got {text:?}")))
    }
}

impl Color {
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
        let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some(Self([channel(0)?, channel(2)?, channel(4)?]))
    }

    /// `"#rrggbb"`.
    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

impl Serialize for Paint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Paint::Auto => serializer.serialize_str("auto"),
            Paint::Color(color) => color.serialize(serializer),
        }
    }
}

/// A color, or one derived from the wallpaper (`"auto"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// Picked from the wallpaper's palette.
    Auto,
    Color(Color),
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text == "auto" {
            return Ok(Paint::Auto);
        }
        Color::parse(&text).map(Paint::Color).ok_or_else(|| {
            serde::de::Error::custom(format!("expected \"auto\" or \"#rrggbb\", got {text:?}"))
        })
    }
}

/// Native styling for windows (DWM) and the system accent. Every field is
/// optional: unset means "leave Windows' own setting alone".
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsStyle {
    /// System accent color (taskbar, Start, highlights).
    pub accent: Option<Paint>,
    /// Window border color.
    pub border: Option<Paint>,
    /// Title bar background color.
    pub caption: Option<Color>,
    /// Title bar text color.
    pub caption_text: Option<Color>,
    pub corners: Option<Corners>,
    pub backdrop: Option<Backdrop>,
    /// Force dark (`true`) or light (`false`) mode for apps and the system.
    pub dark_mode: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    Default,
    Round,
    Small,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    Auto,
    None,
    Mica,
    Acrylic,
    Tabbed,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Wallpaper {
    /// A WGSL fragment shader defining `fn wallpaper(frag: vec2f) -> vec4f`.
    Shader(ShaderWallpaper),
    /// One still image. Rendered once, then costs nothing.
    Image(ImageWallpaper),
    /// Images from a folder, cross-faded on a timer.
    Slideshow(SlideshowWallpaper),
    /// A looping, hardware-decoded video.
    Video(VideoWallpaper),
    /// A 3D scene (glTF / GLB).
    Scene(SceneWallpaper),
    /// A web page (WebView2), suspended while hidden.
    Web(WebWallpaper),
    /// An external program rendering into our desktop window, e.g. a
    /// Unity player (`-parentHWND`).
    App(AppWallpaper),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebWallpaper {
    /// An `http(s)://` URL, or a path to a local `.html` file.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppWallpaper {
    /// The executable (relative to the theme folder, or absolute).
    pub command: PathBuf,
    /// Arguments; `{hwnd}` is replaced with the decimal handle of the window
    /// the app should render into. Unity: `["-parentHWND", "{hwnd}", "delayed"]`.
    #[serde(default)]
    pub args: Vec<String>,
}

/// How a source that has its own size fills a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Fill the screen, cropping the overflow (like CSS `cover`).
    #[default]
    Cover,
    /// Show everything, letterboxed.
    Contain,
    /// Fill the screen, ignoring aspect ratio.
    Stretch,
    /// Pixel-for-pixel, centered.
    Center,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderWallpaper {
    pub source: PathBuf,
    /// Frame cap. Lower = lighter; slow shaders look identical at 30.
    #[serde(default = "default_fps")]
    pub fps: u32,
    /// Render resolution relative to the screen (0.25..=1), upscaled after.
    /// Smooth shaders look the same at 0.5 for a quarter of the GPU work.
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageWallpaper {
    pub source: PathBuf,
    #[serde(default)]
    pub fit: Fit,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideshowWallpaper {
    /// Folder of images (png, jpg, bmp, gif, tiff, webp, heic: whatever WIC decodes).
    pub folder: PathBuf,
    /// Seconds each image stays up.
    #[serde(default = "default_interval")]
    pub interval: f32,
    /// Cross-fade duration in seconds (0 = cut).
    #[serde(default = "default_transition")]
    pub transition: f32,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub fit: Fit,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoWallpaper {
    pub source: PathBuf,
    #[serde(default)]
    pub fit: Fit,
    /// Playback cap; frames above it are dropped, not decoded-then-wasted.
    #[serde(default = "default_video_fps")]
    pub fps: u32,
    /// Playback speed multiplier.
    #[serde(default = "one")]
    pub speed: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneWallpaper {
    /// A `.glb` or `.gltf` file.
    pub source: PathBuf,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "one")]
    pub scale: f32,
    /// Camera orbit speed in degrees per second (0 = still camera).
    #[serde(default = "default_orbit")]
    pub orbit: f32,
    /// How far the camera leans toward the mouse (0 = off).
    #[serde(default = "default_parallax")]
    pub parallax: f32,
    /// Background / clear color, as linear RGB 0..1.
    #[serde(default = "default_background")]
    pub background: [f32; 3],
}

fn default_fps() -> u32 {
    60
}
fn default_video_fps() -> u32 {
    30
}
fn one() -> f32 {
    1.0
}
fn default_interval() -> f32 {
    300.0
}
fn default_transition() -> f32 {
    1.5
}
fn default_orbit() -> f32 {
    4.0
}
fn default_parallax() -> f32 {
    0.15
}
fn default_background() -> [f32; 3] {
    [0.02, 0.025, 0.04]
}

/// A parsed theme together with the folder it was loaded from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTheme {
    pub dir: PathBuf,
    pub theme: Theme,
}

impl LoadedTheme {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let manifest = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("reading {}", manifest.display()))?;
        Self::parse(dir, &text).with_context(|| format!("in {}", manifest.display()))
    }

    pub fn parse(dir: PathBuf, json: &str) -> Result<Self> {
        let theme: Theme = serde_json::from_str(json)?;
        match &theme.wallpaper {
            Wallpaper::Shader(s) => {
                check_path(&s.source)?;
                check_fps(s.fps)?;
                check_scale(s.scale)?;
            }
            Wallpaper::Image(i) => check_path(&i.source)?,
            Wallpaper::Slideshow(s) => {
                check_path(&s.folder)?;
                if s.interval.is_nan() || s.interval < 1.0 {
                    bail!("wallpaper.interval must be at least 1 second");
                }
                if !(0.0..=s.interval).contains(&s.transition) {
                    bail!("wallpaper.transition must be between 0 and the interval");
                }
            }
            Wallpaper::Video(v) => {
                check_path(&v.source)?;
                check_fps(v.fps)?;
                if !(0.1..=4.0).contains(&v.speed) {
                    bail!("wallpaper.speed must be between 0.1 and 4");
                }
            }
            Wallpaper::Scene(s) => {
                check_path(&s.source)?;
                check_fps(s.fps)?;
                check_scale(s.scale)?;
            }
            Wallpaper::Web(w) => {
                let remote = w.url.starts_with("https://") || w.url.starts_with("http://");
                if !remote {
                    check_path(Path::new(&w.url))?;
                }
            }
            Wallpaper::App(a) => check_path(&a.command)?,
        }
        for skin in [&theme.widgets, &theme.bars].into_iter().flatten() {
            if let Some(appearance) = &skin.appearance {
                appearance.validate()?;
            }
            if let Some(css) = &skin.css {
                check_path(css)?;
            }
        }
        Ok(Self { dir, theme })
    }

    /// Resolves an asset path: relative to the theme folder, or absolute.
    pub fn asset(&self, path: &Path) -> PathBuf {
        self.dir.join(path)
    }
}

fn check_fps(fps: u32) -> Result<()> {
    if !(1..=240).contains(&fps) {
        bail!("wallpaper.fps must be between 1 and 240, got {fps}");
    }
    Ok(())
}

fn check_scale(scale: f32) -> Result<()> {
    if !(0.25..=1.0).contains(&scale) {
        bail!("wallpaper.scale must be between 0.25 and 1, got {scale}");
    }
    Ok(())
}

/// Relative paths may not climb out of the theme folder.
fn check_path(path: &Path) -> Result<()> {
    if path.is_relative() && path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!(
            "asset path {} must stay inside the theme folder (or be absolute)",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<LoadedTheme> {
        LoadedTheme::parse(PathBuf::from("themes/t"), json)
    }

    fn wallpaper(json: &str) -> Result<Wallpaper> {
        parse(&format!(r#"{{ "name": "A", "wallpaper": {json} }}"#)).map(|t| t.theme.wallpaper)
    }

    #[test]
    fn parses_every_kind_with_defaults() {
        assert_eq!(
            wallpaper(r#"{ "kind": "shader", "source": "a.wgsl" }"#).unwrap(),
            Wallpaper::Shader(ShaderWallpaper {
                source: "a.wgsl".into(),
                fps: 60,
                scale: 1.0
            })
        );
        assert_eq!(
            wallpaper(r#"{ "kind": "image", "source": "a.jpg", "fit": "contain" }"#).unwrap(),
            Wallpaper::Image(ImageWallpaper {
                source: "a.jpg".into(),
                fit: Fit::Contain
            })
        );
        assert!(matches!(
            wallpaper(r#"{ "kind": "slideshow", "folder": "pics" }"#).unwrap(),
            Wallpaper::Slideshow(SlideshowWallpaper {
                interval: 300.0,
                shuffle: false,
                ..
            })
        ));
        assert!(matches!(
            wallpaper(r#"{ "kind": "video", "source": "a.mp4" }"#).unwrap(),
            Wallpaper::Video(VideoWallpaper {
                fps: 30,
                fit: Fit::Cover,
                ..
            })
        ));
        assert!(matches!(
            wallpaper(r#"{ "kind": "scene", "source": "a.glb", "scale": 0.5 }"#).unwrap(),
            Wallpaper::Scene(SceneWallpaper {
                scale: 0.5,
                fps: 60,
                ..
            })
        ));
    }

    #[test]
    fn rejects_typos_unknown_kinds_and_bad_ranges() {
        assert!(wallpaper(r#"{ "kind": "shader", "sorce": "a.wgsl" }"#).is_err());
        assert!(wallpaper(r#"{ "kind": "hologram" }"#).is_err());
        assert!(wallpaper(r#"{ "kind": "shader", "source": "a.wgsl", "fps": 0 }"#).is_err());
        assert!(wallpaper(r#"{ "kind": "shader", "source": "a.wgsl", "scale": 2 }"#).is_err());
        assert!(
            wallpaper(r#"{ "kind": "slideshow", "folder": "p", "interval": 5, "transition": 9 }"#)
                .is_err()
        );
        assert!(wallpaper(r#"{ "kind": "image", "source": "a.jpg", "fit": "zoom" }"#).is_err());
    }

    #[test]
    fn relative_paths_stay_inside_but_absolute_paths_are_allowed() {
        assert!(wallpaper(r#"{ "kind": "image", "source": "../x.jpg" }"#).is_err());
        assert!(wallpaper(r#"{ "kind": "image", "source": "C:\\Pictures\\x.jpg" }"#).is_ok());
        let t = parse(r#"{ "name": "A", "wallpaper": { "kind": "image", "source": "x.jpg" } }"#)
            .unwrap();
        assert_eq!(t.asset(Path::new("x.jpg")), PathBuf::from("themes/t/x.jpg"));
    }

    #[test]
    fn skins_layer_and_validate_and_colors_round_trip() {
        let json = r##"{ "name": "A", "wallpaper": { "kind": "image", "source": "x.jpg" },
            "widgets": { "appearance": { "surface": "none", "text": "light", "opacity": 0.4 }, "css": "w.css" },
            "bars": { "appearance": { "surface": "glass", "tint": "#101820", "accent": "auto" } } }"##;
        let theme = parse(json).unwrap().theme;
        let widgets = theme.widgets.unwrap().appearance.unwrap();
        let user = Appearance {
            surface: Some(Surface::Glass),
            blur: Some(30.0),
            ..Default::default()
        };
        let merged = user.over(&widgets);
        assert_eq!(merged.surface, Some(Surface::Glass)); // the upper layer wins
        assert_eq!(merged.opacity, Some(0.4)); // unset above: kept from below
        assert_eq!(merged.blur, Some(30.0));

        let bars = theme.bars.unwrap().appearance.unwrap();
        let encoded = serde_json::to_value(&bars).unwrap();
        assert_eq!(encoded["tint"], "#101820");
        assert_eq!(encoded["accent"], "auto");
        assert_eq!(serde_json::from_value::<Appearance>(encoded).unwrap(), bars);

        assert!(parse(&json.replace("0.4", "1.5")).is_err()); // opacity out of range
        assert!(parse(&json.replace("w.css", "../w.css")).is_err());
    }

    #[test]
    fn parses_web_app_and_windows_styling() {
        assert!(wallpaper(r#"{ "kind": "web", "url": "https://example.com" }"#).is_ok());
        assert!(wallpaper(r#"{ "kind": "web", "url": "../page.html" }"#).is_err());
        assert!(matches!(
            wallpaper(r#"{ "kind": "app", "command": "game.exe", "args": ["-parentHWND", "{hwnd}"] }"#).unwrap(),
            Wallpaper::App(AppWallpaper { args, .. }) if args.len() == 2
        ));

        let json = r##"{ "name": "A", "wallpaper": { "kind": "image", "source": "x.jpg" },
            "windows": { "accent": "auto", "border": "#ff8800", "corners": "small", "backdrop": "mica" } }"##;
        let style = parse(json).unwrap().theme.windows.unwrap();
        assert_eq!(style.accent, Some(Paint::Auto));
        assert_eq!(style.border, Some(Paint::Color(Color([0xff, 0x88, 0x00]))));
        assert_eq!(style.corners, Some(Corners::Small));
        assert_eq!(style.dark_mode, None);
        assert!(parse(&json.replace("#ff8800", "orange")).is_err());
    }

    #[test]
    fn locates_themes_from_a_nested_exe() {
        let root = std::env::temp_dir().join(format!("mypaper-locate-{}", std::process::id()));
        let theme = root.join("themes").join("aurora");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join(MANIFEST), "{}").unwrap();
        let exe = root.join("target").join("debug").join("mypaper-core.exe");

        let found = locate_from("aurora", None, &exe).unwrap();
        assert_eq!(found, theme);
        assert!(locate_from("nope", None, &exe).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
