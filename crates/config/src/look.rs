//! The user's look (`config.json` › `look`): their layer over the look a wallpaper brings and
//! the style's defaults (plans/native/studio-redesign.md, D2). Studio's Look page edits it.
//!
//! - The desktop half (`style`, `material`, `appearance`, `accent`, `text`, `shape`, and the
//!   per-family `bars`, `widgets`, `menus` overrides) is what Glaze draws.
//! - `windows` is how Windows draws its own windows and pointer. It used to live only in a
//!   theme's `theme.json`; now the theme's section is the layer below this one, so styling no
//!   longer needs a copy of a built-in theme. Each field here is tri-state: absent follows the
//!   wallpaper, `null` keeps Windows' own setting ("As is"), a value sets it.
//!
//! Every field is optional and left out of the file while unset, so a config that never
//! touched the look has no `look` key at all.

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};

use crate::apps::AppsStyle;
use crate::windows::{Backdrop, Color, Corners, Paint, WindowsStyle};

/// The five native styles. Liquid is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Liquid,
    Oneui,
    Solid,
    Fluent,
    Mono,
}

impl Style {
    pub const ALL: [Style; 5] = [
        Style::Liquid,
        Style::Oneui,
        Style::Solid,
        Style::Fluent,
        Style::Mono,
    ];
}

/// Light, dark, or following Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Light,
    Dark,
    /// Match Windows.
    Auto,
}

/// What a surface is made of; only the controls the style uses apply.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Material {
    /// Clear (1) to tinted (0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarity: Option<f32>,
    /// Background blur in DIPs, 0..=32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<f32>,
    /// How strongly the rim bends what's behind (liquid), 0..=1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refraction: Option<f32>,
    /// The specular rim (liquid), 0..=1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<f32>,
    /// The tint colour, or `auto` (from the wallpaper).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<Paint>,
}

impl Material {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `self` over `base`: each field set here wins.
    pub fn over(&self, base: &Material) -> Material {
        Material {
            clarity: self.clarity.or(base.clarity),
            blur: self.blur.or(base.blur),
            refraction: self.refraction.or(base.refraction),
            highlight: self.highlight.or(base.highlight),
            tint: self.tint.or(base.tint),
        }
    }

    fn validate(&self, at: &str) -> Result<()> {
        for (name, value, max) in [
            ("clarity", self.clarity, 1.0),
            ("blur", self.blur, 32.0),
            ("refraction", self.refraction, 1.0),
            ("highlight", self.highlight, 1.0),
        ] {
            if let Some(v) = value
                && !(0.0..=max).contains(&v)
            {
                bail!("{at}.{name} must be between 0 and {max}, got {v}");
            }
        }
        Ok(())
    }
}

/// Light text for dark wallpapers, dark text for light ones, or decided from the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Auto,
    Light,
    Dark,
}

/// The icon font the shell draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    Lucide,
    Fluent,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextLook {
    /// A font family name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Content scale, 0.5..=2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<bool>,
    /// Text drawn straight on the wallpaper.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<Icons>,
    /// An icon pack: a folder under `%APPDATA%\glaze\icons\`, or a bundled one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_pack: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShapeLook {
    /// Corner radius in DIPs, 0..=48.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corners: Option<f32>,
    /// A hairline around every surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<bool>,
}

/// "Different for" one family (bars, widgets, menus and panels): its own style and material
/// over the look's.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FamilyLook {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    #[serde(skip_serializing_if = "Material::is_empty")]
    pub material: Material,
}

/// How apps draw: matching the look's appearance, a fixed mode, or as Windows has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Apps {
    Match,
    Light,
    Dark,
    AsIs,
}

