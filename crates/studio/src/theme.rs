//! Studio's own look: the monochrome grays in light or dark (following Windows unless the user
//! chose), a Windows contrast theme mapped onto system colours, Windows' text size, and the icon
//! glyphs (Segoe Fluent Icons).

use mypaper_ui::controls::{Palette, Type};
use mypaper_ui::{Rgba, Text};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::w;

/// Studio's appearance setting (Settings › General).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    MatchWindows,
    Light,
    Dark,
}

impl Appearance {
    pub fn parse(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::MatchWindows,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub ty: Type,
    /// A Windows contrast theme is on: previews keep their colours inside a 1 px outline.
    pub contrast: bool,
}

impl Theme {
    /// The look for `appearance`, reading Windows' mode, contrast theme and text size.
    pub fn resolve(appearance: Appearance, text: &Text) -> Self {
        let mut ty = Type::new(text);
        ty.text_scale = text_scale();
        if let Some(palette) = contrast() {
            return Self {
                palette,
                ty,
                contrast: true,
            };
        }
        let dark = match appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::MatchWindows => !windows_light(),
        };
        Self {
            palette: if dark {
                Palette::dark()
            } else {
                Palette::light()
            },
            ty,
            contrast: false,
        }
    }

    /// [`resolve`](Self::resolve) with the families already known.
    pub fn resolve_with(appearance: Appearance, mut ty: Type) -> Self {
        ty.text_scale = text_scale();
        if let Some(palette) = contrast() {
            return Self { palette, ty, contrast: true };
        }
        let dark = match appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::MatchWindows => !windows_light(),
        };
        Self {
            palette: if dark { Palette::dark() } else { Palette::light() },
            ty,
            contrast: false,
        }
    }

    /// A fixed look for golden images: no machine settings.
    #[cfg(test)]
    pub fn fixed(dark: bool, text: &Text) -> Self {
        Self {
            palette: if dark {
                Palette::dark()
            } else {
                Palette::light()
            },
            ty: Type::new(text),
            contrast: false,
        }
    }
}

fn dword(key: windows::core::PCWSTR, name: windows::core::PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: a DWORD read into a local of the stated size.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(value)
}

/// Apps use the light theme (Settings › Personalization › Colors).
pub fn windows_light() -> bool {
    dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("AppsUseLightTheme"),
    )
    .is_none_or(|v| v != 0)
}

/// Windows' text size (Accessibility › Text size), 1..=2.25.
fn text_scale() -> f32 {
    dword(w!("Software\\Microsoft\\Accessibility"), w!("TextScaleFactor"))
        .map_or(1.0, |v| (v as f32 / 100.0).clamp(1.0, 2.25))
}

/// The tokens of an active contrast theme, from its system colours.
fn contrast() -> Option<Palette> {
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: a documented query into a sized local struct.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&raw mut hc).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() || !hc.dwFlags.contains(HCF_HIGHCONTRASTON) {
        return None;
    }
    let sys = |index| {
        // SAFETY: a system colour query.
        let c = unsafe { GetSysColor(index) };
        Rgba::rgb8([c as u8, (c >> 8) as u8, (c >> 16) as u8], 1.0)
    };
    Some(Palette::contrast(
        sys(COLOR_WINDOW),
        sys(COLOR_WINDOWTEXT),
        sys(COLOR_HIGHLIGHT),
        sys(COLOR_GRAYTEXT),
        sys(COLOR_BTNFACE),
    ))
}

/// Segoe Fluent Icons code points used across Studio.
pub mod glyph {
    pub const HOME: char = '\u{E80F}';
    pub const WALLPAPERS: char = '\u{EB9F}';
    pub const LOOK: char = '\u{E790}';
    pub const WIDGETS: char = '\u{F0E2}';
    pub const BARS: char = '\u{E737}';
    pub const INTEGRATIONS: char = '\u{E71B}';
    pub const SETTINGS: char = '\u{E713}';
    pub const SEARCH: char = '\u{E721}';
    pub const MINIMIZE: char = '\u{E921}';
    pub const MAXIMIZE: char = '\u{E922}';
    pub const RESTORE: char = '\u{E923}';
    pub const CLOSE: char = '\u{E8BB}';
    pub const UNDO: char = '\u{E7A7}';
    pub const REDO: char = '\u{E7A6}';
    pub const EYE: char = '\u{E890}';
    pub const PAUSE: char = '\u{E769}';
    pub const PLAY: char = '\u{E768}';
    pub const RESET: char = '\u{E72C}';
    pub const ADD: char = '\u{E710}';
    pub const CHECK: char = '\u{E73E}';
    pub const CHEVRON_RIGHT: char = '\u{E76C}';
    pub const CHEVRON_LEFT: char = '\u{E76B}';
    pub const CHEVRON_DOWN: char = '\u{E70D}';
    pub const ALERT: char = '\u{E7BA}';
    pub const INFO: char = '\u{E946}';
    pub const COPY: char = '\u{E8C8}';
    pub const DELETE: char = '\u{E74D}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const FILE: char = '\u{E8A5}';
    pub const GLOBE: char = '\u{E774}';
    pub const APP: char = '\u{E7C4}';
    pub const MONITOR: char = '\u{E7F4}';
    pub const POINTER: char = '\u{E7C9}';
    pub const MOVE: char = '\u{E7C2}';
    pub const GRIP: char = '\u{E76F}';
    pub const CLOCK: char = '\u{E823}';
    pub const CALENDAR: char = '\u{E787}';
    pub const MUSIC: char = '\u{E8D6}';
    pub const BELL: char = '\u{EA8F}';
    pub const CLOUD: char = '\u{E753}';
    pub const KEYBOARD: char = '\u{E765}';
    pub const GAUGE: char = '\u{EC4A}';
    pub const WRENCH: char = '\u{E90F}';
    pub const DESKTOP: char = '\u{E7F8}';
    pub const SLIDERS: char = '\u{E9E9}';
    pub const PLACE: char = '\u{E707}';
    pub const IMAGE: char = '\u{EB9F}';
    pub const VIDEO: char = '\u{E714}';
    pub const SHADER: char = '\u{E945}';
    pub const SCENE: char = '\u{F158}';
    pub const SLIDESHOW: char = '\u{E8B9}';
    pub const TERMINAL: char = '\u{E756}';
    pub const PULSE: char = '\u{E9D9}';
    pub const LAYERS: char = '\u{E81E}';
    pub const SOUND: char = '\u{E767}';
    pub const START: char = '\u{E71D}';
    pub const POWER: char = '\u{E7E8}';
    pub const WIFI: char = '\u{E701}';
    pub const BATTERY: char = '\u{E850}';
    pub const NOTES: char = '\u{E70B}';
    pub const TODO: char = '\u{E73A}';
    pub const WEATHER: char = '\u{E9CA}';
    pub const CHIP: char = '\u{E950}';
    pub const TIMER: char = '\u{E916}';
    pub const QUOTE: char = '\u{E8BD}';
    pub const RSS: char = '\u{E701}';
    pub const COIN: char = '\u{E8C7}';
    pub const HEATMAP: char = '\u{E80A}';
    pub const PHOTO: char = '\u{E91B}';
    pub const TOGGLES: char = '\u{E9F5}';
    pub const LAUNCH: char = '\u{E8A7}';
    pub const PALETTE: char = '\u{E790}';
    pub const STAR: char = '\u{E734}';
}
