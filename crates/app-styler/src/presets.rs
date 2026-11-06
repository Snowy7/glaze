//! Built-in presets and rule resolution: preset < theme's `apps` < the user's `look.apps`, with
//! `$accent` / `$dominant` colour tokens filled in from the user's look or the wallpaper.
//!
//! Presets are data (`presets/*.json`), written for mypaper's looks:
//!
//! - `calm`: quiet and flat; neutral translucent cards, no borders, a grey selection mark.
//! - `glass`: Explorer's and Settings' backdrop (Mica, or acrylic with `look.windows.backdrop`)
//!   shows through: transparent bars, frosted cards, round corners, the accent as the mark.
//! - `compact`: tighter cards, rows and text.

use mypaper_config::{AppRules, AppsStyle};

/// The preset names `apps.preset` accepts (plus `"none"`).
pub const NAMES: [&str; 3] = ["calm", "glass", "compact"];

/// A preset by name. `None` for `"none"` or an unknown name.
pub fn get(name: &str) -> Option<AppsStyle> {
    let json = match name {
        "calm" => include_str!("../presets/calm.json"),
        "glass" => include_str!("../presets/glass.json"),
        "compact" => include_str!("../presets/compact.json"),
        _ => return None,
    };
    serde_json::from_str(json).ok()
}

/// Colours a rule value may name as `$accent` or `$dominant`, optionally with a hex alpha
/// (`$accent/40` is the accent at 0x40).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Colors {
    pub accent: Option<[u8; 3]>,
    pub dominant: Option<[u8; 3]>,
}

/// One app's rules in effect, ready for the TAP: layered, `null`s dropped, tokens filled in. A
/// property whose token can't be resolved (no palette yet) is left out.
pub fn resolve(app: &str, theme: Option<&AppsStyle>, user: &AppsStyle, colors: Colors) -> AppRules {
    let theme = theme.cloned().unwrap_or_default();
    let layered = user.over(&theme);
    let preset = layered.preset.as_deref().and_then(get).unwrap_or_default();
    let all = layered.over(&preset);
    let mut rules = all.app(app).map(AppRules::settled).unwrap_or_default();
    for properties in rules.rules.values_mut() {
        for value in properties.values_mut() {
            *value = value.as_deref().and_then(|v| fill(v, colors));
        }
        properties.retain(|_, v| v.is_some());
    }
    rules.rules.retain(|_, p| !p.is_empty());
    for value in rules.resources.values_mut() {
        *value = value.as_deref().and_then(|v| fill(v, colors));
    }
    rules.resources.retain(|_, v| v.is_some());
    rules
}

/// `$accent`, `$accent/40`, `$dominant`... to `#rrggbb` / `#aarrggbb`; anything else as is.
fn fill(value: &str, colors: Colors) -> Option<String> {
    let Some(token) = value.strip_prefix('$') else {
        return Some(value.to_owned());
    };
    let (name, alpha) = match token.split_once('/') {
        Some((name, alpha)) => (name, Some(u8::from_str_radix(alpha, 16).ok()?)),
        None => (token, None),
    };
    let [r, g, b] = match name {
        "accent" => colors.accent?,
        "dominant" => colors.dominant?,
        _ => return None,
    };
    Some(match alpha {
        Some(a) => format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
        None => format!("#{r:02x}{g:02x}{b:02x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Selector;

    fn style(json: &str) -> AppsStyle {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn every_preset_parses_and_every_selector_is_valid() {
        for name in NAMES {
            let preset = get(name).unwrap_or_else(|| panic!("{name} parses"));
            for app in mypaper_config::apps::APPS {
                let rules = preset.app(app).unwrap();
                assert!(!rules.rules.is_empty(), "{name} styles {app}");
                for selector in rules.rules.keys() {
                    assert!(selector.parse::<Selector>().is_ok(), "{name}: {selector}");
                }
            }
        }
        assert_eq!(get("none"), None);
    }

    #[test]
    fn preset_then_theme_then_user_and_tokens_fill_in() {
        let theme = style(
            r##"{ "preset": "glass", "settings": { "rules": {
                "Grid#RootPageGrid": { "Background": "#202020" } } } }"##,
        );
        let user = style(
            r##"{ "settings": { "rules": {
                "Grid#RootPageGrid": { "Background": "$dominant/80" },
                "GridViewItem": { "CornerRadius": null } } } }"##,
        );
        let colors = Colors {
            accent: Some([0x19, 0xcd, 0x86]),
            dominant: Some([0x1a, 0x71, 0x4f]),
        };
        let rules = resolve("settings", Some(&theme), &user, colors);
        let root = &rules.rules["Grid#RootPageGrid"];
        assert_eq!(
            root["Background"].as_deref(),
            Some("#801a714f"),
            "user over theme over preset"
        );
        assert!(
            !rules.rules.contains_key("GridViewItem"),
            "null took the preset's away"
        );
        let mark = &rules.rules["Rectangle#SelectionIndicator"];
        assert_eq!(
            mark["Fill"].as_deref(),
            Some("#19cd86"),
            "the preset's $accent"
        );

        // No palette yet: the token's property is left out, the rest stays.
        let rules = resolve("settings", Some(&theme), &user, Colors::default());
        assert!(!rules.rules.contains_key("Rectangle#SelectionIndicator"));
        assert!(
            rules
                .rules
                .contains_key("ButtonEntityItem > Button#ContainerButton")
        );
    }

    #[test]
    fn no_preset_and_nothing_set_styles_nothing() {
        let rules = resolve("explorer", None, &AppsStyle::default(), Colors::default());
        assert_eq!(rules, AppRules::default());
        assert_eq!(fill("$nope", Colors::default()), None);
        assert_eq!(
            fill(
                "$accent/zz",
                Colors {
                    accent: Some([1, 2, 3]),
                    dominant: None
                }
            ),
            None
        );
        assert_eq!(fill("12,4", Colors::default()).as_deref(), Some("12,4"));
    }
}