/// The user's Windows styling layer. Each field: absent follows the wallpaper's, `null` keeps
/// Windows' own ("As is"), a value sets it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowsLayer {
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub accent: Option<Option<Paint>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub border: Option<Option<Paint>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub caption: Option<Option<Color>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub caption_text: Option<Option<Color>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub corners: Option<Option<Corners>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<Option<Backdrop>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub accent_on_taskbar: Option<Option<bool>>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub accent_on_title_bars: Option<Option<bool>>,
    /// Light or dark apps (and Windows' own surfaces).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Apps>,
    #[serde(deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub cursors: Option<Option<String>>,
}

/// A field that is present: `null` is `Some(None)` ("As is"), not absent.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

impl WindowsLayer {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A theme's `windows` section, adopted as the user's own (every field it sets).
    pub fn adopt(style: &WindowsStyle) -> Self {
        Self {
            accent: style.accent.map(Some),
            border: style.border.map(Some),
            caption: style.caption.map(Some),
            caption_text: style.caption_text.map(Some),
            corners: style.corners.map(Some),
            backdrop: style.backdrop.map(Some),
            accent_on_taskbar: style.accent_on_taskbar.map(Some),
            accent_on_title_bars: style.accent_on_title_bars.map(Some),
            apps: style
                .dark_mode
                .map(|dark| if dark { Apps::Dark } else { Apps::Light }),
            cursors: style.cursors.clone().map(Some),
        }
    }
}

