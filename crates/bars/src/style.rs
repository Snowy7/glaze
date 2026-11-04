//! The bars' look and motion as `bars.json` writes them (docs/bars.md, "Look" and "Animation"):
//! backgrounds and materials, borders, glow and shadow per state, item states, and the animation
//! tree. The host only validates these; the page turns them into CSS (`bars/shell/src/style/`).

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Fails with `<name> must be between <lo> and <hi>` unless `value` is in `range`.
pub fn check(name: &str, value: Option<f32>, range: std::ops::RangeInclusive<f32>) -> Result<()> {
    match value {
        Some(v) if !range.contains(&v) => bail!(
            "{name} must be between {} and {}, got {v}",
            range.start(),
            range.end()
        ),
        _ => Ok(()),
    }
}

// --- Colors ------------------------------------------------------------------------------------

/// Named colors the page maps to its CSS variables.
pub const COLOR_TOKENS: &[&str] = &[
    "accent",
    "text",
    "transparent",
    "system-accent",
    "system-accent-light-1",
    "system-accent-light-2",
    "system-accent-light-3",
    "system-accent-dark-1",
    "system-accent-dark-2",
    "system-accent-dark-3",
];

/// A color: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, a token ([`COLOR_TOKENS`]), or
/// `var(--name)`, a theme variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorRef(String);

impl ColorRef {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(hex) = text.strip_prefix('#') {
            let ok =
                matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
            return ok.then(|| Self(text.to_ascii_lowercase()));
        }
        let var = text
            .strip_prefix("var(--")
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|name| {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            });
        (var || COLOR_TOKENS.contains(&text)).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ColorRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ColorRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "expected a color (#rrggbb, #rrggbbaa, var(--name) or one of {}), got {text:?}",
                COLOR_TOKENS.join(", ")
            ))
        })
    }
}

// --- Fills -------------------------------------------------------------------------------------

/// A surface's background.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Fill {
    /// Nothing: content sits on whatever is behind the bar.
    None,
    Solid {
        color: ColorRef,
    },
    Gradient(Gradient),
    Image(ImageFill),
    /// "Wallpaper glass": the wallpaper engine's blurred snapshot of this monitor, aligned to the
    /// screen, under the tint.
    Wallpaper(WallpaperFill),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
    Conic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gradient {
    #[serde(default)]
    pub kind: GradientKind,
    /// Degrees, for linear and conic gradients (CSS: 180 is top to bottom).
    #[serde(default = "default_angle")]
    pub angle: f32,
    /// 2 to 8 colors, each a color or `{ "color", "at": 0..1 }`.
    pub stops: Vec<Stop>,
}

fn default_angle() -> f32 {
    180.0
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Stop {
    Color(ColorRef),
    At { color: ColorRef, at: f32 },
}

impl<'de> Deserialize<'de> for Stop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct At {
            color: ColorRef,
            at: f32,
        }
        match Value::deserialize(deserializer)? {
            Value::String(text) => ColorRef::parse(&text)
                .map(Stop::Color)
                .ok_or_else(|| serde::de::Error::custom(format!("bad gradient color {text:?}"))),
            other => {
                let At { color, at } = At::deserialize(other).map_err(serde::de::Error::custom)?;
                Ok(Stop::At { color, at })
            }
        }
    }
}

