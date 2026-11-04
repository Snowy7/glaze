//! What Windows says about color and motion: the accent palette (seven shades, from
//! `HKCU\…\Explorer\Accent\AccentPalette`, what Settings writes), whether the shell is in light
//! or dark mode (`SystemUsesLightTheme`), and whether animations are on
//! (`SPI_GETCLIENTAREAANIMATION`). Read at start and again on `WM_SETTINGCHANGE`.

use serde::Serialize;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, PCWSTR, w};

/// Windows' default blue, for a machine that has no palette yet.
const DEFAULT: [&str; 7] = [
    "#0078d4", "#0091f8", "#4cc2ff", "#99ebff", "#0067c0", "#003e92", "#001a68",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Light,
    Dark,
}

/// Sent to the page as `system` (it sets `--system-accent*` and `data-system-scheme`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct System {
    /// `[base, light-1, light-2, light-3, dark-1, dark-2, dark-3]`, `#rrggbb`.
    pub accent: [String; 7],
    pub scheme: Scheme,
    /// Windows' "Animation effects" (the page's reduce-motion default).
    #[serde(skip)]
    pub animations: bool,
}

/// Reorders the registry's `AccentPalette` (eight RGBA entries, lightest first: light 3, 2, 1,
/// base, dark 1, 2, 3, unused) into [`System::accent`]'s order.
fn shades(palette: &[u8]) -> Option<[String; 7]> {
    if palette.len() < 28 {
        return None;
    }
    let hex = |i: usize| {
        let [r, g, b] = [palette[i * 4], palette[i * 4 + 1], palette[i * 4 + 2]];
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    Some([hex(3), hex(2), hex(1), hex(0), hex(4), hex(5), hex(6)])
}

fn read_value(key: PCWSTR, name: PCWSTR, kind: u32, out: &mut [u8]) -> Option<usize> {
    let mut size = out.len() as u32;
    // SAFETY: `out` is writable for `size` bytes, which the call is told.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            windows::Win32::System::Registry::REG_ROUTINE_FLAGS(kind),
            None,
            Some(out.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(size as usize)
}

pub fn read() -> System {
    let mut palette = [0u8; 32];
    let accent = read_value(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent"),
        w!("AccentPalette"),
        RRF_RT_REG_BINARY.0,
        &mut palette,
    )
    .and_then(|len| shades(&palette[..len]))
    .unwrap_or_else(|| DEFAULT.map(str::to_owned));

    let mut light = [0u8; 4];
    let light = read_value(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
        RRF_RT_REG_DWORD.0,
        &mut light,
    )
    .is_some_and(|_| u32::from_le_bytes(light) != 0);

    let mut animations = BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut animations).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    System {
        accent,
        scheme: if light { Scheme::Light } else { Scheme::Dark },
        animations: animations.as_bool(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_is_reordered_base_first() {
        let mut palette = Vec::new();
        for shade in 0u8..8 {
            palette.extend([shade, shade, shade, 0xff]);
        }
        let shades = shades(&palette).unwrap();
        assert_eq!(shades[0], "#030303", "base is the fourth entry");
        assert_eq!(&shades[1..4], ["#020202", "#010101", "#000000"]);
        assert_eq!(&shades[4..], ["#040404", "#050505", "#060606"]);
        assert!(super::shades(&[0; 8]).is_none());
    }
}
