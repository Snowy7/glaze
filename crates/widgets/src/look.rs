//! Widget appearance (docs/widgets.md, "Appearance"): the layers, how they merge into the plain
//! values widgets receive, and the active theme's widget skin.
//!
//! Layers, lowest first: the built-in [`defaults`], the theme's `widgets` skin, the user's
//! `appearance` in `widgets.json`, and an instance's own `appearance`. `accent: auto` is resolved
//! from the wallpaper palette here, so widgets only ever see colors.
//!
//! Pure apart from reading the theme folder: no Win32.

use std::path::Path;

use anyhow::{Context, Result};
use mypaper_theme::{Appearance, Color, LoadedTheme, Paint, Surface, TextTone};
use serde_json::{Map, Value};

/// A style sheet bigger than this is ignored (it is sent to every widget on every change).
const MAX_CSS_BYTES: u64 = 512 * 1024;

/// The accent when the palette is missing.
const FALLBACK_ACCENT: Color = Color([0xff, 0xff, 0xff]);

/// The built-in layer: every field set.
pub fn defaults() -> Appearance {
    Appearance {
        surface: Some(Surface::None),
        tint: Some(Color([0x10, 0x10, 0x10])),
        opacity: Some(0.55),
        blur: Some(24.0),
        radius: Some(18.0),
        border: Some(true),
        text: Some(TextTone::Light),
        accent: Some(Paint::Color(FALLBACK_ACCENT)),
        font: Some("Segoe UI Variable".to_owned()),
        scale: Some(1.0),
        shadow: Some(true),
    }
}

/// The active theme's widget skin: its appearance layer and style sheet text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skin {
    pub appearance: Appearance,
    pub css: Option<String>,
}

/// Reads `widgets` from the theme in `dir`. Anything wrong (no theme, a broken
/// `theme.json`, a missing style sheet) degrades to "no skin", with a log line.
pub fn load_skin(dir: Option<&Path>) -> Skin {
    let Some(dir) = dir else {
        return Skin::default();
    };
    let theme = match LoadedTheme::load(dir) {
        Ok(theme) => theme,
        Err(e) => {
            log::warn!("no widget skin from {}: {e:#}", dir.display());
            return Skin::default();
        }
    };
    let Some(skin) = theme.theme.widgets.as_ref() else {
        return Skin::default();
    };
    let css = skin.css.as_ref().and_then(|css| {
        read_css(&theme.asset(css))
            .inspect_err(|e| log::warn!("theme style sheet: {e:#}"))
            .ok()
    });
    Skin {
        appearance: skin.appearance.clone().unwrap_or_default(),
        css,
    }
}

fn read_css(path: &Path) -> Result<String> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .len();
    anyhow::ensure!(
        size <= MAX_CSS_BYTES,
        "{} is {size} bytes (the limit is {MAX_CSS_BYTES})",
        path.display()
    );
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// The layers below the user's: built-in defaults, then the theme's skin.
pub fn base(skin: &Skin) -> Appearance {
    skin.appearance.over(&defaults())
}

/// What a widget gets: `own` over `user` over `base`, every field set, `auto` replaced by
/// `accent` (the palette's accent; [`accent`] picks it).
pub fn resolve(
    own: &Appearance,
    user: &Appearance,
    base: &Appearance,
    accent: Color,
) -> Appearance {
    let mut merged = own.over(&user.over(base));
    if merged.accent == Some(Paint::Auto) {
        merged.accent = Some(Paint::Color(accent));
    }
    merged
}

/// The color `accent: auto` stands for: the wallpaper palette's accent, else white.
pub fn accent(palette_accent: Option<&str>) -> Color {
    palette_accent
        .and_then(Color::parse)
        .unwrap_or(FALLBACK_ACCENT)
}

/// A layer as a JSON object holding only the fields it sets. Numbers are the shortest
/// text that reads back as the same `f32` (so `0.55`, not `0.550000011920929`).
pub fn json_of(appearance: &Appearance) -> Map<String, Value> {
    let Ok(Value::Object(map)) = serde_json::to_value(appearance) else {
        return Map::new();
    };
    map.into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
            let value = match value.as_f64() {
                Some(n) => short_number(n as f32),
                None => value,
            };
            (key, value)
        })
        .collect()
}

fn short_number(n: f32) -> Value {
    n.to_string()
        .parse::<serde_json::Number>()
        .map_or(Value::Null, Value::Number)
}

