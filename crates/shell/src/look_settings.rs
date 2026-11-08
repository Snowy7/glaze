//! Resolve config.json's Look at runtime. Called on config notifications, never while drawing.
//! Specific bar/instance values beat this layer; wallpaper/design values sit below it.
use crate::glass::Family;
use glaze_bars::{
    accent,
    style::{Look as BarLook, NativeStyle},
};
use glaze_config::look::{Icons, Look, Scheme, Style, Tone};
use glaze_theme::{Appearance, Paint, TextTone};
use glaze_ui::{Text, icons::IconSet};

#[cfg(test)]
#[path = "look_settings_tests.rs"]
mod tests;

pub fn family(look: &Look, surface: Family) -> Option<&glaze_config::look::FamilyLook> {
    match surface {
        Family::Bars => look.bars.as_ref(),
        Family::Widgets => look.widgets.as_ref(),
        Family::Menus => look.menus.as_ref(),
    }
}

pub fn style(look: &Look, surface: Family) -> Option<NativeStyle> {
    family(look, surface)
        .and_then(|f| f.style)
        .or(look.style)
        .map(|s| match s {
            Style::Liquid => NativeStyle::Liquid,
            Style::Oneui => NativeStyle::OneUi,
            Style::Solid => NativeStyle::Solid,
            Style::Fluent => NativeStyle::Fluent,
            Style::Mono => NativeStyle::Mono,
        })
}

/// An installed family, with a deterministic system fallback for empty or missing names.
pub fn font(name: &str, installed: impl Fn(&str) -> bool) -> String {
    let name = name.trim();
    if !name.is_empty() && installed(name) {
        name.into()
    } else if installed(glaze_ui::VARIABLE) {
        glaze_ui::VARIABLE.into()
    } else {
        "Segoe UI".into()
    }
}

pub fn icons(look: &Look, text: &Text) -> Option<IconSet> {
    look.text.icons.map(|i| match i {
        Icons::Lucide => IconSet::Vector,
        Icons::Fluent => IconSet::Font(glaze_ui::icons::system_family(text)),
    })
}

/// The user's appearance over a wallpaper's. Tone/shadow affect only bare wallpaper text.
/// `auto` uses the wallpaper's resolved tone (and ultimately the OS scheme).
pub fn appearance(
    look: &Look,
    surface: Family,
    base: &Appearance,
    bare: bool,
    system: &accent::System,
) -> Appearance {
    let material = family(look, surface).map_or(look.material, |f| f.material.over(&look.material));
    let mut layer = Appearance {
        radius: look.shape.corners,
        border: look.shape.edge,
        font: look.text.font.clone(),
        scale: look.text.size,
        accent: look.accent,
        tint: match material.tint {
            Some(Paint::Color(c)) => Some(c),
            // Automatic popup/widget glass is neutral, so wallpaper colour comes through the
            // lens itself rather than being painted onto it a second time.
            Some(Paint::Auto) if surface != Family::Bars => Some(glaze_widgets::look::DEFAULT_TINT),
            Some(Paint::Auto) => glaze_tweaks::palette::read().map(|p| {
                let dark = match look.appearance {
                    Some(Scheme::Light) => false,
                    Some(Scheme::Dark) => true,
                    _ => system.scheme == accent::Scheme::Dark,
                };
                crate::glass::tone::wallpaper_tint(p.palette.accent, dark)
            }),
            None => None,
        },
        shadow: bare.then_some(look.text.shadow).flatten(),
        ..Default::default()
    }
    .over(base);
    let tone = match look.appearance {
        Some(Scheme::Light) => Some(TextTone::Dark),
        Some(Scheme::Dark) => Some(TextTone::Light),
        Some(Scheme::Auto) => Some(if system.scheme == accent::Scheme::Light {
            TextTone::Dark
        } else {
            TextTone::Light
        }),
        None => base.text,
    };
    layer.text = if bare {
        match look.text.tone {
            Some(Tone::Light) => Some(TextTone::Light),
            Some(Tone::Dark) => Some(TextTone::Dark),
            Some(Tone::Auto) => base.text.or(tone),
            None => tone,
        }
    } else {
        tone
    };
    layer
}

/// Put the user's layer between theme/design defaults and explicitly configured bar fields.
#[allow(clippy::too_many_arguments)]
pub fn bar(
    look: &Look,
    surface: Family,
    config: &glaze_bars::BarsConfig,
    key: &str,
    bar: &mut BarLook,
    base: &mut Appearance,
    system: &accent::System,
    text: &Text,
) {
    let own = |p| crate::bars::look::explicit_look(config, key, p);
    let bare = bar.transparent.unwrap_or_else(|| match bar.background {
        Some(glaze_bars::style::Fill::None) => true,
        Some(_) => false,
        None => base.surface == Some(glaze_theme::Surface::None),
    });
    *base = appearance(look, surface, base, bare, system);
    if (surface != Family::Bars || !own("/style"))
        && let Some(style) = style(look, surface)
    {
        bar.style = Some(style);
        // A glass design's translucent tint is a default, not an override of Solid's body.
        if style == NativeStyle::Solid && !own("/tint") { bar.tint = None; }
    }
    if look.shape.corners.is_some() && !own("/radius") {
        bar.radius = None;
    }
    if look.text.font.is_some() && !own("/font/family") {
        bar.font.family = None;
    }
    if let Some(name) = bar.font.family.as_mut().or(base.font.as_mut()) {
        *name = font(name, |f| text.has_family(f));
    }
    if look.appearance.is_some() && !own("/scheme") {
        bar.scheme = None;
    }
    // Bare labels use their own tone, independently of the material's appearance.
    if bare && look.text.tone.is_some() && !own("/scheme") {
        bar.scheme = None;
    }
    if look.shape.edge.is_some() && !own("/border") {
        bar.border = Default::default();
    }
    let material = family(look, surface).map_or(look.material, |f| f.material.over(&look.material));
    if !own("/transparent") {
        if own("/background") {
            // A chosen body replaces a design's transparent rail, including background: none.
            bar.transparent = None;
        } else if material.tint.is_some() {
            bar.transparent = Some(false);
        } else if matches!(
            style(look, surface),
            Some(NativeStyle::Solid | NativeStyle::Mono)
        ) {
            bar.transparent = Some(false);
            bar.background = None;
        }
    }
    if material.tint.is_some() && bar.transparent != Some(true) && !own("/background") {
        // A reserved top strip never overlaps maximized apps, even when they pause the
        // wallpaper. Use live glass for the chosen tint, as for a dock over an app; a
        // wallpaper plate would remain invisible until the desktop is uncovered.
        bar.background = Some(glaze_bars::style::Fill::Blur(
            glaze_bars::style::BlurFill {
                brightness: 1.0,
                blur: 0.0,
            },
        ));
    }
    for (path, value, target) in [
        ("/glass/clarity", material.clarity, &mut bar.glass.clarity),
        ("/glass/blur", material.blur, &mut bar.glass.blur),
        (
            "/glass/refraction",
            material.refraction,
            &mut bar.glass.refraction,
        ),
        (
            "/glass/highlight",
            material.highlight,
            &mut bar.glass.highlight,
        ),
    ] {
        if value.is_some() && !own(path) {
            *target = value;
        }
    }
    if material.clarity.is_some() && !own("/glass/tint") {
        bar.glass.tint = None;
    }
    if material.clarity.is_some() && !own("/tint") {
        bar.tint = None;
    }
}
