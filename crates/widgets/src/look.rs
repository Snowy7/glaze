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
static WARNED_CSS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
use mypaper_theme::{
    Appearance, Color, Liquid, LoadedTheme, Paint, Surface, TextTone, WidgetStyle,
};
use serde_json::{Map, Value};

/// The accent when the palette is missing.
const FALLBACK_ACCENT: Color = Color([0xff, 0xff, 0xff]);

/// The built-in tint: a calm slate, a little lighter than most dark wallpapers so the tile reads
/// as frosted glass rather than a hole.
pub const DEFAULT_TINT: Color = Color([0x34, 0x3a, 0x42]);

/// The built-in layer, every field set: the calm desktop-widget look (a quiet translucent tile,
/// a hairline, generous radius).
pub fn defaults() -> Appearance {
    Appearance {
        surface: Some(Surface::Glass),
        tint: Some(DEFAULT_TINT),
        opacity: Some(0.46),
        blur: Some(12.0),
        radius: Some(22.0),
        border: Some(true),
        text: Some(TextTone::Light),
        accent: Some(Paint::Color(FALLBACK_ACCENT)),
        font: Some("Segoe UI Variable".to_owned()),
        scale: Some(1.0),
        shadow: Some(true),
        style: Some(WidgetStyle::Minimal),
    }
}

/// The built-in liquid glass knobs, under the theme's `widgets.glass`: a clear lens bent at its
/// rim, one thin specular edge, a whisper of tint and 12 px of frost.
pub fn liquid_defaults() -> Liquid {
    Liquid {
        intensity: Some(1.0),
        refraction: Some(0.6),
        highlight: Some(0.55),
        tint: Some(0.06),
        blur: Some(12.0),
    }
}

/// The active theme's widget skin: its appearance layer, style sheet text and glass knobs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skin {
    pub appearance: Appearance,
    pub css: Option<String>,
    pub glass: Liquid,
}

/// The liquid glass knobs in effect: the theme's over the built-in ones, every field set.
pub fn liquid(skin: &Skin) -> Liquid {
    skin.glass.over(&liquid_defaults())
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
    if skin.css.is_some() && !WARNED_CSS.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::warn!("widget CSS is no longer supported; use native appearance tokens");
    }
    Skin {
        appearance: skin.appearance.clone().unwrap_or_default(),
        css: None,
        glass: skin.glass.unwrap_or_default(),
    }
}

/// The layers below the user's: built-in defaults, then the theme's skin.
pub fn base(skin: &Skin) -> Appearance {
    skin.appearance.over(&defaults())
}

/// What the default tint becomes with dark text (the near-black default would be dark on dark).
const DARK_TEXT_TINT: Color = Color([0xf2, 0xf2, 0xf2]);
/// What the default accent (white) becomes with dark text: the ink, since white would vanish.
const DARK_TEXT_ACCENT: Color = Color([0x0a, 0x0a, 0x0c]);

/// What a widget gets: `own` over `user` over `base`, every field set, `auto` replaced by
/// `accent` (the palette's accent; [`accent`] picks it). With dark text, a tint or accent that is
/// still the built-in default becomes the dark-text default (`#f2f2f2`, `#0a0a0c`): the host sends
/// final values, so the widget kit has nothing to flip. (The shell mirrors this in
/// `crates/shell/src/widgets` for native views.)
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
    if merged.text == Some(TextTone::Dark) {
        let default = defaults();
        if merged.tint == default.tint {
            merged.tint = Some(DARK_TEXT_TINT);
        }
        if merged.accent == default.accent {
            merged.accent = Some(Paint::Color(DARK_TEXT_ACCENT));
        }
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
        assert_eq!(json.len(), 12, "{json:?}");
        assert_eq!(json["surface"], "glass");
        assert_eq!(json["style"], "minimal");
        assert_eq!(json["opacity"], 0.46);
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
            glass: Liquid::default(),
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
        assert_eq!(got.blur, Some(12.0), "untouched fields keep the default");
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
    fn dark_text_flips_the_default_tint_and_accent_but_not_chosen_ones() {
        let dark = Appearance {
            text: Some(TextTone::Dark),
            ..Default::default()
        };
        let none = Appearance::default();
        let got = resolve(&dark, &none, &defaults(), color("#14d17f"));
        assert_eq!(got.tint, Some(color("#f2f2f2")));
        assert_eq!(got.accent, Some(Paint::Color(color("#0a0a0c"))));

        // Chosen colors stay, an auto accent is the palette's, and light text changes nothing.
        let chosen = Appearance {
            text: Some(TextTone::Dark),
            tint: Some(color("#303040")),
            accent: Some(Paint::Auto),
            ..Default::default()
        };
        let got = resolve(&chosen, &none, &defaults(), color("#14d17f"));
        assert_eq!(got.tint, Some(color("#303040")));
        assert_eq!(got.accent, Some(Paint::Color(color("#14d17f"))));
        let light = resolve(&none, &none, &defaults(), color("#14d17f"));
        assert_eq!(light.tint, Some(DEFAULT_TINT));
        assert_eq!(light.accent, Some(Paint::Color(color("#ffffff"))));

        // A theme that sets the tint keeps it under dark text too.
        let themed = Appearance {
            tint: Some(color("#223344")),
            ..Default::default()
        };
        let got = resolve(&dark, &none, &themed.over(&defaults()), color("#14d17f"));
        assert_eq!(got.tint, Some(color("#223344")));
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
        assert_eq!(
            edit(serde_json::json!({ "style": "neon" })).unwrap().style,
            Some(WidgetStyle::Neon)
        );
        assert!(
            edit(serde_json::json!({ "style": "glossy" })).is_err(),
            "unknown style"
        );
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
                  "widgets": { "appearance": { "surface": "glass", "accent": "auto" }, "css": "w.css",
                               "glass": { "refraction": 0.3 } } }"##,
        )
        .unwrap();
        std::fs::write(dir.join("w.css"), "html { --x: 1 }").unwrap();
        let skin = load_skin(Some(&dir));
        assert_eq!(skin.appearance.surface, Some(Surface::Glass));
        assert_eq!(skin.appearance.accent, Some(Paint::Auto));
        assert_eq!(skin.css, None);
        let glass = liquid(&skin);
        assert_eq!(glass.refraction, Some(0.3), "the theme's knob");
        assert_eq!(
            glass.intensity,
            liquid_defaults().intensity,
            "the rest built in"
        );

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