/// The user's look. See the module docs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    #[serde(skip_serializing_if = "Material::is_empty")]
    pub material: Material,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Scheme>,
    /// One accent for everything: a colour, or `auto` (from the wallpaper).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<Paint>,
    #[serde(skip_serializing_if = "is_default")]
    pub text: TextLook,
    #[serde(skip_serializing_if = "is_default")]
    pub shape: ShapeLook,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<FamilyLook>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widgets: Option<FamilyLook>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menus: Option<FamilyLook>,
    #[serde(skip_serializing_if = "WindowsLayer::is_empty")]
    pub windows: WindowsLayer,
    /// The app styler's rules over the wallpaper's `apps` (experimental; see `crate::apps`).
    #[serde(skip_serializing_if = "AppsStyle::is_empty")]
    pub apps: AppsStyle,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl Look {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        self.material.validate("look.material")?;
        for (name, family) in [
            ("bars", &self.bars),
            ("widgets", &self.widgets),
            ("menus", &self.menus),
        ] {
            if let Some(family) = family {
                family.material.validate(&format!("look.{name}.material"))?;
            }
        }
        if let Some(size) = self.text.size
            && !(0.5..=2.0).contains(&size)
        {
            bail!("look.text.size must be between 0.5 and 2, got {size}");
        }
        if let Some(corners) = self.shape.corners
            && !(0.0..=48.0).contains(&corners)
        {
            bail!("look.shape.corners must be between 0 and 48, got {corners}");
        }
        Ok(())
    }

    /// The Windows styling in effect: this layer over the wallpaper's `theme` section. `None`
    /// when nothing is styled (Windows is left as it is).
    pub fn windows_style(&self, theme: Option<&WindowsStyle>) -> Option<WindowsStyle> {
        let base = theme.cloned().unwrap_or_default();
        let w = &self.windows;
        // absent: the theme's; null: as is; a value: the user's.
        fn pick<T: Copy>(user: &Option<Option<T>>, theme: Option<T>) -> Option<T> {
            user.unwrap_or(theme)
        }
        let dark_mode = match w.apps {
            None => base.dark_mode,
            Some(Apps::AsIs) => None,
            Some(Apps::Light) => Some(false),
            Some(Apps::Dark) => Some(true),
            Some(Apps::Match) => match self.appearance {
                Some(Scheme::Light) => Some(false),
                Some(Scheme::Dark) => Some(true),
                Some(Scheme::Auto) | None => None,
            },
        };
        let style = WindowsStyle {
            accent: pick(&w.accent, base.accent),
            border: pick(&w.border, base.border),
            caption: pick(&w.caption, base.caption),
            caption_text: pick(&w.caption_text, base.caption_text),
            corners: pick(&w.corners, base.corners),
            backdrop: pick(&w.backdrop, base.backdrop),
            dark_mode,
            accent_on_taskbar: pick(&w.accent_on_taskbar, base.accent_on_taskbar),
            accent_on_title_bars: pick(&w.accent_on_title_bars, base.accent_on_title_bars),
            cursors: match &w.cursors {
                Some(value) => value.clone(),
                None => base.cursors,
            },
        };
        (style != WindowsStyle::default()).then_some(style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> WindowsStyle {
        WindowsStyle {
            accent: Some(Paint::Auto),
            corners: Some(Corners::Small),
            dark_mode: Some(true),
            cursors: Some("glaze-dark".into()),
            ..Default::default()
        }
    }

    #[test]
    fn absent_follows_the_wallpaper_null_keeps_windows_and_a_value_wins() {
        let look: Look = serde_json::from_str(
            r##"{ "windows": { "accent": null, "corners": "square", "border": "#112233" } }"##,
        )
        .unwrap();
        let style = look.windows_style(Some(&theme())).unwrap();
        assert_eq!(style.accent, None, "null: as Windows has it");
        assert_eq!(style.corners, Some(Corners::Square), "the user's value");
        assert_eq!(style.border, Some(Paint::Color(Color([0x11, 0x22, 0x33]))));
        assert_eq!(style.dark_mode, Some(true), "absent: the wallpaper's");
        assert_eq!(style.cursors.as_deref(), Some("glaze-dark"));
        // Round trip keeps the three states apart.
        let text = serde_json::to_string(&look).unwrap();
        assert_eq!(serde_json::from_str::<Look>(&text).unwrap(), look);
        assert!(text.contains(r#""accent":null"#));
    }

    #[test]
    fn apps_match_follows_the_looks_appearance_and_nothing_styled_is_none() {
        let mut look = Look {
            appearance: Some(Scheme::Light),
            ..Default::default()
        };
        look.windows.apps = Some(Apps::Match);
        assert_eq!(look.windows_style(None).unwrap().dark_mode, Some(false));
        look.appearance = Some(Scheme::Auto);
        assert_eq!(
            look.windows_style(None),
            None,
            "auto leaves Windows' mode alone"
        );
        assert_eq!(Look::default().windows_style(None), None);
        assert_eq!(Look::default().windows_style(Some(&theme())), Some(theme()));
    }

    #[test]
    fn an_empty_look_is_left_out_and_ranges_are_checked() {
        assert_eq!(serde_json::to_string(&Look::default()).unwrap(), "{}");
        let bad: Look = serde_json::from_str(r#"{ "material": { "clarity": 2 } }"#).unwrap();
        assert!(bad.validate().is_err());
        assert!(serde_json::from_str::<Look>(r#"{ "style": "neon" }"#).is_err());
    }

    #[test]
    fn accent_visibility_has_independent_absent_null_and_boolean_layers() {
        let theme = WindowsStyle {
            accent_on_taskbar: Some(true),
            accent_on_title_bars: Some(true),
            ..Default::default()
        };
        let look: Look = serde_json::from_str(
            r#"{"windows":{"accent_on_taskbar":null,"accent_on_title_bars":false}}"#,
        )
        .unwrap();
        let resolved = look.windows_style(Some(&theme)).unwrap();
        assert_eq!(resolved.accent_on_taskbar, None);
        assert_eq!(resolved.accent_on_title_bars, Some(false));
        assert_eq!(
            Look::default().windows_style(Some(&theme)),
            Some(theme.clone())
        );
        assert_eq!(
            WindowsLayer::adopt(&theme).accent_on_taskbar,
            Some(Some(true))
        );
        assert_eq!(
            serde_json::from_str::<Look>(&serde_json::to_string(&look).unwrap()).unwrap(),
            look
        );
        assert!(serde_json::from_str::<Look>(r#"{"windows":{"accent_on_taskbar":1}}"#).is_err());
    }
}