impl Gradient {
    fn validate(&self, name: &str) -> Result<()> {
        if !(2..=8).contains(&self.stops.len()) {
            bail!("{name}.stops needs 2 to 8 colors");
        }
        check(&format!("{name}.angle"), Some(self.angle), 0.0..=360.0)?;
        for stop in &self.stops {
            if let Stop::At { at, .. } = stop {
                check(&format!("{name}.stops[].at"), Some(*at), 0.0..=1.0)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFit {
    #[default]
    Cover,
    Contain,
    Fill,
    Tile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFill {
    /// An image file: absolute, or relative to the active theme's folder.
    pub path: String,
    #[serde(default)]
    pub fit: ImageFit,
    /// CSS `background-position` keywords (`center`, `top`, `left 30%`…).
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "one")]
    pub brightness: f32,
    /// Extra blur in DIPs.
    #[serde(default)]
    pub blur: f32,
}

fn default_position() -> String {
    "center".into()
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperFill {
    #[serde(default = "one")]
    pub brightness: f32,
    /// Extra blur in DIPs on top of the engine's (usually 0: the snapshot is already blurred).
    #[serde(default)]
    pub blur: f32,
    /// Seconds between snapshots of an animated wallpaper.
    #[serde(default = "default_interval")]
    pub interval: u32,
}

fn default_interval() -> u32 {
    10
}

impl Fill {
    fn validate(&self) -> Result<()> {
        match self {
            Fill::None | Fill::Solid { .. } => Ok(()),
            Fill::Gradient(gradient) => gradient.validate("look.background"),
            Fill::Image(image) => {
                if image.path.trim().is_empty() {
                    bail!("look.background.path is empty");
                }
                let words: Vec<&str> = image.position.split_whitespace().collect();
                let word_ok = |w: &&str| {
                    matches!(*w, "center" | "top" | "bottom" | "left" | "right")
                        || w.strip_suffix('%')
                            .is_some_and(|n| n.parse::<u8>().is_ok_and(|n| n <= 100))
                };
                if words.is_empty() || words.len() > 2 || !words.iter().all(word_ok) {
                    bail!(
                        "look.background.position: expected up to two of center/top/bottom/left/right/<n>%, got {:?}",
                        image.position
                    );
                }
                check(
                    "look.background.brightness",
                    Some(image.brightness),
                    0.0..=2.0,
                )?;
                check("look.background.blur", Some(image.blur), 0.0..=64.0)
            }
            Fill::Wallpaper(wallpaper) => {
                check(
                    "look.background.brightness",
                    Some(wallpaper.brightness),
                    0.0..=2.0,
                )?;
                check("look.background.blur", Some(wallpaper.blur), 0.0..=64.0)?;
                if !(2..=600).contains(&wallpaper.interval) {
                    bail!("look.background.interval must be between 2 and 600 seconds");
                }
                Ok(())
            }
        }
    }
}

/// A border or item paint: a color, or a gradient.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Paint {
    Color(ColorRef),
    Gradient(Gradient),
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::String(text) => ColorRef::parse(&text)
                .map(Paint::Color)
                .ok_or_else(|| serde::de::Error::custom(format!("bad color {text:?}"))),
            other => Gradient::deserialize(other)
                .map(Paint::Gradient)
                .map_err(serde::de::Error::custom),
        }
    }
}

impl Paint {
    fn validate(&self, name: &str) -> Result<()> {
        match self {
            Paint::Color(_) => Ok(()),
            Paint::Gradient(gradient) => gradient.validate(name),
        }
    }
}

// --- States ------------------------------------------------------------------------------------

/// A style at rest, under the pointer, and active (pressed, open, or the app in front). Hover and
/// active start from rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerState<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<T>,
}

impl<T> Default for PerState<T> {
    fn default() -> Self {
        Self {
            rest: None,
            hover: None,
            active: None,
        }
    }
}

