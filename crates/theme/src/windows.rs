//! One native style's Windows defaults and app-styler colour tokens. This is pure: callers
//! supply the active wallpaper palette and Windows' unmanaged light/dark preference, then
//! apply the result through the normal DWM/registry undo machinery when an input changes.

use mypaper_config::look::{Apps, Scheme, Style};
use mypaper_config::{Backdrop, Color, Corners, Look, Paint, WindowsStyle};

use crate::{TextTone, Theme};

/// The active wallpaper's two colours. Callers must reject palettes from another theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dominant: Color,
    pub accent: Color,
}

/// Windows styling plus the same `$accent` / `$dominant` tokens used inside native apps.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub windows: Option<WindowsStyle>,
    pub accent: Option<Color>,
    pub dominant: Option<Color>,
}

/// Layers style defaults < wallpaper < shared look < explicit `look.windows` fields.
/// Selecting a style opts into synchronization; an absent style preserves legacy unmanaged
/// Windows behavior. `appearance: auto` / `windows.apps: as-is` never force Windows' mode.
pub fn resolve(
    look: &Look,
    theme: Option<&Theme>,
    palette: Option<Palette>,
    system_dark: bool,
) -> Resolved {
    let theme_windows = theme.and_then(|t| t.windows.as_ref());
    let windows = if let Some(style) = look.style {
        let mode = match look.windows.apps {
            Some(Apps::Dark) => Some(true),
            Some(Apps::Light) => Some(false),
            Some(Apps::AsIs) => None,
            Some(Apps::Match) | None => match look.appearance {
                Some(Scheme::Dark) => Some(true),
                Some(Scheme::Light) => Some(false),
                Some(Scheme::Auto) => None,
                None => theme_windows.and_then(|w| w.dark_mode).or_else(|| {
                    let text = theme
                        .and_then(|t| t.bars.as_ref())
                        .and_then(|b| b.appearance.as_ref())
                        .and_then(|a| a.text);
                    Some(text != Some(TextTone::Dark))
                }),
            },
        };
        let dark = mode.unwrap_or(system_dark);
        let mut base = defaults(style, dark);
        if let Some(wallpaper) = theme_windows {
            base = wallpaper.over(&base);
        }
        // A shared accent also supplies the default border. An explicit Windows border
        // remains an independent field, including null (leave Windows alone).
        if let Some(accent) = look.accent {
            base.accent = Some(accent);
        }
        if style == Style::Mono && base.accent == Some(Paint::Auto) {
            base.accent = defaults(style, dark).accent;
        }
        base.border = theme_windows.and_then(|w| w.border).or(base.accent);
        let mut resolved = look.windows_style(Some(&base)).unwrap_or_default();
        resolved.dark_mode = mode;
        (resolved != WindowsStyle::default()).then_some(resolved)
    } else {
        look.windows_style(theme_windows)
    };
    let accent = windows
        .as_ref()
        .and_then(|w| w.accent)
        .or(look.accent)
        .or_else(|| theme_windows.and_then(|w| w.accent))
        .or(Some(Paint::Auto));
    Resolved {
        windows,
        accent: paint(accent, palette),
        dominant: palette.map(|p| p.dominant),
    }
}

fn paint(paint: Option<Paint>, palette: Option<Palette>) -> Option<Color> {
    match paint? {
        Paint::Color(color) => Some(color),
        Paint::Auto => palette.map(|p| p.accent),
    }
}

