//! A theme is a folder: `theme.json` plus the assets it references.
//!
//! ```json
//! { "name": "Aurora", "wallpaper": { "kind": "shader", "source": "aurora.wgsl", "fps": 60 } }
//! ```
//!
//! Wallpaper kinds: `shader`, `image`, `slideshow`, `video`, `scene`, `web`, `app`, and
//! `stack` (a base plus layers; docs/wallpaper-builder.md and [`stack`]).
//! Relative asset paths resolve inside the theme folder and may not use
//! `..`; absolute paths (e.g. your own pictures or videos) are allowed.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub mod stack;
pub mod windows;

pub use stack::{
    Base, Blend, EffectLayer, EffectManifest, Environment, ImageLayer, Layer, LoadedEffect,
    ModelLayer, Param, ParamKind, StackWallpaper,
};

pub const MANIFEST: &str = "theme.json";

/// Used when no theme is given on the command line.
pub const DEFAULT_THEME: &str = "aurora";

/// Legacy and developer themes stay addressable by ID, outside the curated gallery.
pub const EXAMPLES_DIR: &str = "examples";

/// Finds a theme folder and returns its absolute path. `name` is a path
/// (absolute or relative to the current directory) or a bare theme name,
/// looked up in the user themes folder (`%APPDATA%\glaze\themes`) and
/// then in a `themes/` folder beside the executable or any parent, so a
/// double-clicked `target/debug/*.exe` finds the repo themes just like an
/// installed copy finds its own.
pub fn locate(name: Option<&str>) -> Result<PathBuf> {
    let user = glaze_config::user_themes_dir().ok();
    locate_from(
        name.unwrap_or(DEFAULT_THEME),
        user.as_deref(),
        &std::env::current_exe()?,
    )
}

/// Every folder that holds themes: the user folder (if it exists), then
/// the bundled `themes/` folder beside the executable or a parent.
pub fn theme_roots() -> Vec<PathBuf> {
    let user = glaze_config::user_themes_dir()
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
    // Also preserve saved relative/absolute paths such as `themes/webdemo` after the move.
    let moved = direct
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "themes"))
        .zip(direct.file_name())
        .map(|(parent, folder)| parent.join(EXAMPLES_DIR).join(folder));
    let in_user = user.map(|dir| dir.join(name));
    let beside_exe = exe.ancestors().skip(1).flat_map(|dir| {
        let themes = dir.join("themes");
        [
            themes.join(name),
            themes.join(EXAMPLES_DIR).join(name),
            dir.join(name),
        ]
    });
    let mut tried = Vec::new();
    for candidate in std::iter::once(direct)
        .chain(moved)
        .chain(in_user)
        .chain(beside_exe)
    {
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
    /// Per-wallpaper renderer preferences; user config overrides this layer.
    #[serde(default)]
    pub engine: glaze_config::RenderOptions,
    /// Exclude from discovery while still allowing an existing selection to resolve.
    #[serde(default)]
    pub hidden: bool,
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
    /// How the app styler restyles the XAML inside File Explorer and Settings while this theme
    /// is active (experimental; docs/windows-styling.md, "App styler"). `look.apps` layers over it.
    #[serde(default)]
    pub apps: Option<glaze_config::AppsStyle>,
}

/// A theme's look for a family of Glaze UI (widgets, bars): appearance
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
    /// How liquid glass looks for this family (with `style: "liquid"`); the color of its `tint`
    /// is `appearance.tint`.
    #[serde(default)]
    pub glass: Option<Liquid>,
}

/// Liquid glass knobs, each optional (layered like [`Appearance`]), with the bars' `look.glass`
/// names and ranges: `intensity` scales the whole effect, `refraction` how strongly the rim bends
/// what is behind it, `highlight` the specular rim, `tint` how much of the appearance's `tint`
/// color washes over the lens (all 0..=1), and `blur` a light blur of what is behind (DIPs,
/// 0..=32). Liquid glass wants little tint and no blur.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Liquid {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refraction: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur: Option<f32>,
}

impl Liquid {
    /// `self` layered over `base`: each field set here wins.
    pub fn over(&self, base: &Liquid) -> Liquid {
        Liquid {
            intensity: self.intensity.or(base.intensity),
            refraction: self.refraction.or(base.refraction),
            highlight: self.highlight.or(base.highlight),
            tint: self.tint.or(base.tint),
            blur: self.blur.or(base.blur),
        }
    }