impl<T> PerState<T> {
    fn each(&self) -> impl Iterator<Item = (&'static str, &T)> {
        [
            ("rest", &self.rest),
            ("hover", &self.hover),
            ("active", &self.active),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.as_ref().map(|v| (name, v)))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BorderStyle {
    /// DIPs, 0..8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Paint>,
}

impl BorderStyle {
    fn validate(&self, name: &str) -> Result<()> {
        check(&format!("{name}.width"), self.width, 0.0..=8.0)?;
        self.color
            .as_ref()
            .map_or(Ok(()), |c| c.validate(&format!("{name}.color")))
    }
}

/// A soft colored halo (an outer box shadow with no offset).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlowStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Blur radius in DIPs, 0..64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
}

impl GlowStyle {
    fn validate(&self, name: &str) -> Result<()> {
        check(&format!("{name}.size"), self.size, 0.0..=64.0)?;
        check(&format!("{name}.opacity"), self.opacity, 0.0..=1.0)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<f32>,
}

impl ShadowStyle {
    fn validate(&self, name: &str) -> Result<()> {
        check(&format!("{name}.x"), self.x, -64.0..=64.0)?;
        check(&format!("{name}.y"), self.y, -64.0..=64.0)?;
        check(&format!("{name}.blur"), self.blur, 0.0..=96.0)?;
        check(&format!("{name}.spread"), self.spread, -32.0..=32.0)
    }
}

/// How module buttons and dock items look in one state.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<BorderStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glow: Option<GlowStyle>,
    /// 0.5..1.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
}

impl ItemStyle {
    fn validate(&self, name: &str) -> Result<()> {
        if let Some(background) = &self.background {
            background.validate(&format!("{name}.background"))?;
        }
        if let Some(border) = &self.border {
            border.validate(&format!("{name}.border"))?;
        }
        if let Some(glow) = &self.glow {
            glow.validate(&format!("{name}.glow"))?;
        }
        check(&format!("{name}.scale"), self.scale, 0.5..=1.5)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Items {
    /// Corner radius of item backgrounds, DIPs 0..32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest: Option<ItemStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<ItemStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<ItemStyle>,
}

/// Running-indicator colors per state.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndicatorColors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<ColorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<ColorRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention: Option<ColorRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Follows Windows' light or dark mode.
    Auto,
    Light,
    Dark,
}

/// CSS `corner-shape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    #[default]
    Round,
    Squircle,
    Bevel,
    Scoop,
    Notch,
}

/// The bar surfaces' look beyond `appearance` (docs/bars.md, "Look").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    /// Light or dark; missing: from `appearance.text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<Scheme>,
    /// Missing: the appearance's tint at its opacity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Fill>,
    /// Opacity of the tint laid over an image, gradient or wallpaper background, 0..1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<f32>,
    /// Saturation of what's behind (wallpaper or image), 0..3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
    /// Opacity of a fine noise texture over the surface, 0..0.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise: Option<f32>,
    /// Corner radius in DIPs, 0..64; missing: `appearance.radius`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corners: Option<Corners>,
    pub border: PerState<BorderStyle>,
    pub glow: PerState<GlowStyle>,
    pub shadow: PerState<ShadowStyle>,
    pub items: Items,
    pub indicator: IndicatorColors,
}

impl Look {
    pub fn validate(&self) -> Result<()> {
        if let Some(background) = &self.background {
            background.validate()?;
        }
        check("look.tint", self.tint, 0.0..=1.0)?;
        check("look.saturation", self.saturation, 0.0..=3.0)?;
        check("look.noise", self.noise, 0.0..=0.5)?;
        check("look.radius", self.radius, 0.0..=64.0)?;
        for (state, border) in self.border.each() {
            border.validate(&format!("look.border.{state}"))?;
        }
        for (state, glow) in self.glow.each() {
            glow.validate(&format!("look.glow.{state}"))?;
        }
        for (state, shadow) in self.shadow.each() {
            shadow.validate(&format!("look.shadow.{state}"))?;
        }
        check("look.items.radius", self.items.radius, 0.0..=32.0)?;
        for (state, item) in [
            ("rest", &self.items.rest),
            ("hover", &self.items.hover),
            ("active", &self.items.active),
        ] {
            if let Some(item) = item {
                item.validate(&format!("look.items.{state}"))?;
            }
        }
        Ok(())
    }

    /// The wallpaper-glass background, if this look uses it.
    pub fn wallpaper(&self) -> Option<&WallpaperFill> {
        match &self.background {
            Some(Fill::Wallpaper(wallpaper)) => Some(wallpaper),
            _ => None,
        }
    }
}

// --- Animation ---------------------------------------------------------------------------------

/// Named easing curves; the page maps them to CSS (`bars/shell/src/style/motion.ts`).
pub const NAMED_CURVES: &[&str] = &[
    "linear",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "smooth",
    "snappy",
    "emphasized",
    "overshoot",
    "spring",
    "spring-soft",
    "spring-bouncy",
];