/// Surface colours match the native kit's palette; DWM offers round/small/square rather
/// than an arbitrary radius. Flat styles request no backdrop, so Windows draws no blur.
fn defaults(style: Style, dark: bool) -> WindowsStyle {
    let (surface, corners, backdrop) = match style {
        Style::Liquid => (
            if dark { [28, 28, 30] } else { [245, 245, 247] },
            Corners::Round,
            Backdrop::Acrylic,
        ),
        Style::Oneui => (
            if dark { [23, 23, 26] } else { [244, 244, 246] },
            Corners::Round,
            Backdrop::Acrylic,
        ),
        Style::Fluent => (
            if dark { [32, 32, 32] } else { [243, 243, 243] },
            Corners::Small,
            Backdrop::Mica,
        ),
        Style::Solid => (
            if dark { [28, 28, 30] } else { [242, 242, 247] },
            Corners::Square,
            Backdrop::None,
        ),
        Style::Mono => (
            if dark { [20, 20, 22] } else { [247, 247, 248] },
            Corners::Square,
            Backdrop::None,
        ),
    };
    let mono = style == Style::Mono;
    let accent = if mono {
        Paint::Color(Color(if dark { [242; 3] } else { [28, 28, 30] }))
    } else {
        Paint::Auto
    };
    WindowsStyle {
        accent: Some(accent),
        border: Some(accent),
        caption: Some(Color(surface)),
        caption_text: Some(Color(if dark { [255; 3] } else { [28, 28, 30] })),
        corners: Some(corners),
        backdrop: Some(backdrop),
        dark_mode: Some(dark),
        // Windows only supports a coloured Start/taskbar in dark system mode.
        accent_on_taskbar: Some(!mono && dark),
        accent_on_title_bars: Some(!mono),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette {
            dominant: Color([12, 34, 56]),
            accent: Color([56, 123, 210]),
        }
    }

    fn look(json: &str) -> Look {
        serde_json::from_str(json).unwrap()
    }

    fn theme(windows: &str) -> Theme {
        serde_json::from_str(&format!(
            r#"{{"name":"test","wallpaper":{{"kind":"image","source":"x.png"}},"windows":{windows}}}"#,
        ))
        .unwrap()
    }

    #[test]
    fn five_styles_set_frames_mode_and_independent_accent_visibility() {
        for style in Style::ALL {
            for (appearance, dark) in [(Scheme::Dark, true), (Scheme::Light, false)] {
                let look = Look {
                    style: Some(style),
                    appearance: Some(appearance),
                    ..Default::default()
                };
                let result = resolve(&look, None, Some(palette()), !dark);
                let windows = result.windows.unwrap();
                assert_eq!(windows.dark_mode, Some(dark));
                assert_eq!(
                    windows.accent_on_taskbar,
                    Some(style != Style::Mono && dark)
                );
                assert_eq!(windows.accent_on_title_bars, Some(style != Style::Mono));
                assert_eq!(result.dominant, Some(palette().dominant));
                let expected = if style == Style::Mono {
                    Color(if dark { [242; 3] } else { [28, 28, 30] })
                } else {
                    palette().accent
                };
                assert_eq!(result.accent, Some(expected));
                assert!(windows.caption.is_some() && windows.caption_text.is_some());
                assert_eq!(
                    windows.backdrop,
                    Some(match style {
                        Style::Liquid | Style::Oneui => Backdrop::Acrylic,
                        Style::Fluent => Backdrop::Mica,
                        Style::Solid | Style::Mono => Backdrop::None,
                    })
                );
            }
        }
    }

    #[test]
    fn explicit_windows_fields_win_over_shared_look_theme_and_style() {
        let theme = theme(r##"{"accent":"#112233","corners":"small","dark_mode":true}"##);
        let look = look(
            r##"{"style":"solid","accent":"#334455","appearance":"light","windows":{
            "accent":"#aa5500","border":null,"caption":"#eeeeee","caption_text":"#123456",
            "corners":"round","backdrop":null,"apps":"dark","accent_on_taskbar":null,
            "accent_on_title_bars":false,"cursors":null}}"##,
        );
        let result = resolve(&look, Some(&theme), Some(palette()), false);
        let w = result.windows.unwrap();
        assert_eq!(result.accent, Some(Color([170, 85, 0])));
        assert_eq!(w.accent, Some(Paint::Color(Color([170, 85, 0]))));
        assert_eq!(w.border, None);
        assert_eq!(w.caption, Some(Color([238; 3])));
        assert_eq!(w.caption_text, Some(Color([18, 52, 86])));
        assert_eq!(w.corners, Some(Corners::Round));
        assert_eq!(w.backdrop, None);
        assert_eq!(w.dark_mode, Some(true));
        assert_eq!(w.accent_on_taskbar, None);
        assert_eq!(w.accent_on_title_bars, Some(false));
        assert_eq!(w.cursors, None);
    }

    #[test]
    fn wallpaper_overrides_defaults_and_shared_look_overrides_wallpaper() {
        let theme = theme(
            r##"{"accent":"#112233","caption":"#101112","corners":"small","dark_mode":true}"##,
        );
        let result = resolve(
            &look(r#"{"style":"solid"}"#),
            Some(&theme),
            Some(palette()),
            false,
        );
        let w = result.windows.unwrap();
        assert_eq!(w.caption, Some(Color([16, 17, 18])));
        assert_eq!(w.corners, Some(Corners::Small));
        assert_eq!(result.accent, Some(Color([17, 34, 51])));
        assert_eq!(w.dark_mode, Some(true));
        let result = resolve(
            &look(r##"{"style":"solid","accent":"#bbccdd","appearance":"light"}"##),
            Some(&theme),
            None,
            true,
        );
        assert_eq!(result.accent, Some(Color([187, 204, 221])));
        assert_eq!(result.windows.unwrap().dark_mode, Some(false));
    }

    #[test]
    fn auto_and_as_is_release_mode_and_use_the_unmanaged_scheme_for_frames() {
        for json in [
            r#"{"style":"fluent","appearance":"auto"}"#,
            r#"{"style":"fluent","appearance":"dark","windows":{"apps":"as-is"}}"#,
        ] {
            for dark in [false, true] {
                let result = resolve(&look(json), None, None, dark);
                let w = result.windows.unwrap();
                assert_eq!(w.dark_mode, None);
                assert_eq!(
                    w.caption,
                    Some(Color(if dark { [32; 3] } else { [243; 3] }))
                );
                assert_eq!(w.accent_on_taskbar, Some(dark));
            }
        }
    }

    #[test]
    fn absent_style_preserves_unmanaged_windows_and_pending_palette_stays_pending() {
        assert_eq!(resolve(&Look::default(), None, None, true).windows, None);
        let theme = theme(r#"{"corners":"small"}"#);
        assert_eq!(
            resolve(&Look::default(), Some(&theme), None, true).windows,
            theme.windows
        );
        let result = resolve(&look(r#"{"style":"liquid"}"#), None, None, true);
        assert_eq!(result.accent, None);
        assert_eq!(result.dominant, None);
        assert_eq!(result.windows.unwrap().accent, Some(Paint::Auto));
        let result = resolve(
            &look(r##"{"style":"liquid","accent":"#123456"}"##),
            None,
            None,
            true,
        );
        assert_eq!(result.accent, Some(Color([18, 52, 86])));
    }

    #[test]
    fn mono_keeps_automatic_accents_neutral_but_explicit_windows_paint_wins() {
        let look = look(r#"{"style":"mono","accent":"auto","appearance":"dark"}"#);
        let result = resolve(&look, None, Some(palette()), false);
        assert_eq!(result.accent, Some(Color([242; 3])));
        let mut look = look;
        look.windows.accent = Some(Some(Paint::Auto));
        assert_eq!(
            resolve(&look, None, Some(palette()), false).accent,
            Some(palette().accent)
        );
    }
}