    pub fn validate(&self) -> Result<()> {
        for (name, value, max) in [
            ("intensity", self.intensity, 1.0),
            ("refraction", self.refraction, 1.0),
            ("highlight", self.highlight, 1.0),
            ("tint", self.tint, 1.0),
            ("blur", self.blur, 32.0),
        ] {
            if let Some(v) = value
                && !(0.0..=max).contains(&v)
            {
                bail!("glass.{name} must be between 0 and {max}, got {v}");
            }
        }
        Ok(())
    }
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
    /// The widget design language (depth, glow, gloss). Widgets only; bars ignore it.
    pub style: Option<WidgetStyle>,
}

/// The design language of a widget (the kit's `data-style`): how its surface, borders and
/// highlights are lit. `Minimal` is the flat look every widget started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetStyle {
    Minimal,
    Depth,
    Neon,
    Paper,
    Aero,
    /// Clear glass that bends the wallpaper at its rim (the wallpaper engine draws the plate).
    Liquid,
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
            style: self.style.or(base.style),
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

// Colours and Windows styling live in `glaze_config` (the user's look layers over a
// theme's `windows` section, D2); re-exported so themes keep their names.
pub use glaze_config::{Backdrop, Color, Corners, Paint, WindowsStyle};

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
    /// A base plus a stack of effect, image and model layers (docs/wallpaper-builder.md).
    Stack(StackWallpaper),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebWallpaper {
    /// An `http(s)://` URL, or a path to a local `.html` file.
    pub url: String,
    /// Forward the mouse (move, buttons, wheel) to the page while the cursor
    /// is over the bare desktop. Off by default: no input is read at all.
    #[serde(default)]
    pub interactive: bool,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
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
    /// Texture at `binding`, filtering sampler at `binding + 1`, in group 0 (0 is paper).
    #[serde(default)]
    pub images: Vec<ShaderImage>,
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
pub struct ShaderImage {
    /// Omit for a generated static texture (prefilter or bake is then required).
    #[serde(default)]
    pub source: Option<PathBuf>,
    /// Odd bindings 1..=29 reserve one texture/sampler pair each.
    pub binding: u32,
    /// Optional theme-owned static image pass (compiled and drawn only at load).
    #[serde(default)]
    pub prefilter: Option<ShaderPrefilter>,
    /// A baked layer: the name of a `@fragment` entry point in the theme's own shader that the
    /// engine draws once per render size, in the frame's format, before the first frame of
    /// that size. It reads `paper` (time 0) and the other images, never a baked layer.
    /// Excludes `source` and `prefilter`.
    #[serde(default)]
    pub bake: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderPrefilter {
    pub source: PathBuf,
    /// Defaults to the input dimensions.
    #[serde(default)]
    pub size: Option<[u32; 2]>,
    #[serde(default)]
    pub format: ShaderImageFormat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum ShaderImageFormat {
    #[default]
    #[serde(rename = "bgra8unorm")]
    Bgra8Unorm,
    /// Data cache accessed with textureLoad (no filtering or mip chain).
    #[serde(rename = "rgba32float")]
    Rgba32Float,
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
    /// Drag with the left button over the bare desktop to orbit the camera.
    /// Off by default: no input is read at all.
    #[serde(default)]
    pub interactive: bool,
    /// What is drawn behind the models (`color` = `background`).
    #[serde(default)]
    pub environment: Environment,
    /// Linear exposure multiplier.
    #[serde(default = "one")]
    pub exposure: f32,
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
pub(crate) fn default_orbit() -> f32 {
    4.0
}
pub(crate) fn default_parallax() -> f32 {
    0.15
}
pub(crate) fn default_background() -> [f32; 3] {
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
        theme.engine.validate()?;
        match &theme.wallpaper {
            Wallpaper::Shader(s) => {
                check_path(&s.source)?;
                for (i, image) in s.images.iter().enumerate() {
                    if let Some(source) = &image.source {
                        check_path(source)?;
                    }
                    match &image.bake {
                        Some(_) if image.source.is_some() || image.prefilter.is_some() => {
                            bail!("a baked shader image has no source or prefilter")
                        }
                        Some(entry) if entry.is_empty() => bail!("bake needs an entry point"),
                        None if image.source.is_none() && image.prefilter.is_none() => {
                            bail!("shader image needs a source, prefilter or bake")
                        }
                        _ => {}
                    }
                    if let Some(effect) = &image.prefilter {
                        check_path(&effect.source)?;
                        if let Some(size) = effect.size
                            && !size.into_iter().all(|n| (1..=16384).contains(&n))
                        {
                            bail!("prefilter dimensions must be between 1 and 16384");
                        }
                    }
                    if image.binding == 0
                        || image.binding > 29
                        || image.binding % 2 == 0
                        || s.images[..i]
                            .iter()
                            .any(|other| other.binding == image.binding)
                    {
                        bail!("shader image bindings must be unique odd numbers from 1 to 29");
                    }
                }
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
            Wallpaper::Stack(s) => s.validate()?,
        }
        if theme.windows.as_ref().and_then(|w| w.cursors.as_deref()) == Some("") {
            bail!("windows.cursors must name a cursor set or folder");
        }
        for skin in [&theme.widgets, &theme.bars].into_iter().flatten() {
            if let Some(appearance) = &skin.appearance {
                appearance.validate()?;
            }
            if let Some(css) = &skin.css {
                check_path(css)?;
            }
            if let Some(glass) = &skin.glass {
                glass.validate()?;
            }
        }
        Ok(Self { dir, theme })
    }

    /// Resolves an asset path: relative to the theme folder, or absolute.
    pub fn asset(&self, path: &Path) -> PathBuf {
        self.dir.join(path)
    }
}

pub(crate) fn check_fps(fps: u32) -> Result<()> {
    if !(1..=240).contains(&fps) {
        bail!("wallpaper.fps must be between 1 and 240, got {fps}");
    }
    Ok(())
}

pub(crate) fn check_scale(scale: f32) -> Result<()> {
    if !(0.25..=1.0).contains(&scale) {
        bail!("wallpaper.scale must be between 0.25 and 1, got {scale}");
    }
    Ok(())
}

/// D2's one-time migration: Windows styling used to live only in a theme file, so a user who
/// styled Windows did it in their own copy of a theme. If the user's look has no Windows layer
/// yet and the active theme is one of their own with a `windows` section, that section becomes
/// their layer (`look.windows`). Nothing on screen changes; the theme file is left as it is
/// (now the layer below). Returns whether `config` changed.
pub fn migrate_windows(config: &mut glaze_config::Config) -> bool {
    match glaze_config::user_themes_dir() {
        Ok(root) => migrate_windows_in(config, &root),
        Err(_) => false,
    }
}

/// [`migrate_windows`] with the user themes folder given.
fn migrate_windows_in(config: &mut glaze_config::Config, user_root: &Path) -> bool {
    if !config.look.windows.is_empty() {
        return false;
    }
    let Ok(dir) = locate(Some(&config.theme)) else {
        return false;
    };
    let canonical = |p: &Path| std::fs::canonicalize(p).ok();
    let own = canonical(&dir).and_then(|d| d.parent().map(Path::to_path_buf));
    if own.is_none() || own != canonical(user_root) {
        return false;
    }
    match LoadedTheme::load(&dir).map(|t| t.theme.windows) {
        Ok(Some(style)) if style != WindowsStyle::default() => {
            config.look.windows = glaze_config::look::WindowsLayer::adopt(&style);
            true
        }
        _ => false,
    }
}

/// Relative paths may not climb out of the theme folder.
pub(crate) fn check_path(path: &Path) -> Result<()> {
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
                images: Vec::new(),
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
    fn shader_images_reserve_valid_texture_sampler_pairs() {
        let image = r#"{"kind":"shader","source":"rain.wgsl","scale":0.65,"images":[{"source":"media/city.jpg","binding":1}]}"#;
        let Wallpaper::Shader(shader) = wallpaper(image).unwrap() else {
            panic!("shader");
        };
        assert_eq!(shader.scale, 0.65);
        assert_eq!(
            shader.images[0].source.as_deref(),
            Some(Path::new("media/city.jpg"))
        );
        for binding in [0, 2, 30] {
            assert!(
                wallpaper(&image.replace("\"binding\":1", &format!("\"binding\":{binding}")))
                    .is_err()
            );
        }
        assert!(wallpaper(&image.replace("media/city.jpg", "../city.jpg")).is_err());
        assert!(
            wallpaper(&image.replace(
                "\"binding\":1}",
                "\"binding\":1},{\"source\":\"b.jpg\",\"binding\":1}"
            ))
            .is_err()
        );
        let generated = r#"{"kind":"shader","source":"rain.wgsl","images":[{"binding":3,"prefilter":{"source":"profile.wgsl","size":[8192,1],"format":"rgba32float"}}]}"#;
        assert!(wallpaper(generated).is_ok());
        for invalid in [
            generated.replace("8192,1", "8192,0"),
            generated.replace("8192,1", "16385,1"),
            generated.replace("rgba32float", "rgba16float"),
            generated.replace("profile.wgsl", "../profile.wgsl"),
        ] {
            assert!(wallpaper(&invalid).is_err());
        }
        assert!(
            wallpaper(r#"{"kind":"shader","source":"rain.wgsl","images":[{"binding":1}]}"#)
                .is_err()
        );
        let baked =
            r#"{"kind":"shader","source":"rain.wgsl","images":[{"binding":5,"bake":"still"}]}"#;
        let Wallpaper::Shader(shader) = wallpaper(baked).unwrap() else {
            panic!("shader");
        };
        assert_eq!(shader.images[0].bake.as_deref(), Some("still"));
        for invalid in [
            baked.replace("\"bake\"", "\"source\":\"a.jpg\",\"bake\""),
            baked.replace("\"still\"", "\"\""),
        ] {
            assert!(wallpaper(&invalid).is_err(), "{invalid}");
        }
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
    fn widget_style_is_lowercase_layers_and_rejects_unknown_variants() {
        let appearance = |json: &str| serde_json::from_str::<Appearance>(json);
        for (name, style) in [
            ("minimal", WidgetStyle::Minimal),
            ("depth", WidgetStyle::Depth),
            ("neon", WidgetStyle::Neon),
            ("paper", WidgetStyle::Paper),
            ("aero", WidgetStyle::Aero),
            ("liquid", WidgetStyle::Liquid),
        ] {
            let parsed = appearance(&format!(r#"{{ "style": "{name}" }}"#)).unwrap();
            assert_eq!(parsed.style, Some(style));
            assert_eq!(serde_json::to_value(&parsed).unwrap()["style"], name);
        }
        assert!(
            appearance(r#"{ "style": "Neon" }"#).is_err(),
            "case matters"
        );
        assert!(appearance(r#"{ "style": "glossy" }"#).is_err());

        let theme = Appearance {
            style: Some(WidgetStyle::Depth),
            ..Default::default()
        };
        let none = Appearance::default();
        assert_eq!(
            none.over(&theme).style,
            Some(WidgetStyle::Depth),
            "unset keeps the layer below"
        );
        let user = Appearance {
            style: Some(WidgetStyle::Aero),
            ..Default::default()
        };
        assert_eq!(
            user.over(&theme).style,
            Some(WidgetStyle::Aero),
            "the upper layer wins"
        );
    }

    #[test]
    fn liquid_knobs_layer_and_are_range_checked() {
        let skin = r##"{ "name": "A", "wallpaper": { "kind": "image", "source": "x.jpg" },
            "widgets": { "glass": { "refraction": 0.4, "tint": 0.1, "blur": 2 } } }"##;
        let glass = parse(skin).unwrap().theme.widgets.unwrap().glass.unwrap();
        let base = Liquid {
            intensity: Some(1.0),
            refraction: Some(0.6),
            ..Default::default()
        };
        let merged = glass.over(&base);
        assert_eq!(merged.intensity, Some(1.0), "unset keeps the layer below");
        assert_eq!(
            (merged.refraction, merged.tint, merged.blur),
            (Some(0.4), Some(0.1), Some(2.0))
        );
        assert!(parse(&skin.replace("\"blur\": 2", "\"blur\": 40")).is_err());
        assert!(parse(&skin.replace("\"tint\": 0.1", "\"tint\": 1.5")).is_err());
        assert!(parse(&skin.replace("\"tint\": 0.1", "\"sheen\": 1")).is_err());
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
    fn interactive_is_opt_in_for_web_and_scene_only() {
        let interactive = |json: &str| match wallpaper(json).unwrap() {
            Wallpaper::Web(w) => w.interactive,
            Wallpaper::Scene(s) => s.interactive,
            other => panic!("unexpected {other:?}"),
        };
        assert!(!interactive(
            r#"{ "kind": "web", "url": "https://example.com" }"#
        ));
        assert!(interactive(
            r#"{ "kind": "web", "url": "a.html", "interactive": true }"#
        ));
        assert!(!interactive(r#"{ "kind": "scene", "source": "a.glb" }"#));
        assert!(interactive(
            r#"{ "kind": "scene", "source": "a.glb", "interactive": true }"#
        ));
        // Kinds that cannot use input reject the field, like any other typo.
        assert!(
            wallpaper(r#"{ "kind": "shader", "source": "a.wgsl", "interactive": true }"#).is_err()
        );
        assert!(wallpaper(r#"{ "kind": "web", "url": "a.html", "interactive": "yes" }"#).is_err());
    }

    #[test]
    fn windows_cursors_names_a_set_or_folder() {
        let cursors = |value: &str| {
            let json = format!(
                r#"{{ "name": "A", "wallpaper": {{ "kind": "image", "source": "x.jpg" }},
                    "windows": {{ "cursors": {value} }} }}"#
            );
            parse(&json).map(|t| t.theme.windows.unwrap().cursors)
        };
        assert_eq!(
            cursors(r#""glaze-dark""#).unwrap().as_deref(),
            Some("glaze-dark")
        );
        assert_eq!(
            cursors(r#""cursors\\mine""#).unwrap().as_deref(),
            Some(r"cursors\mine")
        );
        assert!(cursors(r#""""#).is_err());
        assert!(cursors("3").is_err());
        let unset = r#"{ "name": "A", "wallpaper": { "kind": "image", "source": "x.jpg" }, "windows": {} }"#;
        assert_eq!(parse(unset).unwrap().theme.windows.unwrap().cursors, None);
    }

    #[test]
    fn locates_themes_from_a_nested_exe() {
        let root = std::env::temp_dir().join(format!("glaze-locate-{}", std::process::id()));
        let theme = root.join("themes").join("aurora");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join(MANIFEST), "{}").unwrap();
        let exe = root.join("target").join("debug").join("glaze-core.exe");

        let found = locate_from("aurora", None, &exe).unwrap();
        assert_eq!(found, theme);
        assert!(locate_from("nope", None, &exe).is_err());

        for id in [
            "webdemo",
            "interactive",
            "aurora",
            "midnight",
            "earth-orbit",
            "islands",
            "photo",
            "gallery",
        ] {
            let example = root.join("themes").join(EXAMPLES_DIR).join(id);
            std::fs::create_dir_all(&example).unwrap();
            std::fs::write(example.join(MANIFEST), "{}").unwrap();
            // A top-level theme still wins when both locations exist.
            let expected = if id == "aurora" { &theme } else { &example };
            assert_eq!(locate_from(id, None, &exe).unwrap(), *expected);
            assert_eq!(
                locate_from(root.join("themes").join(id).to_str().unwrap(), None, &exe).unwrap(),
                *expected,
            );
        }
        let user = root.join("user");
        let user_web = user.join("webdemo");
        std::fs::create_dir_all(&user_web).unwrap();
        std::fs::write(user_web.join(MANIFEST), "{}").unwrap();
        assert_eq!(locate_from("webdemo", Some(&user), &exe).unwrap(), user_web);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hidden_defaults_to_false_and_accepts_only_booleans() {
        let json = r#"{"name":"A","wallpaper":{"kind":"image","source":"a.jpg"}}"#;
        assert!(!parse(json).unwrap().theme.hidden);
        assert!(
            parse(&json.replacen('{', r#"{"hidden":true,"#, 1))
                .unwrap()
                .theme
                .hidden
        );
        assert!(parse(&json.replacen('{', r#"{"hidden":"true","#, 1)).is_err());
    }

    fn bundled_dirs(root: &Path) -> Vec<PathBuf> {
        [root.to_owned(), root.join(EXAMPLES_DIR)]
            .into_iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap().map(Result::unwrap))
            .map(|entry| entry.path())
            .filter(|dir| dir.join(MANIFEST).is_file())
            .collect()
    }

    #[test]
    fn bundled_manifests_and_referenced_assets_are_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let mut visible = 0;
        for dir in bundled_dirs(&root) {
            let loaded = LoadedTheme::load(&dir).unwrap();
            let exists = |path: &Path| {
                assert!(
                    loaded.asset(path).exists(),
                    "missing {}",
                    loaded.asset(path).display()
                );
            };
            match &loaded.theme.wallpaper {
                Wallpaper::Shader(w) => {
                    exists(&w.source);
                    for image in &w.images {
                        if let Some(source) = &image.source {
                            exists(source);
                        }
                        if let Some(effect) = &image.prefilter {
                            exists(&effect.source);
                        }
                    }
                }
                Wallpaper::Image(w) => exists(&w.source),
                Wallpaper::Video(w) => exists(&w.source),
                Wallpaper::Scene(w) => exists(&w.source),
                Wallpaper::Web(w) if !w.url.starts_with("http") => exists(Path::new(&w.url)),
                Wallpaper::App(w) => exists(&w.command),
                Wallpaper::Slideshow(w) => {
                    exists(&w.folder);
                    assert!(std::fs::read_dir(loaded.asset(&w.folder)).unwrap().count() >= 2);
                }
                Wallpaper::Web(_) => {}
                Wallpaper::Stack(_) => {}
            }
            for skin in [&loaded.theme.widgets, &loaded.theme.bars]
                .into_iter()
                .flatten()
            {
                if let Some(css) = &skin.css {
                    exists(css);
                }
            }
            if !loaded.theme.hidden {
                visible += 1;
                exists(Path::new("preview.jpg"));
            }
        }
        assert!(
            (15..=25).contains(&visible),
            "{visible} visible curated themes"
        );
    }

    #[test]
    fn every_bundled_file_has_a_credit_entry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let credits = std::fs::read_to_string(root.join("CREDITS.md")).unwrap();
        let catalog: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("catalog.json")).unwrap())
                .unwrap();
        let paths: std::collections::HashSet<_> = catalog["themes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|theme| theme["assets"].as_array().unwrap())
            .chain(catalog["legacy_assets"].as_array().unwrap())
            .chain(catalog["shared_assets"].as_array().unwrap())
            .map(|asset| {
                let path = asset["path"].as_str().unwrap();
                for field in [
                    "author",
                    "source",
                    "license",
                    "license_url",
                    "changes",
                    "sha256",
                ] {
                    assert!(
                        !asset[field].as_str().unwrap_or_default().is_empty(),
                        "{path}: missing {field}"
                    );
                }
                assert!(credits.contains(path), "{path} is missing from CREDITS.md");
                assert_eq!(
                    std::fs::metadata(root.join(path)).unwrap().len(),
                    asset["bytes"].as_u64().unwrap()
                );
                path
            })
            .collect();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if !["CREDITS.md", "catalog.json"].contains(&relative.as_str()) {
                    assert!(
                        paths.contains(relative.as_str()),
                        "{relative} has no attribution record"
                    );
                }
            }
        }
    }

    #[test]
    fn a_user_themes_windows_styling_becomes_the_users_once() {
        let root = std::env::temp_dir().join(format!("glaze-theme-d2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mine = root.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(
            mine.join(MANIFEST),
            r##"{ "name": "Mine", "wallpaper": { "kind": "image", "source": "a.png" },
                 "windows": { "corners": "square", "dark_mode": true } }"##,
        )
        .unwrap();
        let mut config = glaze_config::Config {
            theme: mine.display().to_string(),
            ..Default::default()
        };
        assert!(migrate_windows_in(&mut config, &root));
        let style = config.look.windows_style(None).unwrap();
        assert_eq!(style.corners, Some(Corners::Square));
        assert_eq!(style.dark_mode, Some(true));
        assert!(!migrate_windows_in(&mut config, &root), "only once");
        // A theme outside the user's folder (a built-in) is never adopted.
        let mut builtin = glaze_config::Config {
            theme: mine.display().to_string(),
            ..Default::default()
        };
        assert!(!migrate_windows_in(&mut builtin, &root.join("elsewhere")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