/// An easing curve: a name, a cubic Bézier `[x1, y1, x2, y2]`, or a spring
/// `{ "mass", "stiffness", "damping" }` (a spring sets its own duration).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Curve {
    Named(String),
    Bezier([f32; 4]),
    Spring(Spring),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spring {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl<'de> Deserialize<'de> for Curve {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let curve = match Value::deserialize(deserializer)? {
            Value::String(name) if NAMED_CURVES.contains(&name.as_str()) => Curve::Named(name),
            Value::String(name) => {
                return Err(D::Error::custom(format!(
                    "unknown curve {name:?}; expected one of {}, [x1, y1, x2, y2] or {{ mass, stiffness, damping }}",
                    NAMED_CURVES.join(", ")
                )));
            }
            array @ Value::Array(_) => {
                let [x1, y1, x2, y2] = <[f32; 4]>::deserialize(array).map_err(D::Error::custom)?;
                if !(0.0..=1.0).contains(&x1)
                    || !(0.0..=1.0).contains(&x2)
                    || !(-2.0..=3.0).contains(&y1)
                    || !(-2.0..=3.0).contains(&y2)
                {
                    return Err(D::Error::custom(
                        "a cubic-bezier needs x1 and x2 in 0..1 and y1, y2 in -2..3",
                    ));
                }
                Curve::Bezier([x1, y1, x2, y2])
            }
            object => {
                let spring = Spring::deserialize(object).map_err(D::Error::custom)?;
                if !(0.1..=10.0).contains(&spring.mass)
                    || !(1.0..=2000.0).contains(&spring.stiffness)
                    || !(0.0..=200.0).contains(&spring.damping)
                {
                    return Err(D::Error::custom(
                        "a spring needs mass 0.1..10, stiffness 1..2000 and damping 0..200",
                    ));
                }
                Curve::Spring(spring)
            }
        };
        Ok(curve)
    }
}

/// How something enters and leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MotionStyle {
    Slide,
    Fade,
    Pop,
    Scale,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReduceMotion {
    /// Windows' "Animation effects" setting (`SPI_GETCLIENTAREAANIMATION`).
    #[default]
    System,
    Always,
    Never,
}

/// One node of the animation tree. Each leaf takes, field by field, the most specific value on
/// its path: `animation` < `bar` < `top`/`dock`, then the same leaf name at those levels; any
/// `enabled: false` on the path turns it off (docs/bars.md, "Animation").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Motion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Milliseconds, 0..5000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
    /// For the bars' show and the flyouts' open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<MotionStyle>,
    /// Root only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_motion: Option<ReduceMotion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dock: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub press: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnify: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce: Option<Box<Motion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention: Option<Box<Motion>>,
}

/// Where a node sits in the tree, which decides its allowed children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Root,
    Bar,
    Top,
    Dock,
    Leaf,
}

impl Motion {
    pub fn validate(&self) -> Result<()> {
        self.validate_at(Level::Root, "animation")
    }

