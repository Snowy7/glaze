//! The flyouts' look: the top bar's native style from `bars.json` over the theme's bars
//! appearance (the same layers the bars use), and the glass every popup (flyouts and menus)
//! puts under its body. Blurred styles get engine glass; solid and mono draw their own fill.

use std::sync::Arc;

use mypaper_bars::accent::{self, System};
use mypaper_bars::config::{self, BarsConfig};
use mypaper_bars::style::{Family, Look as BarLook, NativeStyle, Scheme};
use mypaper_config::Config;
use mypaper_theme::{Appearance, LoadedTheme, Paint, TextTone};
use mypaper_ui::interact::State;
use mypaper_ui::{GlassMaterial, Ink, Rgba, Text, icons};

#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub ink: Rgba,
    pub accent: Rgba,
    /// Text and glyphs on an accent fill.
    pub on_accent: Rgba,
    /// The body when there is no glass (solid, mono, or no engine).
    pub fill: Rgba,
    /// The glass under the body, for the blurred styles.
    pub glass: Option<GlassMaterial>,
    /// Corner radius in design pixels.
    pub radius: f32,
    pub font: Arc<str>,
    pub icons: Arc<str>,
    /// How quick-settings modules group (Fluent folds network and volume into it).
    pub family: Family,
    /// `launcher.winKey`: the Windows key opens the launcher (and Win+N, Win+A ours).
    pub win_key: bool,
    /// `calendar.firstDay`, Monday = 1, Sunday = 0; `None` follows the locale.
    pub first_day: Option<u32>,
    /// `audio.switchOutput`: the mixer may change the default output.
    pub switch_output: bool,
}

impl Look {
    /// Reads `bars.json` and the theme now (the shell calls this when either changes).
    pub fn load(text: &Text) -> Self {
        let bars = config::path()
            .and_then(|p| BarsConfig::load(&p))
            .map(|mut c| {
                c.resolve(Vec::new);
                c
            })
            .inspect_err(|e| log::debug!("flyouts: bars.json: {e:#}"))
            .unwrap_or_default();
        let appearance = theme_appearance();
        let mut look = Self::resolve(
            &bars.top.look,
            &bars.appearance.over(&appearance),
            &accent::read(),
        );
        look.icons = icons::system_family(text);
        look.win_key = bars.launcher.win_key;
        look.first_day = bars.calendar.first_day.map(|d| match d {
            config::Weekday::Sunday => 0,
            config::Weekday::Monday => 1,
            config::Weekday::Saturday => 6,
        });
        look.switch_output = bars.audio.switch_output;
        look
    }

    /// The look of `bar` over `appearance`, as the bars resolve theirs (bars/look.rs).
    pub fn resolve(bar: &BarLook, appearance: &Appearance, system: &System) -> Self {
        let style = bar.style.unwrap_or_default();
        let dark = match bar.scheme {
            Some(Scheme::Dark) => true,
            Some(Scheme::Light) => false,
            Some(Scheme::Auto) => system.scheme == accent::Scheme::Dark,
            None => appearance.text.is_none_or(|t| t == TextTone::Light),
        };
        let ink = Rgba::rgb8(if dark { [245, 245, 247] } else { [25, 25, 28] }, 1.0);
        let tint = appearance.tint.map_or(
            Rgba::rgb8(if dark { [28, 29, 34] } else { [246, 246, 249] }, 1.0),
            |c| Rgba::rgb8(c.0, 1.0),
        );
        let accent = match appearance.accent {
            Some(Paint::Color(c)) => Rgba::rgb8(c.0, 1.0),
            _ => hex(&system.accent[0]).unwrap_or(Rgba::rgb8([0x3b, 0x82, 0xf6], 1.0)),
        };
        let luminance = 0.2126 * accent.r + 0.7152 * accent.g + 0.0722 * accent.b;
        let on_accent = if luminance > 0.6 {
            Rgba::rgb8([20, 20, 22], 1.0)
        } else {
            Rgba::rgb8([255, 255, 255], 1.0)
        };
        let flyout_tint = bar.flyouts.tint.unwrap_or(0.62).clamp(0.0, 1.0);
        let fill = match style {
            NativeStyle::Solid => tint,
            NativeStyle::Mono | NativeStyle::Liquid => {
                tint.with_alpha(bar.tint.or(appearance.opacity).unwrap_or(0.92).max(0.85))
            }
        };
        // Flyouts sit over app windows, so their glass is more tinted than the bars' clear
        // glass: text stays readable over anything.
        let glass = (style == NativeStyle::Liquid).then(|| GlassMaterial {
            clarity: 1.0 - flyout_tint,
            // Heavier than the bars' blur: whatever is behind a flyout (an app, a widget)
            // must not read through its text.
            blur: if bar.flyouts.blur == Some(false) {
                0.0
            } else {
                bar.glass.blur.unwrap_or(12.0).max(28.0)
            },
            refraction: bar.glass.refraction.unwrap_or(0.6),
            highlight: bar.glass.highlight.unwrap_or(0.75),
            tint,
        });
        Self {
            ink,
            accent,
            on_accent,
            fill,
            glass,
            radius: bar
                .radius
                .or(appearance.radius)
                .unwrap_or(match bar.family.unwrap_or_default() {
                    Family::Fluent => 8.0,
                    Family::Macos => 18.0,
                    Family::Material => 24.0,
                    Family::Mono => 16.0,
                })
                .max(4.0),
            font: font_family(
                bar.font
                    .family
                    .as_deref()
                    .or(appearance.font.as_deref())
                    .unwrap_or("Segoe UI Variable Text"),
            )
            .into(),
            icons: icons::SYSTEM_FAMILY.into(),
            family: bar.family.unwrap_or_default(),
            win_key: true,
            first_day: None,
            switch_output: false,
        }
    }

