//! Every setting Studio shows from a file, in one table: its label and hint, the document and
//! JSON pointer it edits, its control, and where it lives. Pages draw their rows from it, and
//! the command palette searches it (plans/native/studio-redesign.md, 3.2: "every setting has
//! an id"). Settings that need their own drawing (tiles, lists) are here too, as `Custom`, so
//! search still finds them.

use crate::actions::{Bar, Page, Panel, Section};
use crate::docs::DocId;

/// How a setting is edited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Switch { default: bool },
    /// A few named values (a segmented control, or a select when long). Values are JSON.
    Choice {
        options: &'static [(&'static str, &'static str)],
        default: &'static str,
    },
    /// The same, always shown as a select.
    Select {
        options: &'static [(&'static str, &'static str)],
        default: &'static str,
    },
    /// A number: stored values times `factor` are what the readout shows, with `unit`.
    Slider {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
        factor: f64,
        default: f64,
    },
    Text { placeholder: &'static str },
    /// Drawn by its page (tiles, lists, colours).
    Custom,
}

/// Where a setting lives: the palette goes there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Page(Page),
    /// Look, on its Windows tab or not.
    Look { windows: bool },
    /// Bars with a bar (or a panel) selected; `None` is the overview.
    Bars(Option<Bar>),
    BarsPanel(Panel),
    Settings(Section),
}

impl Place {
    /// The breadcrumb the palette shows ("Look › Windows").
    pub fn crumb(self) -> &'static str {
        match self {
            Place::Page(Page::Home) => "Home",
            Place::Page(Page::Wallpapers) => "Wallpapers",
            Place::Page(Page::Widgets) => "Widgets",
            Place::Page(Page::Integrations) => "Integrations",
            Place::Page(_) => "Studio",
            Place::Look { windows: false } => "Look",
            Place::Look { windows: true } => "Look › Windows",
            Place::Bars(None) => "Bars",
            Place::Bars(Some(Bar::Top)) => "Bars › Top bar",
            Place::Bars(Some(Bar::Dock)) => "Bars › Dock",
            Place::BarsPanel(Panel::Start) => "Bars › Start menu",
            Place::BarsPanel(Panel::Calendar) => "Bars › Calendar",
            Place::BarsPanel(Panel::Sound) => "Bars › Sound",
            Place::BarsPanel(Panel::Tray) => "Bars › Tray",
            Place::BarsPanel(Panel::Notifications) => "Bars › Notifications",
            Place::Settings(Section::General) => "Settings › General",
            Place::Settings(Section::Performance) => "Settings › Performance and power",
            Place::Settings(Section::Desktop) => "Settings › Desktop",
            Place::Settings(Section::Keyboard) => "Settings › Keyboard",
            Place::Settings(Section::Troubleshoot) => "Settings › Troubleshoot",
            Place::Settings(Section::About) => "Settings › About",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub doc: DocId,
    pub path: &'static str,
    pub kind: Kind,
    pub place: Place,
    /// Inside a More disclosure (named): search opens it.
    pub more: Option<&'static str>,
    /// Other words people search with.
    pub keywords: &'static str,
}

impl Setting {
    /// Finds a setting by id. Pages name their rows by id, so a typo is a bug: it panics in
    /// debug builds' tests (see the test below), and draws nothing in release.
    pub fn get(id: &str) -> Option<&'static Setting> {
        SETTINGS.iter().find(|s| s.id == id)
    }
}

const fn sw(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    doc: DocId,
    path: &'static str,
    default: bool,
    place: Place,
) -> Setting {
    Setting {
        id,
        label,
        hint,
        doc,
        path,
        kind: Kind::Switch { default },
        place,
        more: None,
        keywords: "",
    }
}

#[allow(clippy::too_many_arguments)]
const fn ch(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    doc: DocId,
    path: &'static str,
    options: &'static [(&'static str, &'static str)],
    default: &'static str,
    place: Place,
) -> Setting {
    Setting {
        id,
        label,
        hint,
        doc,
        path,
        kind: Kind::Choice { options, default },
        place,
        more: None,
        keywords: "",
    }
}

#[allow(clippy::too_many_arguments)]
const fn sel(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    doc: DocId,
    path: &'static str,
    options: &'static [(&'static str, &'static str)],
    default: &'static str,
    place: Place,
) -> Setting {
    Setting {
        id,
        label,
        hint,
        doc,
        path,
        kind: Kind::Select { options, default },
        place,
        more: None,
        keywords: "",
    }
}

#[allow(clippy::too_many_arguments)]
const fn sl(
    id: &'static str,
    label: &'static str,
    doc: DocId,
    path: &'static str,
    (min, max, step): (f64, f64, f64),
    unit: &'static str,
    factor: f64,
    default: f64,
    place: Place,
) -> Setting {
    Setting {
        id,
        label,
        hint: "",
        doc,
        path,
        kind: Kind::Slider {
            min,
            max,
            step,
            unit,
            factor,
            default,
        },
        place,
        more: None,
        keywords: "",
    }
}

