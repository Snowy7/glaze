//! Studio's own look: the monochrome grays in light or dark (following Windows unless the user
//! chose), a Windows contrast theme mapped onto system colours, Windows' text size, and the icon
//! glyphs (Segoe Fluent Icons).

use glaze_ui::controls::{Palette, Type};
use glaze_ui::{Rgba, Text};
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

/// The icons used across Studio: the kit's vectors, and Segoe Fluent Icons for the caption
/// buttons (Windows' own).
pub mod glyph {
    use glaze_ui::controls::Glyph;
    use glaze_ui::icons::Icon;

    const fn i(icon: Icon) -> Glyph {
        Glyph::Icon(icon)
    }

    pub const HOME: Glyph = i(Icon::Home);
    pub const WALLPAPERS: Glyph = i(Icon::Photo);
    pub const LOOK: Glyph = i(Icon::Palette);
    pub const WIDGETS: Glyph = i(Icon::Apps);
    pub const BARS: Glyph = i(Icon::PanelTop);
    pub const INTEGRATIONS: Glyph = i(Icon::Plug);
    pub const SETTINGS: Glyph = i(Icon::Settings);
    pub const SEARCH: Glyph = i(Icon::Search);
    pub const MINIMIZE: Glyph = Glyph::Font('\u{E921}');
    pub const MAXIMIZE: Glyph = Glyph::Font('\u{E922}');
    pub const RESTORE: Glyph = Glyph::Font('\u{E923}');
    pub const CLOSE: Glyph = i(Icon::Close);
    pub const CAPTION_CLOSE: Glyph = Glyph::Font('\u{E8BB}');
    pub const UNDO: Glyph = i(Icon::Undo);
    pub const REDO: Glyph = i(Icon::Redo);
    pub const EYE: Glyph = i(Icon::Eye);
    pub const PAUSE: Glyph = i(Icon::Pause);
    pub const PLAY: Glyph = i(Icon::Play);
    pub const RESET: Glyph = i(Icon::Refresh);
    pub const ADD: Glyph = i(Icon::Add);
    pub const CHECK: Glyph = i(Icon::Check);
    pub const CHEVRON_RIGHT: Glyph = i(Icon::ChevronRight);
    pub const CHEVRON_LEFT: Glyph = i(Icon::ChevronLeft);
    pub const CHEVRON_DOWN: Glyph = i(Icon::ChevronDown);
    pub const ALERT: Glyph = i(Icon::Warning);
    pub const INFO: Glyph = i(Icon::Info);
    pub const COPY: Glyph = i(Icon::Copy);
    pub const DELETE: Glyph = i(Icon::Delete);
    pub const FOLDER: Glyph = i(Icon::Folder);
    pub const FILE: Glyph = i(Icon::File);
    pub const GLOBE: Glyph = i(Icon::Globe);
    pub const APP: Glyph = i(Icon::Window);
    pub const MONITOR: Glyph = i(Icon::Desktop);
    pub const POINTER: Glyph = i(Icon::Pointer);
    pub const MOVE: Glyph = i(Icon::Move);
    pub const GRIP: Glyph = i(Icon::Grip);
    pub const CLOCK: Glyph = i(Icon::Clock);
    pub const CALENDAR: Glyph = i(Icon::Calendar);
    pub const MUSIC: Glyph = i(Icon::Music);
    pub const BELL: Glyph = i(Icon::Bell);
    pub const CLOUD: Glyph = i(Icon::Cloud);
    pub const KEYBOARD: Glyph = i(Icon::Keyboard);
    pub const GAUGE: Glyph = i(Icon::Gauge);
    pub const WRENCH: Glyph = i(Icon::Wrench);
    pub const DESKTOP: Glyph = i(Icon::Desktop);
    pub const SLIDERS: Glyph = i(Icon::Controls);
    pub const PLACE: Glyph = i(Icon::Location);
    pub const IMAGE: Glyph = i(Icon::Photo);
    pub const VIDEO: Glyph = i(Icon::Video);
    pub const SHADER: Glyph = i(Icon::Sparkles);
    pub const SCENE: Glyph = i(Icon::Package);
    pub const SLIDESHOW: Glyph = i(Icon::Images);
    pub const TERMINAL: Glyph = i(Icon::Terminal);
    pub const PULSE: Glyph = i(Icon::Activity);
    pub const LAYERS: Glyph = i(Icon::Layers);
    pub const SOUND: Glyph = i(Icon::Volume(3));
    pub const START: Glyph = i(Icon::Apps);
    pub const POWER: Glyph = i(Icon::Power);
    pub const WIFI: Glyph = i(Icon::Wifi(3));
    pub const BATTERY: Glyph = i(Icon::Battery(7));
    pub const NOTES: Glyph = i(Icon::Notes);
    pub const TODO: Glyph = i(Icon::Tasks);
    pub const WEATHER: Glyph = i(Icon::CloudSun);
    pub const CHIP: Glyph = i(Icon::Cpu);
    pub const TIMER: Glyph = i(Icon::Timer);
    pub const QUOTE: Glyph = i(Icon::Quote);
    pub const RSS: Glyph = i(Icon::Feed);
    pub const COIN: Glyph = i(Icon::Activity);
    pub const HEATMAP: Glyph = i(Icon::Grid);
    pub const PHOTO: Glyph = i(Icon::Photo);
    pub const TOGGLES: Glyph = i(Icon::Toggle);
    pub const LAUNCH: Glyph = i(Icon::Rocket);
    pub const PALETTE: Glyph = i(Icon::Palette);
    pub const STAR: Glyph = i(Icon::Star);
}