/// `layer` with `patch` applied: a field with a value is set, one that is `null` goes back to
/// "unset" (the layer below shows through). The result must be a valid appearance.
pub fn patch(layer: &Appearance, patch: &Map<String, Value>) -> Result<Appearance> {
    let mut fields = json_of(layer);
    for (key, value) in patch {
        if value.is_null() {
            fields.remove(key);
        } else {
            fields.insert(key.clone(), value.clone());
        }
    }
    let next: Appearance =
        serde_json::from_value(Value::Object(fields)).context("invalid appearance")?;
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(hex: &str) -> Color {
        Color::parse(hex).unwrap()
    }

    #[test]
    fn defaults_set_every_field_and_are_valid() {
        let all = defaults();
        assert!(all.validate().is_ok());
        // `over` an empty layer keeps every field: nothing is None.
        let json = json_of(&all);
        assert_eq!(json.len(), 11, "{json:?}");
        assert_eq!(json["surface"], "none");
        assert_eq!(json["opacity"], 0.55);
        assert_eq!(json["accent"], "#ffffff");
    }

    #[test]
    fn layers_stack_defaults_theme_user_instance() {
        let skin = Skin {
            appearance: Appearance {
                surface: Some(Surface::Glass),
                opacity: Some(0.4),
                radius: Some(10.0),
                text: Some(TextTone::Dark),
                ..Default::default()
            },
            css: None,
        };
        let user = Appearance {
            opacity: Some(0.9),
            radius: Some(30.0),
            ..Default::default()
        };
        let own = Appearance {
            radius: Some(2.0),
            ..Default::default()
        };
        let got = resolve(&own, &user, &base(&skin), color("#14d17f"));
        assert_eq!(got.radius, Some(2.0), "instance beats user");
        assert_eq!(got.opacity, Some(0.9), "user beats theme");
        assert_eq!(got.surface, Some(Surface::Glass), "theme beats defaults");
        assert_eq!(got.text, Some(TextTone::Dark));
        assert_eq!(got.blur, Some(24.0), "untouched fields keep the default");
    }

    #[test]
    fn auto_accent_comes_from_the_palette_or_falls_back_to_white() {
        let auto = Appearance {
            accent: Some(Paint::Auto),
            ..Default::default()
        };
        let none = Appearance::default();
        let base = defaults();
        let from_palette = resolve(&none, &auto, &base, accent(Some("#14d17f")));
        assert_eq!(from_palette.accent, Some(Paint::Color(color("#14d17f"))));
        let no_palette = resolve(&none, &auto, &base, accent(None));
        assert_eq!(no_palette.accent, Some(Paint::Color(color("#ffffff"))));
        let broken = resolve(&none, &auto, &base, accent(Some("green")));
        assert_eq!(broken.accent, Some(Paint::Color(color("#ffffff"))));
        // A fixed color is never replaced.
        let fixed = Appearance {
            accent: Some(Paint::Color(color("#ff0000"))),
            ..Default::default()
        };
        let got = resolve(&fixed, &none, &base, color("#14d17f"));
        assert_eq!(got.accent, Some(Paint::Color(color("#ff0000"))));
    }

    #[test]
    fn json_holds_only_set_fields_with_clean_numbers() {
        let layer = Appearance {
            opacity: Some(0.35),
            accent: Some(Paint::Auto),
            ..Default::default()
        };
        let json = Value::Object(json_of(&layer));
        assert_eq!(json.to_string(), r#"{"accent":"auto","opacity":0.35}"#);
    }

    #[test]
    fn patches_set_and_clear_fields_and_reject_bad_ones() {
        let layer = Appearance {
            blur: Some(10.0),
            border: Some(false),
            ..Default::default()
        };
        let edit = |value: Value| {
            let Value::Object(map) = value else {
                panic!("not an object")
            };
            patch(&layer, &map)
        };
        let next =
            edit(serde_json::json!({ "blur": null, "radius": 12, "surface": "solid" })).unwrap();
        assert_eq!(next.blur, None, "null clears");
        assert_eq!(next.border, Some(false), "untouched fields stay");
        assert_eq!(next.radius, Some(12.0));
        assert_eq!(next.surface, Some(Surface::Solid));
        assert!(
            edit(serde_json::json!({ "opacity": 1.5 })).is_err(),
            "range"
        );
        assert!(edit(serde_json::json!({ "tint": "red" })).is_err(), "color");
        assert!(
            edit(serde_json::json!({ "glow": 1 })).is_err(),
            "unknown field"
        );
    }

    #[test]
    fn a_themes_widget_skin_loads_its_style_sheet() {
        let dir = crate::files::temp_dir("skin");
        std::fs::write(
            dir.join("theme.json"),
            r##"{ "name": "T", "wallpaper": { "kind": "image", "source": "x.jpg" },
                  "widgets": { "appearance": { "surface": "glass", "accent": "auto" }, "css": "w.css" } }"##,
        )
        .unwrap();
        std::fs::write(dir.join("w.css"), "html { --x: 1 }").unwrap();
        let skin = load_skin(Some(&dir));
        assert_eq!(skin.appearance.surface, Some(Surface::Glass));
        assert_eq!(skin.appearance.accent, Some(Paint::Auto));
        assert_eq!(skin.css.as_deref(), Some("html { --x: 1 }"));

        // A style sheet that is missing costs only the sheet.
        std::fs::remove_file(dir.join("w.css")).unwrap();
        let skin = load_skin(Some(&dir));
        assert_eq!(skin.appearance.surface, Some(Surface::Glass));
        assert_eq!(skin.css, None);

        // A theme without a skin, or none at all, is the plain defaults.
        std::fs::write(
            dir.join("theme.json"),
            r#"{ "name": "T", "wallpaper": { "kind": "image", "source": "x.jpg" } }"#,
        )
        .unwrap();
        assert_eq!(load_skin(Some(&dir)), Skin::default());
        assert_eq!(load_skin(None), Skin::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