    /// Menus share the active family, font, colors and material with the panels.
    pub fn menu_tokens(&self) -> mypaper_ui::Tokens {
        let base = mypaper_ui::Tokens::default();
        mypaper_ui::Tokens {
            radius: self.radius,
            font: mypaper_ui::TextStyle::new(&self.font, 13.0),
            icons: self.icons.clone(),
            material: mypaper_ui::Material { tint: self.fill },
            colors: mypaper_ui::Colors {
                text: self.ink,
                text_dim: self.faint(),
                hover: self.ink.with_alpha(0.12),
                line: self.line(),
                ..base.colors
            },
            glass: self.glass,
            ..base
        }
    }

    /// Secondary text (the shared contrast steps, `mypaper_ui::Ink`).
    pub fn dim(&self) -> Rgba {
        Ink::of(self.ink).secondary
    }

    /// Hints and disabled text, hairlines' stronger cousin.
    pub fn faint(&self) -> Rgba {
        Ink::of(self.ink).disabled
    }

    /// The highlight under a hovered row or button (the shared hover wash).
    pub fn hover(&self) -> Rgba {
        State::Hover.wash(self.ink)
    }

    /// The wash under a pressed row or button.
    pub fn pressed(&self) -> Rgba {
        State::Pressed.wash(self.ink)
    }

    /// A control's resting fill (an off tile, a slider's track, a button).
    pub fn control(&self) -> Rgba {
        self.ink.with_alpha(0.10)
    }

    pub fn line(&self) -> Rgba {
        self.ink.with_alpha(0.12)
    }
}

impl Default for Look {
    /// The built-in look on a fixed dark system (for tests and golden images).
    fn default() -> Self {
        Self::resolve(&BarLook::default(), &Appearance::default(), &fixed_system())
    }
}

/// A dark Windows with a blue accent, whatever this machine says.
pub fn fixed_system() -> System {
    System {
        accent: std::array::from_fn(|_| "#3b82f6".to_owned()),
        scheme: accent::Scheme::Dark,
        animations: true,
    }
}

/// The theme's bars appearance, with `accent: auto` taken from the wallpaper's palette.
fn theme_appearance() -> Appearance {
    let loaded = Config::load()
        .and_then(|c| mypaper_theme::locate(Some(&c.theme)))
        .and_then(LoadedTheme::load)
        .ok();
    let mut appearance = loaded
        .as_ref()
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|s| s.appearance.clone())
        .unwrap_or_default();
    if let Some(t) = &loaded
        && matches!(appearance.accent, Some(Paint::Auto))
        && let Some(snapshot) = mypaper_tweaks::palette::read().filter(|p| p.is_for(&t.dir))
    {
        appearance.accent = Some(Paint::Color(snapshot.palette.accent));
    }
    appearance
}

fn hex(value: &str) -> Option<Rgba> {
    let s = value.strip_prefix('#')?;
    let v = u32::from_str_radix(s.get(..6)?, 16).ok()?;
    Some(Rgba::rgb8([(v >> 16) as u8, (v >> 8) as u8, v as u8], 1.0))
}

/// The first concrete family of a CSS-like list.
fn font_family(list: &str) -> String {
    list.split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).trim())
        .find(|f| {
            !f.is_empty()
                && ![
                    "system-ui",
                    "sans-serif",
                    "serif",
                    "monospace",
                    "ui-sans-serif",
                ]
                .iter()
                .any(|g| g.eq_ignore_ascii_case(f))
        })
        .unwrap_or("Segoe UI")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_bars::style::NativeStyle;

    #[test]
    fn only_the_blurred_style_has_glass() {
        let system = fixed_system();
        let liquid = Look::resolve(&BarLook::default(), &Appearance::default(), &system);
        let glass = liquid.glass.expect("liquid is the default and has glass");
        assert!((0.0..1.0).contains(&glass.clarity), "tinted enough to read");
        for style in [NativeStyle::Solid, NativeStyle::Mono] {
            let bar = BarLook {
                style: Some(style),
                ..Default::default()
            };
            let look = Look::resolve(&bar, &Appearance::default(), &system);
            assert_eq!(look.glass, None, "{style:?} draws its own fill");
            assert!(look.fill.a >= 0.85, "{style:?}: a readable fill");
        }
        let solid = BarLook {
            style: Some(NativeStyle::Solid),
            ..Default::default()
        };
        assert_eq!(
            Look::resolve(&solid, &Appearance::default(), &system)
                .fill
                .a,
            1.0
        );
    }
}