/// A text value (a colour, a format); clearing it removes the key.
const fn txt(id: &'static str, label: &'static str, doc: DocId, path: &'static str, placeholder: &'static str, place: Place) -> Setting {
    Setting {
        kind: Kind::Text { placeholder },
        ..sw(id, label, "", doc, path, false, place)
    }
}

const fn custom(id: &'static str, label: &'static str, hint: &'static str, place: Place) -> Setting {
    Setting {
        id,
        label,
        hint,
        doc: DocId::Config,
        path: "",
        kind: Kind::Custom,
        place,
        more: None,
        keywords: "",
    }
}

impl Setting {
    const fn more(mut self, group: &'static str) -> Self {
        self.more = Some(group);
        self
    }

    const fn words(mut self, keywords: &'static str) -> Self {
        self.keywords = keywords;
        self
    }

    const fn hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }
}

use DocId::{Bars as B, Config as C, Studio as S, Widgets as W};

const LOOK: Place = Place::Look { windows: false };
const WIN: Place = Place::Look { windows: true };
const TOP: Place = Place::Bars(Some(Bar::Top));
const DOCK: Place = Place::Bars(Some(Bar::Dock));
const BARS: Place = Place::Bars(None);
const GENERAL: Place = Place::Settings(Section::General);
const PERF: Place = Place::Settings(Section::Performance);
const DESKTOP: Place = Place::Settings(Section::Desktop);

