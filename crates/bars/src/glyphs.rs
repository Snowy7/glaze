//! Native glyph sets, ported from the old shell's glyph-sets.ts. Packs override these.
use crate::style::Glyphs;
pub fn svg(set: Glyphs, key: &str) -> Option<&'static [u8]> {
    if set == Glyphs::Fluent {
        return None;
    }
    if set == Glyphs::Symbols
        && let Some(svg) = symbols(key)
    {
        return Some(svg);
    }
    crate::icon_pack::bundled_mono_icon(&format!("{key}.svg"))
}
fn symbols(key: &str) -> Option<&'static [u8]> {
    match key {
        "search" => Some(include_bytes!("../assets/icons/symbols/search.svg")),
        "wifi-1" => Some(include_bytes!("../assets/icons/symbols/wifi-1.svg")),
        "wifi-2" => Some(include_bytes!("../assets/icons/symbols/wifi-2.svg")),
        "wifi-3" => Some(include_bytes!("../assets/icons/symbols/wifi-3.svg")),
        "wifi-4" => Some(include_bytes!("../assets/icons/symbols/wifi-4.svg")),
        "offline" => Some(include_bytes!("../assets/icons/symbols/offline.svg")),
        "ethernet" => Some(include_bytes!("../assets/icons/symbols/ethernet.svg")),
        "volume-mute" => Some(include_bytes!("../assets/icons/symbols/volume-mute.svg")),
        "volume-low" => Some(include_bytes!("../assets/icons/symbols/volume-low.svg")),
        "volume-mid" => Some(include_bytes!("../assets/icons/symbols/volume-mid.svg")),
        "volume-high" => Some(include_bytes!("../assets/icons/symbols/volume-high.svg")),
        "bolt" => Some(include_bytes!("../assets/icons/symbols/bolt.svg")),
        "quick" => Some(include_bytes!("../assets/icons/symbols/quick.svg")),
        "power" => Some(include_bytes!("../assets/icons/symbols/power.svg")),
        "play" => Some(include_bytes!("../assets/icons/symbols/play.svg")),
        "pause" => Some(include_bytes!("../assets/icons/symbols/pause.svg")),
        "chevron" => Some(include_bytes!("../assets/icons/symbols/chevron.svg")),
        "desktop" => Some(include_bytes!("../assets/icons/symbols/desktop.svg")),
        "trash" => Some(include_bytes!("../assets/icons/symbols/trash.svg")),
        "trash-full" => Some(include_bytes!("../assets/icons/symbols/trash-full.svg")),
        "battery-0" => Some(include_bytes!("../assets/icons/symbols/battery-0.svg")),
        "battery-1" => Some(include_bytes!("../assets/icons/symbols/battery-1.svg")),
        "battery-2" => Some(include_bytes!("../assets/icons/symbols/battery-2.svg")),
        "battery-3" => Some(include_bytes!("../assets/icons/symbols/battery-3.svg")),
        "battery-4" => Some(include_bytes!("../assets/icons/symbols/battery-4.svg")),
        "battery-5" => Some(include_bytes!("../assets/icons/symbols/battery-5.svg")),
        "battery-6" => Some(include_bytes!("../assets/icons/symbols/battery-6.svg")),
        "battery-7" => Some(include_bytes!("../assets/icons/symbols/battery-7.svg")),
        "battery-8" => Some(include_bytes!("../assets/icons/symbols/battery-8.svg")),
        "battery-9" => Some(include_bytes!("../assets/icons/symbols/battery-9.svg")),
        "battery-10" => Some(include_bytes!("../assets/icons/symbols/battery-10.svg")),
        _ => None,
    }
}
