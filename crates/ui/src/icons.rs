//! Icons: glyphs of the system icon font, drawn as text by DirectWrite (crisp at every DPI,
//! no bitmaps, no cost). Segoe Fluent Icons ships with Windows 11; Windows 10 has Segoe MDL2
//! Assets, with the same code points for the outlined glyphs and no solid media glyphs.
//! Views name an [`Icon`] and place it with [`centered`]; lists of raw code points are gone.

use std::sync::Arc;

use crate::geom::{Rect, Rgba};
use crate::scene::Op;
use crate::text::{Text, TextStyle};

pub const SYSTEM_FAMILY: &str = "Segoe Fluent Icons";
const SYSTEM_FALLBACK: &str = "Segoe MDL2 Assets";

/// Segoe Fluent Icons / MDL2 code points (kept for the menu component).
pub const CHECK: char = '\u{E73E}';
pub const CHEVRON_RIGHT: char = '\u{E76C}';

/// The system icon font that is installed here.
pub fn system_family(text: &Text) -> Arc<str> {
    if text.has_family(SYSTEM_FAMILY) {
        SYSTEM_FAMILY.into()
    } else {
        SYSTEM_FALLBACK.into()
    }
}

/// A named glyph of the system icon font. Code points were checked against a rendered sheet
/// of Segoe Fluent Icons on Windows 11 (26300).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    // Media (solid, as media controls are everywhere else).
    Play,
    Pause,
    Previous,
    Next,
    Music,
    // Navigation and commands.
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    ChevronDown,
    Add,
    Remove,
    Close,
    Check,
    More,
    Settings,
    Search,
    Edit,
    Delete,
    Copy,
    Refresh,
    Pin,
    Resize,
    // Shell.
    Apps,
    Power,
    Restart,
    SignOut,
    Lock,
    Bell,
    Calendar,
    Clock,
    Timer,
    Globe,
    Keyboard,
    Desktop,
    // Status.
    Volume(u8),
    Mute,
    Wifi(u8),
    Ethernet,
    Offline,
    Bluetooth,
    Brightness,
    Moon,
    Focus,
    Saver,
    Gauge,
    Thermometer,
    Cpu,
    Battery(u8),
    Charging,
    Notes,
    Tasks,
    Photo,
    Feed,
}

impl Icon {
    /// The glyph in Segoe Fluent Icons. Levels (volume 0..=3, Wi-Fi 0..=3, battery 0..=10)
    /// are clamped.
    pub const fn glyph(self) -> char {
        match self {
            Self::Play => '\u{F5B0}',
            Self::Pause => '\u{F8AE}',
            Self::Previous => '\u{F8AC}',
            Self::Next => '\u{F8AD}',
            Self::Music => '\u{E8D6}',
            Self::ChevronLeft => '\u{E76B}',
            Self::ChevronRight => '\u{E76C}',
            Self::ChevronUp => '\u{E70E}',
            Self::ChevronDown => '\u{E70D}',
            Self::Add => '\u{E710}',
            Self::Remove => '\u{E738}',
            Self::Close => '\u{E711}',
            Self::Check => '\u{E73E}',
            Self::More => '\u{E712}',
            Self::Settings => '\u{E713}',
            Self::Search => '\u{E721}',
            Self::Edit => '\u{E70F}',
            Self::Delete => '\u{E74D}',
            Self::Copy => '\u{E8C8}',
            Self::Refresh => '\u{E72C}',
            Self::Pin => '\u{E718}',
            Self::Resize => '\u{E740}',
            Self::Apps => '\u{ECA5}',
            Self::Power => '\u{E7E8}',
            Self::Restart => '\u{E777}',
            Self::SignOut => '\u{E748}',
            Self::Lock => '\u{E72E}',
            Self::Bell => '\u{EA8F}',
            Self::Calendar => '\u{E787}',
            Self::Clock => '\u{E823}',
            Self::Timer => '\u{E916}',
            Self::Globe => '\u{E774}',
            Self::Keyboard => '\u{E765}',
            Self::Desktop => '\u{E7F4}',
            Self::Volume(level) => match level {
                0 => '\u{E992}',
                1 => '\u{E993}',
                2 => '\u{E994}',
                _ => '\u{E995}',
            },
            Self::Mute => '\u{E74F}',
            Self::Wifi(level) => match level {
                0 => '\u{E872}',
                1 => '\u{E873}',
                2 => '\u{E874}',
                _ => '\u{E701}',
            },
            Self::Ethernet => '\u{E839}',
            Self::Offline => '\u{E871}',
            Self::Bluetooth => '\u{E702}',
            Self::Brightness => '\u{E706}',
            Self::Moon => '\u{E708}',
            Self::Focus => '\u{E916}',
            Self::Saver => '\u{E83F}',
            Self::Gauge => '\u{EC4A}',
            Self::Thermometer => '\u{E9CA}',
            Self::Cpu => '\u{E950}',
            Self::Battery(level) => {
                // E850 (empty) ..= E85A (full).
                let level = if level > 10 { 10 } else { level };
                match char::from_u32(0xE850 + level as u32) {
                    Some(c) => c,
                    None => '\u{E85A}',
                }
            }
            Self::Charging => '\u{E945}',
            Self::Notes => '\u{E70B}',
            Self::Tasks => '\u{E762}',
            Self::Photo => '\u{E91B}',
            Self::Feed => '\u{E8A1}',
        }
    }

    /// The glyph in `family`: Segoe MDL2 Assets lacks the solid media glyphs and the apps
    /// grid, so it gets their outlined counterparts.
    pub fn glyph_in(self, family: &str) -> char {
        if family == SYSTEM_FAMILY {
            return self.glyph();
        }
        match self {
            Self::Play => '\u{E768}',
            Self::Pause => '\u{E769}',
            Self::Previous => '\u{E892}',
            Self::Next => '\u{E893}',
            Self::Apps => '\u{E71D}',
            other => other.glyph(),
        }
    }
}

/// `icon` at `size` device pixels, centred in `rect` by its measured box.
pub fn centered(
    text: &mut Text,
    family: &str,
    icon: Icon,
    size: f32,
    rect: Rect,
    color: Rgba,
) -> Op {
    let style = TextStyle::new(family, size);
    let glyph: Arc<str> = icon.glyph_in(family).to_string().into();
    let (w, h) = text.measure(&glyph, &style);
    Op::Text {
        rect: Rect::new(
            (rect.left + (rect.width() - w) / 2.0).round(),
            (rect.top + (rect.height() - h) / 2.0).round(),
            w,
            h,
        ),
        text: glyph,
        style,
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_clamp_and_mdl2_gets_outlined_media() {
        assert_eq!(Icon::Battery(10).glyph(), '\u{E85A}');
        assert_eq!(Icon::Battery(250).glyph(), '\u{E85A}');
        assert_eq!(Icon::Volume(9).glyph(), '\u{E995}');
        assert_eq!(Icon::Play.glyph_in(SYSTEM_FALLBACK), '\u{E768}');
        assert_eq!(Icon::Play.glyph_in(SYSTEM_FAMILY), '\u{F5B0}');
        assert_eq!(Icon::Close.glyph_in(SYSTEM_FALLBACK), Icon::Close.glyph());
    }
}
