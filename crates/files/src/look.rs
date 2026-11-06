//! The same kit style, scheme, accent, family and type as the active shell panels.
use mypaper_config::{Config, look};
use mypaper_ui::{Rgba, Scheme, Style, Theme, theme::Knobs};

pub fn load(override_scheme: Option<Scheme>) -> Theme {
    let config = Config::load().unwrap_or_default();
    let bars = mypaper_bars::config::path()
        .and_then(|path| mypaper_bars::BarsConfig::load(&path))
        .unwrap_or_default();
    let system = mypaper_bars::accent::read();
    let wallpaper = mypaper_theme::locate(Some(&config.theme))
        .and_then(mypaper_theme::LoadedTheme::load)
        .ok();
    let appearance = wallpaper
        .as_ref()
        .and_then(|loaded| loaded.theme.bars.as_ref())
        .and_then(|skin| skin.appearance.as_ref());
    let bar = &bars.top.look;
    let family = config.look.menus.as_ref();
    let style = family
        .and_then(|f| f.style)
        .or(config.look.style)
        .map(|style| match style {
            look::Style::Liquid => Style::Liquid,
            look::Style::Oneui => Style::OneUi,
            look::Style::Solid => Style::Solid,
            look::Style::Fluent => Style::Fluent,
            look::Style::Mono => Style::Mono,
        })
        .unwrap_or_else(|| match bar.native_style() {
            mypaper_bars::style::NativeStyle::Liquid => Style::Liquid,
            mypaper_bars::style::NativeStyle::OneUi => Style::OneUi,
            mypaper_bars::style::NativeStyle::Solid => Style::Solid,
            mypaper_bars::style::NativeStyle::Fluent => Style::Fluent,
            mypaper_bars::style::NativeStyle::Mono => Style::Mono,
        });
    let scheme = override_scheme.unwrap_or_else(|| match config.look.appearance {
        Some(look::Scheme::Dark) => Scheme::Dark,
        Some(look::Scheme::Light) => Scheme::Light,
        _ => match bar.scheme {
            Some(mypaper_bars::style::Scheme::Light) => Scheme::Light,
            Some(mypaper_bars::style::Scheme::Dark) => Scheme::Dark,
            _ => {
                if system.scheme == mypaper_bars::accent::Scheme::Light {
                    Scheme::Light
                } else {
                    Scheme::Dark
                }
            }
        },
    });
    let accent = match config.look.accent {
        Some(mypaper_config::Paint::Color(mypaper_config::Color(rgb))) => Rgba::rgb8(rgb, 1.0),
        _ => parse_color(&system.accent[0]).unwrap_or(mypaper_ui::theme::SYSTEM_BLUE),
    };
    let mut theme = Theme::new(style, scheme, accent);
    let material = family.map_or(config.look.material, |f| {
        f.material.over(&config.look.material)
    });
    if let Some(glass) = theme.glass.as_mut() {
        *glass = glass.tuned(
            style,
            &Knobs {
                clarity: material.clarity,
                blur: material.blur,
                refraction: material.refraction,
                highlight: material.highlight,
            },
        );
        if let Some(mypaper_config::Paint::Color(mypaper_config::Color(rgb))) = material.tint {
            glass.tint = Rgba::rgb8(rgb, glass.tint.a);
        }
    }
    if let Some(radius) = config.look.shape.corners.or(bar.radius) {
        theme.radius.surface = radius;
    }
    if let Some(font) = config.look.text.font.as_ref().or(bar.font.family.as_ref()) {
        theme.font.family = font.as_str().into();
    }
    if let Some(size) = config.look.text.size {
        for role in &mut theme.font.roles {
            role.size *= size;
            role.line *= size;
        }
    }
    // Wallpaper tint is the inherited surface when the user has not set a material tint.
    if let Some(tint) = material.tint.or_else(|| {
        appearance
            .and_then(|a| a.tint)
            .map(mypaper_config::Paint::Color)
    }) && let mypaper_config::Paint::Color(mypaper_config::Color(rgb)) = tint
    {
        theme.color.surface = Rgba::rgb8(rgb, 1.0);
    }
    // A full app sits over other applications' text. Its default glass is calmer and more
    // frosted than a small bar; explicit material controls still win, including clear glass.
    if theme.glass.is_some() {
        let base = mypaper_ui::GlassMaterial {
            clarity: 0.14,
            blur: 28.0,
            refraction: 0.0,
            highlight: 0.0,
            tint: theme.color.surface.with_alpha(0.86),
        };
        let mut glass = base.tuned(
            style,
            &Knobs {
                clarity: material.clarity,
                blur: material.blur,
                refraction: material.refraction,
                highlight: material.highlight,
            },
        );
        if let Some(mypaper_config::Paint::Color(mypaper_config::Color(rgb))) = material.tint {
            glass.tint = Rgba::rgb8(rgb, glass.tint.a);
        }
        theme.glass = Some(glass);
    }
    theme
}

fn parse_color(value: &str) -> Option<Rgba> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(value, 16).ok()?;
    Some(Rgba::rgb8(
        [(value >> 16) as u8, (value >> 8) as u8, value as u8],
        1.0,
    ))
}
