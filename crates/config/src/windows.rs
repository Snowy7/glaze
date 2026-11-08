//! Colours and native Windows styling. Themes and the user's look share these types (themes
//! re-export them from `glaze_theme`), so a theme's `windows` section and the user's layer
//! over it (`look.windows`, D2) read and write the same JSON.

use serde::{Deserialize, Serialize};

/// An sRGB color, written `"#rrggbb"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// A color, or one derived from the wallpaper (`"auto"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Picked from the wallpaper's palette.
    Auto,
    Color(Color),
}

impl Serialize for Paint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Paint::Auto => serializer.serialize_str("auto"),
            Paint::Color(color) => color.serialize(serializer),
        }
    }
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsStyle {
    /// System accent color (taskbar, Start, highlights).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<Paint>,
    /// Window border color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Paint>,
    /// Title bar background color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<Color>,
    /// Title bar text color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_text: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<Corners>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<Backdrop>,
    /// Force dark (`true`) or light (`false`) mode for apps and the system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark_mode: Option<bool>,
    /// Show the accent on Start and the taskbar (`Personalize\\ColorPrevalence`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_on_taskbar: Option<bool>,
    /// Show the accent on title bars and borders (`DWM\\ColorPrevalence`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_on_title_bars: Option<bool>,
    /// A mouse cursor set: the name of a built-in or user set (`glaze-dark`,
    /// `glaze-light`, a folder under `%APPDATA%\glaze\cursors`), or a
    /// folder of `.cur`/`.ani` files (absolute, or relative to the theme).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursors: Option<String>,
}

impl WindowsStyle {
    /// Each field set here overrides the corresponding style default.
    pub fn over(&self, base: &Self) -> Self {
        Self {
            accent: self.accent.or(base.accent),
            border: self.border.or(base.border),
            caption: self.caption.or(base.caption),
            caption_text: self.caption_text.or(base.caption_text),
            corners: self.corners.or(base.corners),
            backdrop: self.backdrop.or(base.backdrop),
            dark_mode: self.dark_mode.or(base.dark_mode),
            accent_on_taskbar: self.accent_on_taskbar.or(base.accent_on_taskbar),
            accent_on_title_bars: self.accent_on_title_bars.or(base.accent_on_title_bars),
            cursors: self.cursors.clone().or_else(|| base.cursors.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    Default,
    Round,
    Small,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    Auto,
    None,
    Mica,
    Acrylic,
    Tabbed,
}