    fn validate_at(&self, level: Level, path: &str) -> Result<()> {
        check(&format!("{path}.duration"), self.duration, 0.0..=5000.0)?;
        if level != Level::Root && self.reduce_motion.is_some() {
            bail!("{path}.reduceMotion is only allowed on animation itself");
        }
        let children = [
            ("bar", &self.bar, Level::Bar),
            ("top", &self.top, Level::Top),
            ("dock", &self.dock, Level::Dock),
            ("hover", &self.hover, Level::Leaf),
            ("press", &self.press, Level::Leaf),
            ("open", &self.open, Level::Leaf),
            ("magnify", &self.magnify, Level::Leaf),
            ("bounce", &self.bounce, Level::Leaf),
            ("attention", &self.attention, Level::Leaf),
        ];
        for (name, child, child_level) in children {
            let Some(child) = child else { continue };
            let allowed = matches!(
                (level, name),
                (Level::Root, "bar" | "hover" | "press" | "open")
                    | (Level::Bar, "top" | "dock" | "hover" | "press" | "open")
                    | (Level::Top, "hover" | "press" | "open")
                    | (
                        Level::Dock,
                        "hover" | "press" | "open" | "magnify" | "bounce" | "attention"
                    )
            );
            if !allowed {
                bail!("{path}.{name} isn't part of the animation tree (see docs/bars.md)");
            }
            child.validate_at(child_level, &format!("{path}.{name}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(json: &str) -> Result<Look> {
        let look: Look = serde_json::from_str(json)?;
        look.validate()?;
        Ok(look)
    }

    #[test]
    fn colors_take_hex_tokens_and_theme_variables() {
        for good in [
            "#fff",
            "#ffffff80",
            "#ABCDEF",
            "accent",
            "system-accent-light-2",
            "var(--glow)",
        ] {
            assert!(ColorRef::parse(good).is_some(), "{good}");
        }
        assert_eq!(ColorRef::parse("#ABCDEF").unwrap().as_str(), "#abcdef");
        for bad in [
            "red",
            "#ggg",
            "#12345",
            "var(--)",
            "var(--a;b)",
            "url(x)",
            "system-accent-light-4",
        ] {
            assert!(ColorRef::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn fills_parse_by_type_and_check_their_ranges() {
        let parsed = look(
            r##"{ "background": { "type": "gradient", "kind": "radial", "stops": ["#ff0080", { "color": "accent", "at": 0.8 }] } }"##,
        )
        .unwrap();
        assert!(
            matches!(parsed.background, Some(Fill::Gradient(Gradient { kind: GradientKind::Radial, angle, .. })) if angle == 180.0)
        );
        let wallpaper = look(r#"{ "background": { "type": "wallpaper" } }"#).unwrap();
        assert_eq!(wallpaper.wallpaper().map(|w| w.interval), Some(10));
        assert!(
            look(r#"{ "background": { "type": "image", "path": "a.png", "fit": "tile", "position": "left 30%" } }"#)
                .is_ok()
        );
        for bad in [
            r#"{ "background": { "type": "glass" } }"#,
            r##"{ "background": { "type": "solid", "color": "#fff", "extra": 1 } }"##,
            r##"{ "background": { "type": "gradient", "stops": ["#fff"] } }"##,
            r#"{ "background": { "type": "image", "path": "a.png", "position": "middle" } }"#,
            r#"{ "background": { "type": "image", "path": "a.png", "brightness": 4 } }"#,
            r#"{ "background": { "type": "wallpaper", "interval": 1 } }"#,
            r#"{ "noise": 0.9 }"#,
            r#"{ "border": { "rest": { "width": 12 } } }"#,
            r#"{ "border": { "focus": {} } }"#,
            r#"{ "glow": { "hover": { "color": "pink" } } }"#,
            r#"{ "items": { "hover": { "scale": 3 } } }"#,
        ] {
            assert!(look(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn border_paints_are_colors_or_gradients() {
        let parsed = look(
            r##"{ "border": { "rest": { "width": 1, "color": { "angle": 90, "stops": ["#f0f", "#0ff"] } }, "hover": { "color": "accent" } } }"##,
        )
        .unwrap();
        assert!(matches!(
            parsed.border.rest,
            Some(BorderStyle {
                color: Some(Paint::Gradient(_)),
                ..
            })
        ));
        assert!(matches!(
            parsed.border.hover,
            Some(BorderStyle {
                color: Some(Paint::Color(_)),
                width: None
            })
        ));
    }

    fn motion(json: &str) -> Result<Motion> {
        let motion: Motion = serde_json::from_str(json)?;
        motion.validate()?;
        Ok(motion)
    }

    #[test]
    fn the_animation_tree_has_a_fixed_shape() {
        let tree = motion(
            r#"{ "duration": 200, "curve": "smooth", "reduceMotion": "system",
                 "bar": { "style": "slide", "dock": { "curve": { "mass": 1, "stiffness": 220, "damping": 20 },
                          "magnify": { "duration": 90 }, "hover": { "enabled": false } } },
                 "hover": { "curve": [0.2, 0.8, 0.2, 1] }, "open": { "style": "pop" } }"#,
        )
        .unwrap();
        let dock = tree.bar.as_ref().and_then(|b| b.dock.as_ref()).unwrap();
        assert!(matches!(dock.curve, Some(Curve::Spring(_))));
        assert!(matches!(
            tree.hover.as_ref().unwrap().curve,
            Some(Curve::Bezier(_))
        ));
        for bad in [
            r#"{ "magnify": {} }"#,
            r#"{ "bar": { "magnify": {} } }"#,
            r#"{ "hover": { "press": {} } }"#,
            r#"{ "bar": { "reduceMotion": "always" } }"#,
            r#"{ "curve": "wobbly" }"#,
            r#"{ "curve": [2, 0, 1, 1] }"#,
            r#"{ "curve": { "mass": 0, "stiffness": 1, "damping": 1 } }"#,
            r#"{ "curve": { "mass": 1, "stiffness": 100 } }"#,
            r#"{ "duration": 9000 }"#,
        ] {
            assert!(motion(bad).is_err(), "accepted {bad}");
        }
    }
}
