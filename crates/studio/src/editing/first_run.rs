//! Starter desktop metadata shared by the first-run editor and previews.

struct Starter {
    name: &'static str,
    line: &'static str,
    wallpaper: &'static str,
    preset: &'static str,
    widgets: &'static [(&'static str, [u32; 2])],
}

const STARTERS: [Starter; 4] = [
    Starter {
        name: "Calm",
        line: "Liquid glass, a menu bar and a dock",
        wallpaper: "rain-city",
        preset: "macos",
        widgets: &[("clock", [3, 2]), ("weather", [3, 2])],
    },
    Starter {
        name: "Familiar",
        line: "Fluent, the Windows 11 taskbar",
        wallpaper: "spring-forest",
        preset: "windows11",
        widgets: &[("clock", [3, 2])],
    },
    Starter {
        name: "Bold",
        line: "One UI cards on islands",
        wallpaper: "star-nest",
        preset: "islands",
        widgets: &[("clock", [3, 2]), ("system-monitor", [3, 2])],
    },
    Starter {
        name: "Quiet",
        line: "Mono, just a top bar",
        wallpaper: "snowfall",
        preset: "monochrome",
        widgets: &[],
    },
];

/// The names and descriptions used by the first-run chooser.
pub(crate) fn panel_starters() -> Vec<(&'static str, &'static str)> {
    STARTERS.iter().map(|s| (s.name, s.line)).collect()
}

/// Read-only starter data for the panel's software desktop specimens.
pub(crate) fn panel_starter(
    index: usize,
) -> Option<(
    &'static str,
    &'static str,
    &'static [(&'static str, [u32; 2])],
)> {
    STARTERS
        .get(index)
        .map(|s| (s.wallpaper, s.preset, s.widgets))
}
