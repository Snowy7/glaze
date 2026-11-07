//! The menu's tokens from the active theme: the built-in look under the theme's `bars`
//! appearance (the variables the bars use). Highlights are a neutral wash of the text colour
//! (the monochrome look), so the theme's accent plays no part. `blur` has no counterpart (the
//! menu is a nearly opaque tint unless the engine draws glass), and `surface: none` still
//! draws a panel.

use mypaper_theme::{Appearance, LoadedTheme, TextTone};
use mypaper_ui::{Colors, Material, Rgba, Text, TextStyle, Tokens, icons};

#[path = "look_settings.rs"]
pub mod settings;

/// The tokens for theme `theme_name` (a name or a folder), with the installed icon font.
pub fn tokens(theme_name: &str, text: &Text) -> Tokens {
    let appearance = mypaper_theme::locate(Some(theme_name))
        .and_then(LoadedTheme::load)
        .inspect_err(|e| log::debug!("desktop menu look: {e:#}"))
        .ok()
        .and_then(|t| t.theme.bars.and_then(|skin| skin.appearance))
        .unwrap_or_default();
    Tokens {
        icons: icons::system_family(text),
        ..resolve(&appearance)
    }
}

/// The built-in tokens with what `theme` sets.
fn resolve(theme: &Appearance) -> Tokens {
    let base = Tokens::default();
    let dark = theme.text.is_none_or(|tone| tone == TextTone::Light);
    let tint = theme
        .tint
        .map_or(base.material.tint, |c| Rgba::rgb8(c.0, 1.0))
        .with_alpha(theme.opacity.unwrap_or(base.material.tint.a));
    let font = theme.font.as_deref().map_or(base.font.clone(), |css| {
        TextStyle::new(&font_family(css), base.font.size)
    });
    Tokens {
        font,
        colors: Colors::neutral(dark),
        material: Material { tint },
        radius: theme.radius.unwrap_or(base.radius),
        border: theme.border.unwrap_or(base.border),
        shadow: theme.shadow.unwrap_or(base.shadow),
        scale: theme.scale.unwrap_or(base.scale),
        ..base
    }
}

/// The first usable family of a CSS `font-family` list.
fn font_family(css: &str) -> String {
    const GENERIC: [&str; 6] = [
        "system-ui",
        "sans-serif",
        "serif",
        "monospace",
        "ui-sans-serif",
        "cursive",
    ];
    css.split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).trim())
        .find(|f| !f.is_empty() && !GENERIC.iter().any(|g| g.eq_ignore_ascii_case(f)))
        .unwrap_or("Segoe UI")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_theme::{Color, Paint};

    #[test]
    fn without_a_theme_look_the_defaults_apply() {
        assert_eq!(resolve(&Appearance::default()), Tokens::default());
    }

    #[test]
    fn the_themes_bars_appearance_wins_and_highlights_stay_neutral() {
        let theme = Appearance {
            tint: Some(Color([0xf0, 0xf0, 0xf2])),
            text: Some(TextTone::Dark),
            radius: Some(4.0),
            accent: Some(Paint::Auto),
            font: Some("Inter".into()),
            shadow: Some(false),
            ..Default::default()
        };
        let tokens = resolve(&theme);
        assert_eq!(
            tokens.colors,
            Colors::neutral(false),
            "dark text, a light menu"
        );
        assert_eq!(tokens.colors.hover, tokens.colors.text.with_alpha(0.10));
        assert_eq!(
            tokens.material.tint,
            Rgba::rgb8([0xf0, 0xf0, 0xf2], 0.94),
            "default opacity"
        );
        assert_eq!(tokens.radius, 4.0);
        assert_eq!(&*tokens.font.family, "Inter");
        assert!(!tokens.shadow);
    }

    #[test]
    fn the_first_concrete_font_family_wins() {
        assert_eq!(
            font_family(r#""Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif"#),
            "Segoe UI Variable Text"
        );
        assert_eq!(font_family("system-ui, 'Cascadia Code'"), "Cascadia Code");
        assert_eq!(font_family("sans-serif"), "Segoe UI");
        assert_eq!(font_family(""), "Segoe UI");
        assert_eq!(font_family("  Inter  ,serif"), "Inter");
    }
}