const EDGES: &[(&str, &str)] = &[(r#""top""#, "Top"), (r#""bottom""#, "Bottom")];
/// A bar's own style over Look's: the same five the shell draws.
const ON_OFF_STYLE: &[(&str, &str)] = &[
    (r#""liquid""#, "Liquid"),
    (r#""oneui""#, "One UI"),
    (r#""fluent""#, "Fluent"),
    (r#""solid""#, "Solid"),
    (r#""mono""#, "Mono"),
];
const DISPLAYS: &[(&str, &str)] = &[(r#""all""#, "All"), (r#""primary""#, "Main display")];
const SCHEME: &[(&str, &str)] = &[
    (r#""light""#, "Light"),
    (r#""dark""#, "Dark"),
    (r#""auto""#, "Match Windows"),
];
const MOTION_STYLE: &[(&str, &str)] = &[
    (r#""slide""#, "Slide"),
    (r#""fade""#, "Fade"),
    (r#""pop""#, "Pop"),
    (r#""scale""#, "Scale"),
    (r#""morph""#, "Morph"),
    (r#""none""#, "None"),
];
const FAMILY: &[(&str, &str)] = &[
    (r#""mono""#, "mypaper"),
    (r#""fluent""#, "Windows 11"),
    (r#""macos""#, "macOS"),
    (r#""material""#, "ChromeOS"),
];
/// A bar's shadow at rest: its design's, none, or a soft one.
const SHADOW: &[(&str, &str)] = &[
    ("null", "From the design"),
    (r#"{"color":"transparent"}"#, "None"),
    (r##"{"color":"#00000059","y":4,"blur":16}"##, "Soft"),
];
const CURVES: &[(&str, &str)] = &[
    (r#""smooth""#, "Smooth"),
    (r#""snappy""#, "Snappy"),
    (r#""emphasized""#, "Emphasized"),
    (r#""spring""#, "Spring"),
    (r#""spring-soft""#, "Soft spring"),
    (r#""spring-bouncy""#, "Bouncy spring"),
    (r#""ease-out""#, "Ease out"),
    (r#""linear""#, "Linear"),
];

/// The table. Ids are stable: deep links and pages use them.
pub static SETTINGS: &[Setting] = &[
    // --- Settings › General -------------------------------------------------------------------
    custom("autostart", "Start with Windows", "mypaper starts when you sign in", GENERAL)
        .words("startup login boot"),
    sw(
        "static_snapshot",
        "Show the wallpaper while Windows starts",
        "Windows shows a still of it before mypaper runs",
        C,
        "/static_snapshot",
        true,
        GENERAL,
    )
    .words("startup snapshot still"),
    ch(
        "studio_appearance",
        "Studio appearance",
        "",
        S,
        "/appearance",
        &[
            (r#""match""#, "Match Windows"),
            (r#""light""#, "Light"),
            (r#""dark""#, "Dark"),
        ],
        r#""match""#,
        GENERAL,
    )
    .words("theme dark light mode"),
    // Experimental features (`config.json` › `experimental`): new or unproven, each its own switch.
    sw(
        "experimental_file_index",
        "File index",
        "Instant file search and folder sizes in the launcher",
        C,
        "/experimental/file_index",
        false,
        GENERAL,
    )
    .words("experimental search files index folder size"),
    sw(
        "experimental_files_default",
        "Open folders in mypaper Files",
        "Folders and Win+E open in Files instead of Explorer",
        C,
        "/experimental/files_default",
        false,
        GENERAL,
    )
    .words("experimental explorer files win+e default"),
    sw(
        "experimental_app_styler",
        "Restyle inside Explorer and Settings",
        "Can crash some Settings pages and grow Explorer's memory",
        C,
        "/experimental/app_styler",
        false,
        GENERAL,
    )
    .words("experimental explorer settings styler restyle theme"),
    // --- Settings › Performance and power ----------------------------------------------------
    custom("gpu", "Graphics card", "Draws on the card your screens use", PERF)
        .words("gpu integrated discrete nvidia amd intel high performance"),
    sel(
        "fps",
        "Wallpaper frame rate",
        "",
        C,
        "/engine/fps",
        &[
            ("null", "The wallpaper's own"),
            ("24", "24 fps"),
            ("30", "30 fps"),
            ("60", "60 fps"),
            ("120", "120 fps"),
            ("144", "144 fps"),
        ],
        "null",
        PERF,
    )
    .words("fps frame rate cap hz"),
    custom("per_wallpaper", "Per wallpaper", "", PERF),
    sw(
        "pause_on_battery",
        "Pause on battery",
        "The wallpaper rests while the laptop is unplugged",
        C,
        "/pause_on_battery",
        true,
        PERF,
    )
    .words("battery power laptop"),
    sw(
        "pause_on_fullscreen",
        "Pause behind fullscreen apps",
        "Games and videos get the whole graphics card",
        C,
        "/pause_on_fullscreen",
        true,
        PERF,
    )
    .words("games fullscreen"),
    ch(
        "save_power",
        "Save power",
        "Fewer animations and no blur on every surface",
        B,
        "/performance",
        &[
            (r#""auto""#, "On battery"),
            (r#""on""#, "Always"),
            (r#""off""#, "Never"),
        ],
        r#""auto""#,
        PERF,
    )
    .words("performance battery blur"),
    ch(
        "reduce_motion",
        "Reduce motion",
        "",
        B,
        "/animation/reduceMotion",
        &[
            (r#""system""#, "Like Windows"),
            (r#""always""#, "Always"),
            (r#""never""#, "Never"),
        ],
        r#""system""#,
        PERF,
    )
    .words("animation motion accessibility"),
    // --- Settings › Desktop ---------------------------------------------------------------------
    sw(
        "desktop_menu",
        "Use mypaper's right-click menu",
        "On the bare desktop",
        C,
        "/desktop_menu/enabled",
        true,
        DESKTOP,
    )
    .words("context menu right click"),
    custom("desktop_menu_items", "Menu items", "Reorder, hide or add your own", DESKTOP)
        .words("context menu items custom"),
    sw(
        "classic_menu",
        "Full menus in Explorer",
        "The classic Windows 11 right-click menu everywhere",
        C,
        "/classic_context_menu",
        false,
        DESKTOP,
    )
    .words("classic context menu show more options explorer"),
    // --- Look › Desktop -------------------------------------------------------------------------
    custom("style", "Style", "Liquid, One UI, Solid, Fluent or Mono", LOOK)
        .words("liquid glass oneui solid fluent mono theme"),
    sl(
        "clarity",
        "Clarity",
        C,
        "/look/material/clarity",
        (0.0, 1.0, 0.01),
        "%",
        100.0,
        0.88,
        LOOK,
    )
    .words("opacity transparency tint glass"),
    sl(
        "blur",
        "Blur",
        C,
        "/look/material/blur",
        (0.0, 32.0, 1.0),
        "px",
        1.0,
        12.0,
        LOOK,
    )
    .words("frosted glass"),
    sl(
        "refraction",
        "Refraction",
        C,
        "/look/material/refraction",
        (0.0, 1.0, 0.01),
        "%",
        100.0,
        0.6,
        LOOK,
    )
    .words("bend lens liquid"),
    sl(
        "highlight",
        "Highlight",
        C,
        "/look/material/highlight",
        (0.0, 1.0, 0.01),
        "%",
        100.0,
        0.55,
        LOOK,
    )
    .words("shine specular rim"),
    custom("tint", "Tint", "The glass colour", LOOK).words("color colour surface"),
    ch(
        "appearance",
        "Appearance",
        "",
        C,
        "/look/appearance",
        SCHEME,
        r#""dark""#,
        LOOK,
    )
    .words("dark light mode"),
    custom("accent", "Accent", "One accent for everything", LOOK).words("color colour highlight"),
    Setting {
        kind: Kind::Text {
            placeholder: "Segoe UI Variable",
        },
        ..sw("font", "Font", "", C, "/look/text/font", false, LOOK)
    }
    .more("Text and icons")
    .words("typeface family"),
    sl(
        "text_size",
        "Size",
        C,
        "/look/text/size",
        (0.5, 2.0, 0.05),
        "%",
        100.0,
        1.0,
        LOOK,
    )
    .more("Text and icons")
    .words("scale text size"),
    sw(
        "text_shadow",
        "Text shadow",
        "Legible on any wallpaper",
        C,
        "/look/text/shadow",
        true,
        LOOK,
    )
    .more("Text and icons"),
    ch(
        "text_tone",
        "Text on the wallpaper",
        "",
        C,
        "/look/text/tone",
        &[
            (r#""auto""#, "Auto"),
            (r#""light""#, "Light"),
            (r#""dark""#, "Dark"),
        ],
        r#""auto""#,
        LOOK,
    )
    .more("Text and icons"),
    ch(
        "icon_style",
        "Icon style",
        "",
        C,
        "/look/text/icons",
        &[(r#""lucide""#, "Lucide"), (r#""fluent""#, "Fluent")],
        r#""fluent""#,
        LOOK,
    )
    .more("Text and icons")
    .words("glyphs symbols"),
    custom("icon_pack", "Icon pack", "The dock, top bar and launcher", LOOK)
        .more("Text and icons")
        .words("icons apps pack"),
    sl(
        "corners",
        "Corners",
        C,
        "/look/shape/corners",
        (0.0, 32.0, 1.0),
        "px",
        1.0,
        16.0,
        LOOK,
    )
    .more("Shape")
    .words("radius rounded"),
    sw(
        "edge",
        "Edge line",
        "A hairline around every surface",
        C,
        "/look/shape/edge",
        true,
        LOOK,
    )
    .more("Shape")
    .words("border outline"),
    custom("different", "Different for", "Bars, widgets, or menus and panels", LOOK)
        .more("Different for")
        .words("override bars widgets menus"),
    // --- Look › Windows -------------------------------------------------------------------------
    custom("windows_look", "Windows look", "As is, Match mypaper, Glass or Sharp", WIN)
        .words("windows styling dwm"),
    custom("windows_accent", "Accent", "", WIN).words("accent colour windows taskbar start"),
    ch(
        "windows_apps",
        "Apps",
        "",
        C,
        "/look/windows/apps",
        &[
            (r#""match""#, "Match"),
            (r#""light""#, "Light"),
            (r#""dark""#, "Dark"),
            (r#""as-is""#, "As is"),
        ],
        r#""as-is""#,
        WIN,
    )
    .hint("Light or dark apps")
    .words("apps light dark mode"),
    ch(
        "windows_corners",
        "Corners",
        "",
        C,
        "/look/windows/corners",
        &[
            ("null", "As is"),
            (r#""round""#, "Round"),
            (r#""small""#, "Small"),
            (r#""square""#, "Square"),
        ],
        "null",
        WIN,
    )
    .words("window corners rounded square"),
    sel(
        "windows_material",
        "Window material",
        "",
        C,
        "/look/windows/backdrop",
        &[
            ("null", "As is"),
            (r#""mica""#, "Mica"),
            (r#""acrylic""#, "Acrylic"),
            (r#""tabbed""#, "Mica tabbed"),
            (r#""none""#, "None"),
            (r#""auto""#, "Windows decides"),
        ],
        "null",
        WIN,
    )
    .words("mica acrylic backdrop"),
    custom("pointer", "Pointer", "", WIN).words("cursor mouse pointer dark light"),
    custom("window_edge", "Window edge", "", WIN)
        .more("Edges and title bars")
        .words("border edge"),
    custom("title_bar", "Title bar", "", WIN)
        .more("Edges and title bars")
        .words("caption"),
    custom("title_text", "Title text", "", WIN)
        .more("Edges and title bars")
        .words("caption text"),
    // --- Bars -----------------------------------------------------------------------------------
    sw("top_enabled", "Top bar", "", B, "/top/enabled", true, BARS).words("menu bar"),
    sw("dock_enabled", "Dock", "", B, "/dock/enabled", true, BARS).words("taskbar"),
    custom("preset", "Presets", "macOS, Windows 11, ChromeOS…", BARS)
        .words("pairing macos windows chromeos islands"),
    ch(
        "taskbar_mode",
        "Windows taskbar",
        "",
        B,
        "/taskbar/mode",
        &[
            (r#""keep""#, "Keep"),
            (r#""autohide""#, "Auto-hide"),
            (r#""hide""#, "Hide"),
        ],
        r#""keep""#,
        BARS,
    )
    .words("hide taskbar"),
    ch(
        "taskbar_alignment",
        "Taskbar icons",
        "",
        B,
        "/taskbar/alignment",
        &[
            ("null", "As is"),
            (r#""left""#, "Left"),
            (r#""center""#, "Center"),
        ],
        "null",
        BARS,
    )
    .words("alignment"),
    custom("top_design", "Design", "", TOP),
    sw("top_hide_maximized", "Hide when a window is maximized", "The dock slides away and comes back when you point at its edge", B, "/top/hide_when_maximized", false, TOP),
    ch(
        "top_edge",
        "Edge",
        "",
        B,
        "/top/position",
        EDGES,
        r#""top""#,
        TOP,
    ),
    sl(
        "top_height",
        "Height",
        B,
        "/top/height",
        (20.0, 64.0, 1.0),
        "px",
        1.0,
        32.0,
        TOP,
    ),
    ch(
        "top_shape",
        "Shape",
        "",
        B,
        "/top/layout",
        &[
            (r#""strip""#, "Strip"),
            (r#""floating""#, "Floating"),
            (r#""islands""#, "Islands"),
        ],
        r#""strip""#,
        TOP,
    ),
    custom("top_width", "Width", "", TOP).more("Layout"),
    sl(
        "top_margin",
        "Margin",
        B,
        "/top/margin",
        (0.0, 32.0, 1.0),
        "px",
        1.0,
        0.0,
        TOP,
    )
    .more("Layout"),
    sl(
        "top_padding",
        "Padding",
        B,
        "/top/padding",
        (0.0, 24.0, 1.0),
        "px",
        1.0,
        5.0,
        TOP,
    )
    .more("Layout"),
    sl(
        "top_spacing",
        "Spacing",
        B,
        "/top/spacing",
        (0.0, 24.0, 1.0),
        "px",
        1.0,
        1.0,
        TOP,
    )
    .more("Layout"),
    sw(
        "top_clock_seconds",
        "Clock seconds",
        "",
        B,
        "/top/clock/seconds",
        false,
        TOP,
    )
    .more("Clock"),
    Setting {
        kind: Kind::Text {
            placeholder: "Thu 5 Nov 10:42",
        },
        ..sw("top_clock_format", "Clock format", "", B, "/top/clock/format", false, TOP)
    }
    .more("Clock"),
    ch(
        "top_displays",
        "Displays",
        "",
        B,
        "/top/monitors",
        DISPLAYS,
        r#""all""#,
        TOP,
    )
    .more("Displays"),
    ch(
        "top_style",
        "Style",
        "Its own, over Look",
        B,
        "/top/look/style",
        ON_OFF_STYLE,
        r#""liquid""#,
        TOP,
    )
    .more("Look"),
    ch(
        "top_scheme",
        "Light or dark",
        "",
        B,
        "/top/look/scheme",
        SCHEME,
        r#""auto""#,
        TOP,
    )
    .more("Look"),
    sl(
        "top_radius",
        "Corners",
        B,
        "/top/look/radius",
        (0.0, 32.0, 1.0),
        "px",
        1.0,
        12.0,
        TOP,
    )
    .more("Look"),
    sl(
        "top_font_size",
        "Text size",
        B,
        "/top/look/font/size",
        (9.0, 24.0, 0.5),
        "px",
        1.0,
        12.0,
        TOP,
    )
    .more("Look"),
    sw(
        "top_motion",
        "Animations",
        "",
        B,
        "/top/animation/enabled",
        true,
        TOP,
    )
    .more("Motion"),
    sel(
        "top_motion_style",
        "Appearing",
        "",
        B,
        "/top/animation/style",
        MOTION_STYLE,
        r#""slide""#,
        TOP,
    )
    .more("Motion"),
    sel(
        "top_motion_curve",
        "Curve",
        "",
        B,
        "/top/animation/curve",
        CURVES,
        r#""smooth""#,
        TOP,
    )
    .more("Motion"),
    sl(
        "top_motion_duration",
        "Duration",
        B,
        "/top/animation/duration",
        (0.0, 1000.0, 10.0),
        "ms",
        1.0,
        240.0,
        TOP,
    )
    .more("Motion"),
    // A bar's own look beyond the common few, and each movement's timing (spec 2.4, 2.7).
    sel("top_family", "Drawn like", "", B, "/top/look/family", FAMILY, r#""mono""#, TOP).more("Look"),
    sl("top_edge_width", "Edge line", B, "/top/look/border/rest/width", (0.0, 8.0, 0.5), "px", 1.0, 0.0, TOP).more("Edges and buttons"),
    txt("top_edge_color", "Edge color", B, "/top/look/border/rest/color", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    ch("top_shadow", "Shadow", "", B, "/top/look/shadow/rest", SHADOW, r#"null"#, TOP).more("Edges and buttons"),
    sl("top_button_radius", "Button corners", B, "/top/look/items/radius", (0.0, 24.0, 1.0), "px", 1.0, 8.0, TOP).more("Edges and buttons"),
    txt("top_button_rest_fill", "At rest: fill", B, "/top/look/items/rest/background", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    txt("top_button_rest_text", "At rest: text", B, "/top/look/items/rest/color", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    txt("top_button_hover_fill", "Pointed at: fill", B, "/top/look/items/hover/background", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    txt("top_button_hover_text", "Pointed at: text", B, "/top/look/items/hover/color", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    txt("top_button_active_fill", "Pressed: fill", B, "/top/look/items/active/background", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    txt("top_button_active_text", "Pressed: text", B, "/top/look/items/active/color", "#rrggbb, accent or text", TOP).more("Edges and buttons"),
    sl("top_motion_hover", "Pointing", B, "/top/animation/hover/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, TOP).more("Motion"),
    sl("top_motion_press", "Pressing", B, "/top/animation/press/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, TOP).more("Motion"),
    sl("top_motion_open", "Menus opening", B, "/top/animation/open/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, TOP).more("Motion"),
    sel("dock_family", "Drawn like", "", B, "/dock/look/family", FAMILY, r#""mono""#, DOCK).more("Look"),
    sl("dock_edge_width", "Edge line", B, "/dock/look/border/rest/width", (0.0, 8.0, 0.5), "px", 1.0, 0.0, DOCK).more("Edges and buttons"),
    txt("dock_edge_color", "Edge color", B, "/dock/look/border/rest/color", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    ch("dock_shadow", "Shadow", "", B, "/dock/look/shadow/rest", SHADOW, r#"null"#, DOCK).more("Edges and buttons"),
    sl("dock_button_radius", "Button corners", B, "/dock/look/items/radius", (0.0, 24.0, 1.0), "px", 1.0, 8.0, DOCK).more("Edges and buttons"),
    txt("dock_button_rest_fill", "At rest: fill", B, "/dock/look/items/rest/background", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_button_rest_text", "At rest: text", B, "/dock/look/items/rest/color", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_button_hover_fill", "Pointed at: fill", B, "/dock/look/items/hover/background", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_button_hover_text", "Pointed at: text", B, "/dock/look/items/hover/color", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_button_active_fill", "Pressed: fill", B, "/dock/look/items/active/background", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_button_active_text", "Pressed: text", B, "/dock/look/items/active/color", "#rrggbb, accent or text", DOCK).more("Edges and buttons"),
    txt("dock_indicator_running", "Running", B, "/dock/look/indicator/running", "#rrggbb, accent or text", DOCK).more("Running indicators"),
    txt("dock_indicator_active", "In front", B, "/dock/look/indicator/active", "#rrggbb, accent or text", DOCK).more("Running indicators"),
    txt("dock_indicator_attention", "Wants attention", B, "/dock/look/indicator/attention", "#rrggbb, accent or text", DOCK).more("Running indicators"),
    sel("dock_motion_curve", "Curve", "", B, "/dock/animation/curve", CURVES, r#""smooth""#, DOCK).more("Motion"),
    sl("dock_motion_duration", "Duration", B, "/dock/animation/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 240.0, DOCK).more("Motion"),
    sl("dock_motion_hover", "Pointing", B, "/dock/animation/hover/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, DOCK).more("Motion"),
    sl("dock_motion_press", "Pressing", B, "/dock/animation/press/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, DOCK).more("Motion"),
    sl("dock_motion_open", "Menus opening", B, "/dock/animation/open/duration", (0.0, 1000.0, 10.0), "ms", 1.0, 160.0, DOCK).more("Motion"),
    sl("dock_motion_magnify", "Magnify", B, "/dock/animation/bar/dock/magnify/duration", (0.0, 2000.0, 10.0), "ms", 1.0, 300.0, DOCK).more("Motion"),
    sl("dock_motion_bounce", "Launch bounce", B, "/dock/animation/bar/dock/bounce/duration", (0.0, 2000.0, 10.0), "ms", 1.0, 300.0, DOCK).more("Motion"),
    sl("dock_motion_attention", "Attention", B, "/dock/animation/bar/dock/attention/duration", (0.0, 2000.0, 10.0), "ms", 1.0, 300.0, DOCK).more("Motion"),
    custom("top_modules", "Modules", "Start, middle and end", TOP).words("modules clock tray"),
    custom("dock_design", "Design", "", DOCK),
    ch(
        "dock_style",
        "Style",
        "",
        B,
        "/dock/style",
        &[(r#""floating""#, "Floating"), (r#""taskbar""#, "Taskbar")],
        r#""floating""#,
        DOCK,
    ),
    ch(
        "dock_edge",
        "Edge",
        "",
        B,
        "/dock/position",
        &[
            (r#""bottom""#, "Bottom"),
            (r#""left""#, "Left"),
            (r#""right""#, "Right"),
            (r#""top""#, "Top"),
        ],
        r#""bottom""#,
        DOCK,
    ),
    sl(
        "dock_size",
        "Icon size",
        B,
        "/dock/size",
        (24.0, 96.0, 1.0),
        "px",
        1.0,
        48.0,
        DOCK,
    ),
    custom("dock_hide", "Visibility", "Hidden docks reveal at the edge and hide a second after you leave", DOCK)
        .words("autohide auto hide maximized screen always Mac"),
    sw(
        "dock_magnify",
        "Magnify",
        "",
        B,
        "/dock/magnify/enabled",
        false,
        DOCK,
    ),
    sl(
        "dock_margin",
        "Margin",
        B,
        "/dock/margin",
        (0.0, 32.0, 1.0),
        "px",
        1.0,
        8.0,
        DOCK,
    )
    .more("Layout"),
    sl(
        "dock_padding",
        "Padding",
        B,
        "/dock/padding",
        (0.0, 24.0, 1.0),
        "px",
        1.0,
        5.0,
        DOCK,
    )
    .more("Layout"),
    sl(
        "dock_spacing",
        "Spacing",
        B,
        "/dock/spacing",
        (0.0, 24.0, 1.0),
        "px",
        1.0,
        6.0,
        DOCK,
    )
    .more("Layout"),
    sl(
        "dock_magnify_scale",
        "Magnify scale",
        B,
        "/dock/magnify/scale",
        (1.0, 2.5, 0.05),
        "%",
        100.0,
        1.5,
        DOCK,
    )
    .more("Layout"),
    sl(
        "dock_magnify_range",
        "Magnify reach",
        B,
        "/dock/magnify/range",
        (1.0, 4.0, 0.5),
        "",
        1.0,
        2.0,
        DOCK,
    )
    .more("Layout"),
    ch(
        "dock_align",
        "Apps",
        "Where the taskbar's apps sit",
        B,
        "/dock/align",
        &[(r#""start""#, "Start"), (r#""center""#, "Center")],
        r#""center""#,
        DOCK,
    )
    .more("Taskbar"),
    sw(
        "dock_titles",
        "Window titles",
        "",
        B,
        "/dock/titles",
        false,
        DOCK,
    )
    .more("Taskbar"),
    // Missing: the icon plus 16, or 168 with titles; the slider starts at the icon's.
    sl(
        "dock_button",
        "Button length",
        B,
        "/dock/button",
        (16.0, 240.0, 4.0),
        "px",
        1.0,
        48.0,
        DOCK,
    )
    .more("Taskbar"),
    sw(
        "dock_clock_seconds",
        "Clock seconds",
        "The taskbar's clock",
        B,
        "/dock/clock/seconds",
        false,
        DOCK,
    )
    .more("Taskbar"),
    ch(
        "dock_running",
        "Running apps",
        "",
        B,
        "/dock/running",
        &[
            ("true", "All"),
            (r#""monitor""#, "This display"),
            ("false", "Pinned only"),
        ],
        "true",
        DOCK,
    )
    .more("Apps"),
    sw(
        "dock_split",
        "One per window",
        "",
        B,
        "/dock/split",
        false,
        DOCK,
    )
    .more("Apps"),
    sw(
        "dock_counter",
        "Count badge",
        "",
        B,
        "/dock/counter",
        false,
        DOCK,
    )
    .more("Apps"),
    sw(
        "dock_previews",
        "Window previews",
        "",
        B,
        "/dock/preview/enabled",
        true,
        DOCK,
    )
    .more("Apps"),
    sl(
        "dock_preview_delay",
        "Preview delay",
        B,
        "/dock/preview/delay",
        (0.0, 2000.0, 50.0),
        "ms",
        1.0,
        500.0,
        DOCK,
    )
    .more("Apps"),
    sw(
        "dock_bounce",
        "Bounce on launch",
        "",
        B,
        "/dock/bounce",
        true,
        DOCK,
    )
    .more("Apps"),
    ch(
        "dock_attention",
        "Attention",
        "",
        B,
        "/dock/attention",
        &[
            (r#""pulse""#, "Pulse"),
            (r#""bounce""#, "Bounce"),
            (r#""none""#, "None"),
        ],
        r#""pulse""#,
        DOCK,
    )
    .more("Apps"),
    sel(
        "dock_indicator",
        "Running indicator",
        "",
        B,
        "/dock/indicator",
        &[
            (r#""dot""#, "Dot"),
            (r#""bar""#, "Bar"),
            (r#""pill""#, "Pill"),
            (r#""glow""#, "Glow"),
            (r#""none""#, "None"),
        ],
        r#""dot""#,
        DOCK,
    )
    .more("Apps"),
    custom("dock_extras", "After the apps", "Show desktop, Recycle Bin", DOCK).more("Apps"),
    ch(
        "dock_displays",
        "Displays",
        "",
        B,
        "/dock/monitors",
        DISPLAYS,
        r#""all""#,
        DOCK,
    )
    .more("Displays"),
    ch(
        "dock_style_own",
        "Style",
        "Its own, over Look",
        B,
        "/dock/look/style",
        ON_OFF_STYLE,
        r#""liquid""#,
        DOCK,
    )
    .more("Look"),
    sl(
        "dock_radius",
        "Corners",
        B,
        "/dock/look/radius",
        (0.0, 40.0, 1.0),
        "px",
        1.0,
        18.0,
        DOCK,
    )
    .more("Look"),
    sw(
        "dock_motion",
        "Animations",
        "",
        B,
        "/dock/animation/enabled",
        true,
        DOCK,
    )
    .more("Motion"),
    sel(
        "dock_motion_style",
        "Appearing",
        "",
        B,
        "/dock/animation/style",
        MOTION_STYLE,
        r#""slide""#,
        DOCK,
    )
    .more("Motion"),
    custom("dock_pinned", "Pinned apps", "Windows' pins or your own", DOCK)
        .words("pins favourites apps"),
    custom("dock_modules", "Modules", "Beside the apps (taskbar style)", DOCK),
    custom("start_pinned", "Pinned apps", "Start's own, or the dock's", Place::BarsPanel(Panel::Start))
        .words("start pins favourites apps"),
    // --- Bars › Panels --------------------------------------------------------------------------
    sel(
        "start_layout",
        "Layout",
        "",
        B,
        "/launcher/layout",
        &[(r#""win11""#, "Windows 11"), (r#""grid""#, "Grid")],
        r#""win11""#,
        Place::BarsPanel(Panel::Start),
    )
    .words("start menu launcher"),
    sw(
        "start_files",
        "Search recent files",
        "",
        B,
        "/launcher/files",
        true,
        Place::BarsPanel(Panel::Start),
    )
    .words("start search files"),
    sw(
        "start_win_key",
        "The Windows key opens it",
        "",
        B,
        "/launcher/winKey",
        true,
        Place::BarsPanel(Panel::Start),
    )
    .words("windows key start"),
    sel(
        "calendar_first_day",
        "Week starts on",
        "",
        B,
        "/calendar/firstDay",
        &[
            ("null", "Like Windows"),
            (r#""monday""#, "Monday"),
            (r#""sunday""#, "Sunday"),
            (r#""saturday""#, "Saturday"),
        ],
        "null",
        Place::BarsPanel(Panel::Calendar),
    )
    .words("calendar week first day"),
    custom(
        "calendar_zones",
        "Other time zones",
        "Clocks under the calendar",
        Place::BarsPanel(Panel::Calendar),
    )
    .words("world clock time zone"),
    sw(
        "sound_switch",
        "Switch the output device",
        "From the sound panel",
        B,
        "/audio/switchOutput",
        true,
        Place::BarsPanel(Panel::Sound),
    )
    .words("audio speakers headphones mixer"),
    sw(
        "tray_host",
        "Collect the tray icons",
        "Not while the Windows taskbar is hidden",
        B,
        "/tray/host",
        true,
        Place::BarsPanel(Panel::Tray),
    )
    .words("notification area tray icons"),
    // --- Widgets ----------------------------------------------------------------------------------
    sw(
        "always_on_top",
        "Always on top",
        "Widgets float over your windows",
        W,
        "/overlay",
        false,
        Place::Page(Page::Widgets),
    )
    .words("overlay widgets top"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_values_are_json() {
        let mut ids: Vec<_> = SETTINGS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate setting ids");
        for s in SETTINGS {
            if let Kind::Choice { options, default } | Kind::Select { options, default } = s.kind {
                for (value, _) in options {
                    serde_json::from_str::<serde_json::Value>(value)
                        .unwrap_or_else(|e| panic!("{}: {value}: {e}", s.id));
                }
                assert!(options.iter().any(|(v, _)| *v == default), "{}: default", s.id);
            }
            if s.kind != Kind::Custom {
                assert!(s.path.starts_with('/'), "{}", s.id);
            }
        }
    }

    /// Each setting, written alone into an empty file with every value it offers, passes the
    /// owning program's parser: the rows point at real fields with real values.
    #[test]
    fn settings_write_what_their_program_accepts() {
        use crate::docs::{DocId, Op, apply_op};
        use serde_json::json;
        for s in SETTINGS.iter().filter(|s| s.doc != DocId::Studio) {
            let values: Vec<serde_json::Value> = match s.kind {
                Kind::Switch { default } => vec![json!(default), json!(!default)],
                Kind::Choice { options, .. } | Kind::Select { options, .. } => {
                    options.iter().map(|(v, _)| serde_json::from_str(v).unwrap()).collect()
                }
                Kind::Slider { min, max, default, .. } => [min, max, default].map(crate::app::number).to_vec(),
                Kind::Text { placeholder } if placeholder.contains("#rrggbb") => vec![json!("#336699"), json!("accent")],
                Kind::Text { .. } | Kind::Custom => continue,
            };
            for value in values {
                let mut root = json!({});
                apply_op(&mut root, &Op { doc: s.doc, path: s.path.into(), value: Some(value.clone()) });
                if let Err(e) = s.doc.parse(&root.to_string()) {
                    panic!("{} = {value}: {e:#}", s.id);
                }
            }
        }
    }
}
