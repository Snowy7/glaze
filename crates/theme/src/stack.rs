//! `stack` wallpapers and effect packages: the format half of docs/wallpaper-builder.md.
//!
//! A stack is a base (image, video, color, gradient, shader, glsl, scene) plus layers (effect,
//! image, model). Effects are folders with an `effect.json` ([`EffectManifest`]) and WGSL passes;
//! [`pass_wgsl`] builds exactly the module the engine compiles for one pass, so Studio and the
//! effects library validate the same code (with the `check` feature, [`check::wgsl`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Color, Fit};

/// `effects/prelude.wgsl`, the start of every effect pass.
pub const EFFECT_PRELUDE: &str = include_str!("../../../effects/prelude.wgsl");

/// The manifest file inside an effect folder.
pub const EFFECT_MANIFEST: &str = "effect.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackWallpaper {
    /// Frame cap while something moves.
    #[serde(default = "default_stack_fps")]
    pub fps: u32,
    /// Render scale while something moves (0.25..=1); a still stack renders at full size.
    #[serde(default = "one")]
    pub scale: f32,
    pub base: Base,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

fn default_stack_fps() -> u32 {
    30
}
fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn center() -> [f32; 2] {
    [0.5, 0.5]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Base {
    Image(ImageBase),
    Video(VideoBase),
    Color(ColorBase),
    Gradient(GradientBase),
    Shader(ShaderBase),
    Glsl(GlslBase),
    Scene(SceneBase),
    /// Another theme's wallpaper, drawn as it is (images, prefilters and bakes included).
    Theme(ThemeBase),
}

/// `{ "kind": "theme", "theme": "rain-city-amber" }`: a theme id (folder name, resolved like
/// `config.theme`) or a path. Web and app wallpapers draw in child windows and can't be a base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeBase {
    pub theme: String,
}

