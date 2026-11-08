//! Starter looks shared by onboarding and its previews: a style, bar preset and widgets.

struct Starter {
    name: &'static str,
    line: &'static str,
    preset: &'static str,
    widgets: &'static [(&'static str, [u32; 2])],
}

const STARTERS: [Starter; 4] = [
    Starter {
        name: "Calm",
        line: "Liquid glass, a menu bar and a dock",
        preset: "macos",
        widgets: &[("clock", [3, 2]), ("weather", [3, 2])],
    },
    Starter {
        name: "Familiar",
        line: "Fluent, the Windows 11 taskbar",
        preset: "windows11",
        widgets: &[("clock", [3, 2])],
    },
    Starter {
        name: "Bold",
        line: "One UI cards on islands",
        preset: "islands",
        widgets: &[("clock", [3, 2]), ("system-monitor", [3, 2])],
    },
    Starter {
        name: "Quiet",
        line: "Mono, just a top bar",
        preset: "monochrome",
        widgets: &[],
    },
];

/// The names and descriptions used by the first-run chooser.
pub(crate) fn panel_starters() -> Vec<(&'static str, &'static str)> {
    STARTERS.iter().map(|s| (s.name, s.line)).collect()
}

/// A starter's bar preset and widgets. Starters never change the wallpaper.
pub(crate) fn panel_starter(index: usize) -> Option<(&'static str, &'static [(&'static str, [u32; 2])])> {
    STARTERS.get(index).map(|s| (s.preset, s.widgets))
}
