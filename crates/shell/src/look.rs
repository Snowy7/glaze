//! The menu's tokens from the active theme: the built-in look under the theme's `bars`
//! appearance (the variables the bars use), with `accent: "auto"` taken from the wallpaper
//! palette. `blur` has no counterpart yet (the menu is a nearly opaque tint until N3's glass),
//! and `surface: none` still draws a panel.

use mypaper_theme::{Appearance, Color, LoadedTheme, Paint, TextTone};
use mypaper_tweaks::palette;
use mypaper_ui::{Colors, Material, Rgba, Text, TextStyle, Tokens, icons};

/// The tokens for theme `theme_name` (a name or a folder), with the installed icon font.
pub fn tokens(theme_name: &str, text: &Text) -> Tokens {
    let loaded = mypaper_theme::locate(Some(theme_name))
        .and_then(LoadedTheme::load)
        .inspect_err(|e| log::debug!("desktop menu look: {e:#}"))
        .ok();
    let appearance = loaded
        .as_ref()
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|skin| skin.appearance.clone())
        .unwrap_or_default();
    let auto_accent = loaded.as_ref().and_then(|t| {
        palette::read()
            .filter(|snapshot| snapshot.is_for(&t.dir))
            .map(|snapshot| snapshot.palette.accent)
    });
    Tokens {
        icons: icons::system_family(text),
        ..resolve(&appearance, auto_accent)
    }
}

/// The built-in tokens with what `theme` sets.
fn resolve(theme: &Appearance, auto_accent: Option<Color>) -> Tokens {
    let base = Tokens::default();
    let dark = theme.text.is_none_or(|tone| tone == TextTone::Light);
    let accent = match theme.accent {
        Some(Paint::Color(color)) => color.0,
        Some(Paint::Auto) | None => auto_accent.map_or([0xff; 3], |c| c.0),
    };
    let tint = theme
        .tint
        .map_or(base.material.tint, |c| Rgba::rgb8(c.0, 1.0))
        .with_alpha(theme.opacity.unwrap_or(base.material.tint.a));
    let font = theme.font.as_deref().map_or(base.font.clone(), |css| {
        TextStyle::new(&font_family(css), base.font.size)
    });
    Tokens {
        font,
        colors: Colors::new(dark, accent),
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

    #[test]
    fn without_a_theme_look_the_defaults_apply() {
        assert_eq!(resolve(&Appearance::default(), None), Tokens::default());
    }

    #[test]
    fn the_themes_bars_appearance_wins_and_auto_accent_follows_the_palette() {
        let theme = Appearance {
            tint: Some(Color([0xf0, 0xf0, 0xf2])),
            text: Some(TextTone::Dark),
            radius: Some(4.0),
            accent: Some(Paint::Auto),
            font: Some("Inter".into()),
            shadow: Some(false),
            ..Default::default()
        };
        let tokens = resolve(&theme, Some(Color([0x19, 0xcd, 0x86])));
        assert_eq!(
            tokens.colors,
            Colors::new(false, [0x19, 0xcd, 0x86]),
            "dark text, a light menu"
        );
        assert_eq!(
            tokens.material.tint,
            Rgba::rgb8([0xf0, 0xf0, 0xf2], 0.94),
            "default opacity"
        );
        assert_eq!(tokens.radius, 4.0);
        assert_eq!(&*tokens.font.family, "Inter");
        assert!(!tokens.shadow);
        // `auto` without a palette falls back to the default accent.
        assert_eq!(resolve(&theme, None).colors, Colors::new(false, [0xff; 3]));
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