impl ThemeBase {
    /// The referenced theme, loaded; an error when it is missing, a web/app wallpaper, or a
    /// stack (stacks are edited directly, so a base never nests).
    pub fn load(&self) -> Result<crate::LoadedTheme> {
        let dir = crate::locate(Some(&self.theme))?;
        let loaded = crate::LoadedTheme::load(&dir)?;
        match loaded.theme.wallpaper {
            crate::Wallpaper::Web(_) | crate::Wallpaper::App(_) => {
                bail!("web and app wallpapers draw in their own windows and can't be a base")
            }
            crate::Wallpaper::Stack(_) => bail!("a composition can't be the base of another"),
            _ => Ok(loaded),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageBase {
    pub source: PathBuf,
    #[serde(default)]
    pub fit: Fit,
    /// The point kept in view when `cover` crops, as fractions of the image.
    #[serde(default = "center")]
    pub focus: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoBase {
    pub source: PathBuf,
    #[serde(default)]
    pub fit: Fit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorBase {
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientBase {
    pub stops: Vec<GradientStop>,
    /// Degrees; 0 runs left to right, 90 top to bottom (linear only).
    #[serde(default)]
    pub angle: f32,
    #[serde(default, rename = "type")]
    pub kind: GradientKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: Color,
    pub at: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

/// The `shader` theme contract (`fn wallpaper(frag: vec2f) -> vec4f`), plus `@param` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderBase {
    pub source: PathBuf,
    /// Values for the shader's `// @param` comments; unknown ids are ignored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

/// A Shadertoy-style `mainImage`, kept with its credits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlslBase {
    pub source: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneBase {
    pub source: PathBuf,
    #[serde(default = "crate::default_orbit")]
    pub orbit: f32,
    #[serde(default = "crate::default_parallax")]
    pub parallax: f32,
    #[serde(default = "crate::default_background")]
    pub background: [f32; 3],
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default = "one")]
    pub exposure: f32,
}

/// What a scene draws behind its models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Sky,
    /// The `background` color.
    #[default]
    Color,
    /// Premultiplied alpha, for compositing.
    Transparent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Layer {
    Effect(EffectLayer),
    Image(ImageLayer),
    Model(ModelLayer),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectLayer {
    #[serde(default)]
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "one")]
    pub mix: f32,
    pub effect: String,
    /// Only values that differ from the effect's defaults; unknown ids are ignored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
    /// How the effect's output combines with its input before `mix`.
    #[serde(default)]
    pub blend: Blend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageLayer {
    #[serde(default)]
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "one")]
    pub mix: f32,
    pub source: PathBuf,
    /// `[x, y, w, h]` as fractions of the screen.
    pub rect: [f32; 4],
    #[serde(default)]
    pub blend: Blend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelLayer {
    #[serde(default)]
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "one")]
    pub mix: f32,
    pub source: PathBuf,
    pub rect: [f32; 4],
    /// Camera orbit in degrees per second (0 = still).
    #[serde(default)]
    pub orbit: f32,
    /// How far the camera leans toward the mouse (0 = off).
    #[serde(default)]
    pub parallax: f32,
    /// Direction toward the key light.
    #[serde(default = "default_light")]
    pub light: [f32; 3],
    #[serde(default = "one")]
    pub scale: f32,
    /// Degrees about x, y, z.
    #[serde(default)]
    pub rotation: [f32; 3],
    /// A glTF clip name to loop, or `null` for none.
    #[serde(default)]
    pub animation: Option<String>,
    #[serde(default)]
    pub blend: Blend,
}

fn default_light() -> [f32; 3] {
    [0.4, 0.8, 0.5]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    #[default]
    Normal,
    Add,
    Screen,
    Multiply,
}

impl Layer {
    pub fn id(&self) -> &str {
        match self {
            Layer::Effect(l) => &l.id,
            Layer::Image(l) => &l.id,
            Layer::Model(l) => &l.id,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Layer::Effect(l) => l.enabled,
            Layer::Image(l) => l.enabled,
            Layer::Model(l) => l.enabled,
        }
    }

    pub fn blend(&self) -> Blend {
        match self {
            Layer::Effect(l) => l.blend,
            Layer::Image(l) => l.blend,
            Layer::Model(l) => l.blend,
        }
    }

    pub fn mix(&self) -> f32 {
        match self {
            Layer::Effect(l) => l.mix,
            Layer::Image(l) => l.mix,
            Layer::Model(l) => l.mix,
        }
    }

    /// Drawn at all: enabled and mixed in. The engine skips the others entirely.
    pub fn active(&self) -> bool {
        self.enabled() && self.mix() > 0.0
    }
}

/// Where a stack starts running every frame (docs/wallpaper-builder.md, "Cost rules"): the
/// first layer from which everything runs at `fps`, or `None` for a still stack. `moves` holds,
/// per layer, whether it is active and animates on its own; a moving base runs every layer.
/// The layers before the split bake once. The engine and Studio's cost estimate both use this.
pub fn per_frame_from(base_moves: bool, moves: impl IntoIterator<Item = bool>) -> Option<usize> {
    if base_moves {
        return Some(0);
    }
    moves.into_iter().position(|m| m)
}

impl Base {
    /// True when the base changes over time (or with the cursor) on its own. A scene base
    /// moves only when it orbits or follows the cursor (it plays no clip).
    pub fn moves(&self) -> bool {
        match self {
            Base::Video(_) | Base::Shader(_) | Base::Glsl(_) => true,
            Base::Scene(s) => s.orbit != 0.0 || s.parallax != 0.0,
            Base::Image(_) | Base::Color(_) | Base::Gradient(_) => false,
            // A still picture bakes; everything else (shader, video, slideshow, scene) moves.
            // A missing theme draws black, which is still.
            Base::Theme(t) => t.load().is_ok_and(|l| {
                !matches!(l.theme.wallpaper, crate::Wallpaper::Image(_))
            }),
        }
    }
}

impl ModelLayer {
    pub fn moves(&self) -> bool {
        self.orbit != 0.0 || self.parallax != 0.0 || self.animation.is_some()
    }
}

impl StackWallpaper {
    /// Range checks and paths, for [`crate::LoadedTheme::parse`].
    pub(crate) fn validate(&self) -> Result<()> {
        crate::check_fps(self.fps)?;
        crate::check_scale(self.scale)?;
        match &self.base {
            Base::Image(b) => crate::check_path(&b.source)?,
            Base::Video(b) => crate::check_path(&b.source)?,
            Base::Shader(b) => crate::check_path(&b.source)?,
            Base::Glsl(b) => crate::check_path(&b.source)?,
            Base::Scene(b) => crate::check_path(&b.source)?,
            Base::Gradient(g) if g.stops.is_empty() => bail!("a gradient needs at least one stop"),
            Base::Theme(t) if t.theme.trim().is_empty() => bail!("a theme base needs a theme id"),
            Base::Theme(_) => {}
            Base::Color(_) | Base::Gradient(_) => {}
        }
        for layer in &self.layers {
            if !(0.0..=1.0).contains(&layer.mix()) {
                bail!("layer {:?}: mix must be between 0 and 1", layer.id());
            }
            match layer {
                Layer::Effect(l) => check_effect_id(&l.effect)?,
                Layer::Image(l) => crate::check_path(&l.source)?,
                Layer::Model(l) => crate::check_path(&l.source)?,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Effect packages

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Category,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// False: the result depends only on the input, so it can be baked into a still.
    #[serde(default)]
    pub animated: bool,
    /// Measured GPU ms at 1920x1080 on the iGPU, at defaults.
    #[serde(default)]
    pub cost: f32,
    /// True while `cost` is an estimate rather than a GPU measurement.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cost_estimated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<Reads>,
    pub passes: Vec<EffectPass>,
    #[serde(default)]
    pub params: Vec<Param>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Weather,
    Camera,
    #[default]
    Color,
    Motion,
    Stylize,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reads {
    Depth,
    Original,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectPass {
    /// A `.wgsl` file in the effect folder.
    pub source: PathBuf,
    /// Render scale of this pass, 0.0625..=1; the next pass samples it filtered.
    #[serde(default = "one")]
    pub scale: f32,
}

/// One tweakable value of an effect (or of a shader base, from `// @param`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(flatten)]
    pub kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParamKind {
    Float {
        min: f32,
        max: f32,
        default: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f32>,
    },
    Color {
        default: Color,
    },
    Bool {
        default: bool,
    },
    Enum {
        options: Vec<String>,
        default: String,
    },
}

impl Param {
    /// The WGSL type of this param's `Params` field.
    pub fn wgsl_type(&self) -> &'static str {
        match self.kind {
            ParamKind::Float { .. } => "f32",
            ParamKind::Color { .. } => "vec4f",
            ParamKind::Bool { .. } | ParamKind::Enum { .. } => "u32",
        }
    }

    /// `value` as this param's uniform words (falls back to the default when absent or of the
    /// wrong shape): one word, or four for a color.
    fn words(&self, value: Option<&Value>) -> Vec<u32> {
        match &self.kind {
            ParamKind::Float {
                min, max, default, ..
            } => {
                let v = value.and_then(Value::as_f64).map_or(*default, |v| v as f32);
                vec![v.clamp(min.min(*max), max.max(*min)).to_bits()]
            }
            ParamKind::Color { default } => {
                let color = value
                    .and_then(Value::as_str)
                    .and_then(Color::parse)
                    .unwrap_or(*default);
                let [r, g, b] = color.0.map(srgb_to_linear);
                [r, g, b, 1.0].map(f32::to_bits).to_vec()
            }
            ParamKind::Bool { default } => {
                let v = match value {
                    Some(Value::Bool(b)) => *b,
                    Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
                    _ => *default,
                };
                vec![u32::from(v)]
            }
            ParamKind::Enum { options, default } => {
                let index = |name: &str| options.iter().position(|o| o == name);
                let v = match value {
                    Some(Value::String(s)) => index(s),
                    Some(Value::Number(n)) => n
                        .as_u64()
                        .filter(|&n| (n as usize) < options.len())
                        .map(|n| n as usize),
                    _ => None,
                };
                vec![v.or_else(|| index(default)).unwrap_or(0) as u32]
            }
        }
    }

    /// The literal WGSL value of [`Self::words`], for shader bases' `const params`.
    fn wgsl_value(&self, value: Option<&Value>) -> String {
        let words = self.words(value);
        match self.kind {
            ParamKind::Float { .. } => format!("{:?}", f32::from_bits(words[0])),
            ParamKind::Color { .. } => {
                let c = words.iter().map(|w| format!("{:?}", f32::from_bits(*w)));
                format!("vec4f({})", c.collect::<Vec<_>>().join(", "))
            }
            ParamKind::Bool { .. } | ParamKind::Enum { .. } => format!("{}u", words[0]),
        }
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The generated `struct Params`, fields named as the param ids in declared order. WGSL needs
/// a member, so an effect without params gets `_none: u32`.
pub fn params_struct(params: &[Param]) -> String {
    let mut out = String::from("struct Params {\n");
    for param in params {
        out.push_str(&format!("    {}: {},\n", param.id, param.wgsl_type()));
    }
    if params.is_empty() {
        out.push_str("    _none: u32,\n");
    }
    out.push_str("}\n");
    out
}

/// The uniform buffer bytes for `struct Params`, laid out as WGSL does (`f32`/`u32` align 4,
/// `vec4f` align 16, size rounded up to 16).
pub fn params_bytes(params: &[Param], values: &BTreeMap<String, Value>) -> Vec<u8> {
    let mut words: Vec<u32> = Vec::new();
    for param in params {
        let value = param.words(values.get(&param.id));
        if value.len() == 4 {
            words.resize(words.len().next_multiple_of(4), 0);
        }
        words.extend(value);
    }
    words.resize(words.len().next_multiple_of(4).max(4), 0);
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A shader base's params as WGSL: the struct plus `const params = Params(...)`, so the
/// shader reads `params.<id>` with no binding at all (a change recompiles, like any edit).
pub fn shader_params_wgsl(params: &[Param], values: &BTreeMap<String, Value>) -> String {
    let fields: Vec<_> = params
        .iter()
        .map(|p| p.wgsl_value(values.get(&p.id)))
        .collect();
    let fields = if params.is_empty() {
        "0u".to_owned()
    } else {
        fields.join(", ")
    };
    format!(
        "{}const params = Params({fields});\n",
        params_struct(params)
    )
}

/// The shader theme prelude (the engine's `prelude.wgsl`), appended after a shader's code.
pub const SHADER_PRELUDE: &str = include_str!("../../wallpaper/src/prelude.wgsl");

/// A shader base exactly as the engine compiles it: `source` first (so compiler lines are the
/// file's lines), then its `// @param` struct and `const params`, then [`SHADER_PRELUDE`].
pub fn shader_base_wgsl(source: &str, values: &BTreeMap<String, Value>) -> String {
    let params = shader_params(source);
    format!(
        "{source}
{}
{SHADER_PRELUDE}",
        shader_params_wgsl(&params, values)
    )
}

/// One effect pass as the engine compiles it.
#[derive(Debug, Clone, PartialEq)]
pub struct PassWgsl {
    pub code: String,
    /// The 1-based line of `code` holding the pass file's first line: subtract
    /// `first_line - 1` from a compiler line to get the line in the pass file.
    pub first_line: u32,
}

impl PassWgsl {
    /// A line in `code` as a line of the pass file, or `None` inside the prelude/params.
    pub fn source_line(&self, line: u32) -> Option<u32> {
        line.checked_sub(self.first_line - 1).filter(|&l| l > 0)
    }
}

/// Builds one pass: [`EFFECT_PRELUDE`], then the generated `struct Params`, then `pass`.
pub fn pass_wgsl(manifest: &EffectManifest, pass: &str) -> PassWgsl {
    let head = format!("{EFFECT_PRELUDE}\n{}\n", params_struct(&manifest.params));
    PassWgsl {
        first_line: head.lines().count() as u32 + 1,
        code: format!("{head}{pass}"),
    }
}

/// An effect manifest together with the folder it was loaded from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedEffect {
    pub dir: PathBuf,
    pub manifest: EffectManifest,
}

impl LoadedEffect {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let path = dir.join(EFFECT_MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest = parse_effect(&text).with_context(|| format!("in {}", path.display()))?;
        Ok(Self { dir, manifest })
    }

    /// Pass `index` built with the prelude ([`pass_wgsl`]).
    pub fn pass(&self, index: usize) -> Result<PassWgsl> {
        let pass = self.manifest.passes.get(index).context("no such pass")?;
        let path = self.dir.join(&pass.source);
        let code = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok(pass_wgsl(&self.manifest, &code))
    }
}

/// Parses and checks an `effect.json`.
pub fn parse_effect(json: &str) -> Result<EffectManifest> {
    let manifest: EffectManifest = serde_json::from_str(json)?;
    check_effect_id(&manifest.id)?;
    if manifest.passes.is_empty() {
        bail!("an effect needs at least one pass");
    }
    for pass in &manifest.passes {
        crate::check_path(&pass.source)?;
        if !(0.0625..=1.0).contains(&pass.scale) {
            bail!(
                "pass scale must be between 0.0625 and 1, got {}",
                pass.scale
            );
        }
    }
    for (i, param) in manifest.params.iter().enumerate() {
        let id = &param.id;
        let mut chars = id.chars();
        let ident = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ident {
            bail!("param id {id:?} must be a WGSL identifier ([A-Za-z_][A-Za-z0-9_]*)");
        }
        if manifest.params[..i].iter().any(|p| &p.id == id) {
            bail!("param id {id:?} is declared twice");
        }
        if let ParamKind::Enum { options, default } = &param.kind
            && !options.contains(default)
        {
            bail!("param {id:?}: default {default:?} is not one of its options");
        }
    }
    Ok(manifest)
}

/// Effect ids are folder names: `[a-z0-9-]`, so a layer can never point outside `effects/`.
fn check_effect_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("effect id {id:?} must be [a-z0-9-]");
    }
    Ok(())
}

/// The shared effect folders: `%APPDATA%\mypaper\effects` (if it exists), then the bundled
/// `effects/` beside the executable or a parent. A theme's own `effects/` comes after them in
/// [`locate_effect`].
pub fn effect_roots() -> Vec<PathBuf> {
    let user = mypaper_config::dir()
        .ok()
        .map(|dir| dir.join("effects"))
        .filter(|dir| dir.is_dir());
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors()
            .skip(1)
            .map(|dir| dir.join("effects"))
            .find(|dir| dir.join("prelude.wgsl").is_file())
    });
    user.into_iter().chain(bundled).collect()
}

/// Resolves effect `id` for the theme in `theme_dir`: each of [`effect_roots`], then the
/// theme's `effects/<id>` ([`effect_search`]).
pub fn locate_effect(id: &str, theme_dir: &Path) -> Result<LoadedEffect> {
    locate_effect_in(id, &effect_search(theme_dir))
}

/// Where [`locate_effect`] looks, in order: the shared roots first, then the theme's own
/// `effects/`. Studio packages every used effect into a saved theme so it travels when shared;
/// those copies are only a fallback, or a snapshot would pin a theme to an effect's old bugs
/// (a black `crt`) forever. A theme's own custom effects have ids no shared root uses.
pub fn effect_search(theme_dir: &Path) -> Vec<PathBuf> {
    effect_roots()
        .into_iter()
        .chain(std::iter::once(theme_dir.join("effects")))
        .collect()
}

pub fn locate_effect_in(id: &str, roots: &[PathBuf]) -> Result<LoadedEffect> {
    check_effect_id(id)?;
    let dir = roots
        .iter()
        .map(|root| root.join(id))
        .find(|dir| dir.join(EFFECT_MANIFEST).is_file())
        .with_context(|| {
            let tried: Vec<_> = roots.iter().map(|r| r.display().to_string()).collect();
            format!("effect {id:?} not found in {}", tried.join(", "))
        })?;
    LoadedEffect::load(dir)
}

// ---------------------------------------------------------------------------------------------
// `// @param` comments in shader bases

/// Reads `// @param` comments from a shader base, in order. Malformed lines are skipped.
///
/// ```text
/// // @param amount float 0..1 = 0.5 "Amount"
/// // @param tint color = #ffffff "Tint"
/// // @param streaks bool = true "Streaks"
/// // @param style enum drizzle|storm = drizzle "Style"
/// ```
pub fn shader_params(source: &str) -> Vec<Param> {
    source.lines().filter_map(param_comment).collect()
}

fn param_comment(line: &str) -> Option<Param> {
    let rest = line
        .trim()
        .strip_prefix("//")?
        .trim()
        .strip_prefix("@param")?;
    let (spec, label) = match rest.split_once('"') {
        Some((spec, label)) => (spec, label.trim_end().trim_end_matches('"').to_owned()),
        None => (rest, String::new()),
    };
    let (decl, default) = spec.split_once('=')?;
    let default = default.trim();
    let mut words = decl.split_whitespace();
    let id = words.next()?.to_owned();
    let ty = words.next()?;
    let extra = words.next();
    let kind = match ty {
        "float" => {
            let (min, max) = extra.unwrap_or("0..1").split_once("..")?;
            let (min, max): (f32, f32) = (min.parse().ok()?, max.parse().ok()?);
            ParamKind::Float {
                min,
                max,
                default: default.parse().ok()?,
                step: None,
            }
        }
        "color" => ParamKind::Color {
            default: Color::parse(default)?,
        },
        "bool" => ParamKind::Bool {
            default: default.parse().ok()?,
        },
        "enum" => {
            let options: Vec<String> = extra?.split('|').map(str::to_owned).collect();
            options.contains(&default.to_owned()).then_some(())?;
            ParamKind::Enum {
                options,
                default: default.to_owned(),
            }
        }
        _ => return None,
    };
    let label = if label.is_empty() { id.clone() } else { label };
    Some(Param { id, label, kind })
}

// ---------------------------------------------------------------------------------------------
// Shadertoy GLSL bases

/// Wraps a Shadertoy `mainImage(out vec4 fragColor, in vec2 fragCoord)` into a GLSL 450
/// fragment shader. Inputs are one uniform block at set 0, binding 0 ([`GLSL_UNIFORM_SIZE`]
/// bytes, std140): `vec3 iResolution; float iTime; vec4 iMouse; float iTimeDelta; int iFrame;
/// float iFrameRate; vec3 iChannelResolution[4]`. `iChannel0..3` all sample one tiling noise
/// texture (binding 1, [`GLSL_NOISE_SIZE`] square RGBA) through a repeating linear sampler
/// (binding 2), like Shadertoy's noise inputs. `fragCoord` has Shadertoy's bottom-left origin.
/// The user's line N is line `N + GLSL_HEADER_LINES` of the result.
pub fn glsl_fragment(main_image: &str) -> String {
    format!("{GLSL_HEADER}{main_image}\n{GLSL_FOOTER}")
}

const GLSL_HEADER: &str = "#version 450
layout(set = 0, binding = 0, std140) uniform MypaperShadertoy {
    vec3 iResolution; float iTime; vec4 iMouse; float iTimeDelta; int iFrame; float iFrameRate;
    vec3 iChannelResolution[4];
};
layout(set = 0, binding = 1) uniform texture2D mypaper_noise;
layout(set = 0, binding = 2) uniform sampler mypaper_repeat;
#define iChannel0 sampler2D(mypaper_noise, mypaper_repeat)
#define iChannel1 sampler2D(mypaper_noise, mypaper_repeat)
#define iChannel2 sampler2D(mypaper_noise, mypaper_repeat)
#define iChannel3 sampler2D(mypaper_noise, mypaper_repeat)
layout(location = 0) out vec4 mypaper_color;
";

const GLSL_FOOTER: &str = "void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    mainImage(color, vec2(gl_FragCoord.x, iResolution.y - gl_FragCoord.y));
    mypaper_color = vec4(color.rgb, 1.0);
}
";

/// Lines [`glsl_fragment`] puts before the user's code.
pub const GLSL_HEADER_LINES: u32 = 12;

/// Bytes of the GLSL uniform block (std140).
pub const GLSL_UNIFORM_SIZE: u64 = 112;

/// Width and height of the noise texture behind `iChannel0..3`.
pub const GLSL_NOISE_SIZE: u32 = 256;

/// One compile error, on a line of the user's file (`None` when it is in generated code).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub line: Option<u32>,
    pub message: String,
}

/// naga checks of exactly what the engine compiles (`check` feature).
#[cfg(feature = "check")]
pub mod check {
    use super::*;

    fn validate(module: &naga::Module) -> Result<(), naga::WithSpan<naga::valid::ValidationError>> {
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::IMMEDIATES,
        )
        .validate(module)
        .map(drop)
    }

    /// Parses and validates a WGSL module; `to_line` maps a line of `code` to the user's file.
    pub fn wgsl(
        code: &str,
        to_line: impl Fn(u32) -> Option<u32>,
    ) -> Result<naga::Module, Diagnostic> {
        let module = naga::front::wgsl::parse_str(code).map_err(|e| Diagnostic {
            line: e.location(code).and_then(|l| to_line(l.line_number)),
            message: e.message().to_owned(),
        })?;
        validate(&module).map_err(|e| Diagnostic {
            line: e.location(code).and_then(|l| to_line(l.line_number)),
            message: e.as_inner().to_string(),
        })?;
        Ok(module)
    }

    /// Checks a shader base as built by [`shader_base_wgsl`]; lines are the file's own.
    pub fn shader_base(
        source: &str,
        values: &BTreeMap<String, Value>,
    ) -> Result<naga::Module, Diagnostic> {
        let lines = source.lines().count() as u32;
        wgsl(&shader_base_wgsl(source, values), |l| {
            (l <= lines).then_some(l)
        })
    }

    /// Checks one effect pass as built by [`pass_wgsl`], with errors on the pass file's lines.
    pub fn pass(manifest: &EffectManifest, source: &str) -> Result<naga::Module, Diagnostic> {
        let built = pass_wgsl(manifest, source);
        wgsl(&built.code, |line| built.source_line(line))
    }

    /// Translates a Shadertoy `mainImage` ([`glsl_fragment`]), errors on the user's lines.
    pub fn glsl(main_image: &str) -> Result<naga::Module, Diagnostic> {
        let code = glsl_fragment(main_image);
        let to_line = |line: u32| line.checked_sub(GLSL_HEADER_LINES).filter(|&l| l > 0);
        let options = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
        let module = naga::front::glsl::Frontend::default()
            .parse(&options, &code)
            .map_err(|errors| {
                let first = errors.errors.first();
                Diagnostic {
                    line: first
                        .and_then(|e| e.location(&code))
                        .and_then(|l| to_line(l.line_number)),
                    message: first.map_or_else(|| "GLSL error".to_owned(), |e| e.kind.to_string()),
                }
            })?;
        validate(&module).map_err(|e| Diagnostic {
            line: e.location(&code).and_then(|l| to_line(l.line_number)),
            message: e.as_inner().to_string(),
        })?;
        Ok(module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r##"{
        "kind": "stack", "fps": 24, "scale": 0.75,
        "base": { "kind": "image", "source": "media/photo.jpg", "fit": "cover", "focus": [0.4, 0.6] },
        "layers": [
            { "id": "rain-1", "kind": "effect", "effect": "rain", "enabled": true,
              "mix": 0.8, "params": { "amount": 0.6, "tint": "#c8d8ff" } },
            { "id": "logo", "kind": "image", "source": "media/logo.png",
              "rect": [0.7, 0.1, 0.2, 0.2], "blend": "screen", "mix": 1.0 },
            { "id": "planet", "kind": "model", "source": "media/planet.glb",
              "rect": [0.05, 0.4, 0.4, 0.5], "orbit": 6.0, "parallax": 0.3, "light": [0.4, 0.8, 0.5],
              "rotation": [0, 90, 0], "animation": "spin" }
        ]
    }"##;

    fn stack(json: &str) -> StackWallpaper {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_full_stack_round_trips() {
        let parsed = stack(FULL);
        assert_eq!((parsed.fps, parsed.scale), (24, 0.75));
        assert!(matches!(&parsed.base, Base::Image(b) if b.focus == [0.4, 0.6]));
        assert!(matches!(&parsed.layers[1], Layer::Image(l) if l.blend == Blend::Screen));
        assert!(matches!(&parsed.layers[2], Layer::Model(l) if l.moves() && l.scale == 1.0));
        let again: StackWallpaper =
            serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
        assert_eq!(again, parsed);
        for base in [
            r##"{ "kind": "video", "source": "a.mp4" }"##,
            r##"{ "kind": "color", "color": "#102030" }"##,
            r##"{ "kind": "gradient", "stops": [{ "color": "#000000", "at": 0 }, { "color": "#ffffff", "at": 1 }], "angle": 45, "type": "radial" }"##,
            r##"{ "kind": "shader", "source": "a.wgsl", "params": { "amount": 0.2 } }"##,
            r##"{ "kind": "glsl", "source": "a.glsl", "author": "iq", "license": "CC BY-NC-SA 3.0", "url": "https://shadertoy.com/view/x" }"##,
            r##"{ "kind": "scene", "source": "a.glb", "environment": "transparent", "exposure": 1.5 }"##,
        ] {
            let parsed: Base = serde_json::from_str(base).unwrap();
            let again: Base =
                serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
            assert_eq!(again, parsed, "{base}");
        }
    }

    #[test]
    fn stacks_validate_through_the_theme_parser() {
        let theme = |w: &str| {
            crate::LoadedTheme::parse(
                "t".into(),
                &format!(r#"{{ "name": "A", "wallpaper": {w} }}"#),
            )
        };
        assert!(theme(FULL).is_ok());
        assert!(theme(&FULL.replace("\"fps\": 24", "\"fps\": 0")).is_err());
        assert!(theme(&FULL.replace("media/logo.png", "../logo.png")).is_err());
        assert!(theme(&FULL.replace("\"effect\": \"rain\"", "\"effect\": \"../rain\"")).is_err());
        assert!(theme(&FULL.replace("\"mix\": 0.8", "\"mix\": 2")).is_err());
    }

    fn manifest() -> EffectManifest {
        parse_effect(
            r##"{ "id": "rain", "name": "Rain", "category": "weather", "animated": true, "cost": 1.2,
              "passes": [{ "source": "rain.wgsl" }],
              "params": [
                { "id": "amount", "label": "Amount", "type": "float", "min": 0, "max": 1, "default": 0.6, "step": 0.01 },
                { "id": "tint", "label": "Tint", "type": "color", "default": "#ffffff" },
                { "id": "streaks", "label": "Streaks", "type": "bool", "default": true },
                { "id": "style", "label": "Style", "type": "enum", "options": ["drizzle", "storm"], "default": "drizzle" }
              ] }"##,
        )
        .unwrap()
    }

    #[test]
    fn params_lay_out_like_wgsl_and_ignore_unknown_ids() {
        let m = manifest();
        assert_eq!(
            params_struct(&m.params),
            "struct Params {\n    amount: f32,\n    tint: vec4f,\n    streaks: u32,\n    style: u32,\n}\n"
        );
        let values: BTreeMap<String, Value> = serde_json::from_str(
            r##"{ "amount": 3.0, "tint": "#000000", "style": "storm", "gone": 1, "streaks": "x" }"##,
        )
        .unwrap();
        let bytes = params_bytes(&m.params, &values);
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        // amount (clamped) @0, tint @16 (vec4 alignment), streaks @32, style @36, size 48.
        assert_eq!(bytes.len(), 48);
        assert_eq!(f32::from_bits(word(0)), 1.0);
        assert_eq!(
            (f32::from_bits(word(4)), f32::from_bits(word(7))),
            (0.0, 1.0)
        );
        assert_eq!(
            (word(8), word(9)),
            (1, 1),
            "wrong-typed streaks keeps its default"
        );
        assert_eq!(params_bytes(&[], &BTreeMap::new()).len(), 16);
    }

    #[test]
    fn effect_manifests_check_ids_passes_and_enums() {
        let json = serde_json::to_string(&manifest()).unwrap();
        assert_eq!(parse_effect(&json).unwrap(), manifest());
        assert!(parse_effect(&json.replace("\"rain\"", "\"Rain!\"")).is_err());
        assert!(parse_effect(&json.replace("\"amount\"", "\"2amount\"")).is_err());
        assert!(
            parse_effect(&json.replace("\"default\":\"drizzle\"", "\"default\":\"hail\"")).is_err()
        );
        assert!(parse_effect(&json.replace("rain.wgsl", "../rain.wgsl")).is_err());
    }

    #[test]
    fn pass_lines_map_back_to_the_pass_file() {
        let built = pass_wgsl(
            &manifest(),
            "fn effect(frag: vec2f, uv: vec2f) -> vec4f {\n    return x;\n}\n",
        );
        let line = built
            .code
            .lines()
            .position(|l| l.contains("return x"))
            .unwrap() as u32
            + 1;
        assert_eq!(built.source_line(line), Some(2));
        assert_eq!(built.source_line(1), None);
    }

    #[test]
    fn param_comments_become_params() {
        let params = shader_params(
            "// @param amount float 0..2 = 0.5 \"Amount\"\n\
             //@param tint color = #ff8000 \"Tint\"\n\
             // @param on bool = true\n\
             // @param style enum drizzle|storm = storm \"Style\"\n\
             // @param broken float 0..x = 1\n\
             fn wallpaper(frag: vec2f) -> vec4f { return vec4f(0.0); }",
        );
        let ids: Vec<_> = params.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["amount", "tint", "on", "style"]);
        assert_eq!(
            params[0].kind,
            ParamKind::Float {
                min: 0.0,
                max: 2.0,
                default: 0.5,
                step: None
            }
        );
        assert_eq!(params[2].label, "on");
        assert!(matches!(&params[3].kind, ParamKind::Enum { default, .. } if default == "storm"));
        let wgsl = shader_params_wgsl(&params, &BTreeMap::new());
        assert!(
            wgsl.contains("const params = Params(0.5, vec4f(1.0, "),
            "{wgsl}"
        );
    }

    #[test]
    fn effects_resolve_theme_then_user_then_bundled() {
        let root = std::env::temp_dir().join(format!("mypaper-effects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |dir: &Path, name: &str| {
            std::fs::create_dir_all(dir).unwrap();
            let json = format!(
                r#"{{ "id": "glow", "name": "{name}", "passes": [{{ "source": "a.wgsl" }}] }}"#
            );
            std::fs::write(dir.join(EFFECT_MANIFEST), json).unwrap();
        };
        let roots = [
            root.join("theme/effects"),
            root.join("user"),
            root.join("bundled"),
        ];
        write(&roots[2].join("glow"), "bundled");
        assert_eq!(
            locate_effect_in("glow", &roots).unwrap().manifest.name,
            "bundled"
        );
        write(&roots[1].join("glow"), "user");
        assert_eq!(
            locate_effect_in("glow", &roots).unwrap().manifest.name,
            "user"
        );
        write(&roots[0].join("glow"), "theme");
        assert_eq!(
            locate_effect_in("glow", &roots).unwrap().manifest.name,
            "theme"
        );
        assert!(locate_effect_in("nope", &roots).is_err());
        assert!(locate_effect_in("..", &roots).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(feature = "check")]
    #[test]
    fn checks_report_lines_in_the_users_file() {
        let ok = "fn effect(frag: vec2f, uv: vec2f) -> vec4f {\n    return sample_src(uv) * params.amount;\n}\n";
        check::pass(&manifest(), ok).unwrap();
        let bad = "fn effect(frag: vec2f, uv: vec2f) -> vec4f {\n    return nope;\n}\n";
        assert_eq!(check::pass(&manifest(), bad).unwrap_err().line, Some(2));
        let params =
            shader_params("// @param amount float 0..1 = 0.5\n// @param tint color = #ffffff\n");
        check::wgsl(&shader_params_wgsl(&params, &BTreeMap::new()), Some).unwrap();
        let toy = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    vec2 uv = fragCoord / iResolution.xy;\n    fragColor = vec4(uv, 0.5 + 0.5 * sin(iTime), 1.0);\n}\n";
        check::glsl(toy).unwrap();
        let base = "// @param amount float 0..1 = 0.5
fn wallpaper(frag: vec2f) -> vec4f {
    return vec4f(params.amount * frag.x / paper.resolution.x);
}
";
        check::shader_base(base, &BTreeMap::new()).unwrap();
        let typo = base.replace("params.amount", "params.amont");
        assert_eq!(
            check::shader_base(&typo, &BTreeMap::new())
                .unwrap_err()
                .line,
            Some(3)
        );
        let broken = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    fragColor = vec4(nope);\n}\n";
        assert_eq!(check::glsl(broken).unwrap_err().line, Some(2));
    }
}
