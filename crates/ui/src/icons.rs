//! Icons. Views name an [`Icon`]; the theme decides how it is drawn:
//!
//! - **Vector** ([`IconSet::Vector`], every style but Fluent): Lucide's outlines (ISC,
//!   `crates/ui/icons/`), compiled into the crate as path data ([`crate::icon_paths`]), drawn by
//!   Direct2D as round-capped strokes ([`Op::Icon`]): SF Symbols' calm line weight at any size.
//!   Media glyphs are filled, as SF's `play.fill`. Chosen over the icon font after rendering
//!   both side by side (`docs/media/native/kit-icons.png`): the font's glyphs are hinted to
//!   Windows' own look and thin out on glass.
//! - **Font** ([`IconSet::Font`], the Fluent style): glyphs of Segoe Fluent Icons drawn as
//!   text. Windows 10 has Segoe MDL2 Assets, with the same outlined code points.
//!
//! [`centered`] places a font glyph (the older API, kept while surfaces move to the kit's
//! `Frame::icon`, which picks the set from the theme).

use std::sync::Arc;

use crate::geom::{Point, Rect, Rgba};
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

/// How a theme draws icons.
#[derive(Debug, Clone, PartialEq)]
pub enum IconSet {
    /// Lucide outlines as Direct2D geometry.
    Vector,
    /// Glyphs of an icon font (Segoe Fluent Icons).
    Font(Arc<str>),
}

/// A named icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    // Media (filled, as media controls are everywhere else).
    Play,
    Pause,
    Previous,
    Next,
    Music,
    Shuffle,
    Repeat,
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
    Grip,
    ArrowUp,
    ArrowDown,
    External,
    Download,
    Upload,
    // Shell.
    Apps,
    Window,
    Power,
    Restart,
    SignOut,
    Lock,
    Bell,
    BellOff,
    Calendar,
    Clock,
    Timer,
    Globe,
    Keyboard,
    Desktop,
    Home,
    User,
    Folder,
    File,
    Clipboard,
    Terminal,
    Message,
    Controls,
    Layers,
    // Status.
    Volume(u8),
    Mute,
    Speaker,
    Headphones,
    Mic,
    Wifi(u8),
    Ethernet,
    Offline,
    Hotspot,
    Cast,
    Airplane,
    Location,
    Bluetooth,
    Brightness,
    Moon,
    NightLight,
    Focus,
    Saver,
    Gauge,
    Thermometer,
    Cpu,
    Battery(u8),
    Charging,
    BatteryWarning,
    Plug,
    Accessibility,
    Eye,
    Camera,
    Info,
    Warning,
    Star,
    Heart,
    Notes,
    Tasks,
    Photo,
    Feed,
    // Weather.
    Sun,
    Cloud,
    CloudSun,
    CloudMoon,
    Rain,
    Drizzle,
    Snow,
    Snowflake,
    Thunder,
    Fog,
    Wind,
    Droplet,
    Umbrella,
    Sunrise,
    Sunset,
}

impl Icon {
    /// The glyph in Segoe Fluent Icons. Levels (volume 0..=3, Wi-Fi 0..=3, battery 0..=10)
    /// are clamped. Icons the font has no glyph for give its "unknown" glyph; the kit draws
    /// those as vectors in every style ([`Icon::font_glyph`]).
    pub const fn glyph(self) -> char {
        match self.font_glyph() {
            Some(c) => c,
            None => '\u{E9CE}',
        }
    }

    /// The glyph in Segoe Fluent Icons, when it has one (code points checked against a
    /// rendered sheet of the font on Windows 11, 26300).
    pub const fn font_glyph(self) -> Option<char> {
        Some(match self {
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
            Self::Speaker => '\u{E7F5}',
            Self::Wifi(level) => match level {
                0 => '\u{E872}',
                1 => '\u{E873}',
                2 => '\u{E874}',
                _ => '\u{E701}',
            },
            Self::Ethernet => '\u{E839}',
            Self::Offline => '\u{E871}',
            Self::Bluetooth => '\u{E702}',
            Self::Brightness | Self::Sun => '\u{E706}',
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
            Self::Cloud => '\u{E753}',
            Self::Home => '\u{E80F}',
            Self::Folder => '\u{E8B7}',
            Self::Info => '\u{E946}',
            Self::Warning => '\u{E7BA}',
            _ => return None,
        })
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

    /// Whether the vector is filled as well as stroked (media, as SF's `.fill` symbols).
    pub const fn filled(self) -> bool {
        matches!(self, Self::Play | Self::Pause | Self::Previous | Self::Next)
    }

    /// The Lucide outline this icon draws.
    pub(crate) const fn lucide(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Previous => "skip-back",
            Self::Next => "skip-forward",
            Self::Music => "music",
            Self::Shuffle => "shuffle",
            Self::Repeat => "repeat",
            Self::ChevronLeft => "chevron-left",
            Self::ChevronRight => "chevron-right",
            Self::ChevronUp => "chevron-up",
            Self::ChevronDown => "chevron-down",
            Self::Add => "plus",
            Self::Remove => "minus",
            Self::Close => "x",
            Self::Check => "check",
            Self::More => "ellipsis",
            Self::Settings => "settings",
            Self::Search => "search",
            Self::Edit => "pencil",
            Self::Delete => "trash-2",
            Self::Copy => "copy",
            Self::Refresh => "refresh-cw",
            Self::Pin => "pin",
            Self::Resize => "maximize-2",
            Self::Grip => "grip-vertical",
            Self::ArrowUp => "arrow-up",
            Self::ArrowDown => "arrow-down",
            Self::External => "external-link",
            Self::Download => "download",
            Self::Upload => "upload",
            Self::Apps => "layout-grid",
            Self::Window => "app-window",
            Self::Power => "power",
            Self::Restart => "rotate-cw",
            Self::SignOut => "log-out",
            Self::Lock => "lock",
            Self::Bell => "bell",
            Self::BellOff => "bell-off",
            Self::Calendar => "calendar",
            Self::Clock => "clock",
            Self::Timer => "timer",
            Self::Globe => "globe",
            Self::Keyboard => "keyboard",
            Self::Desktop => "monitor",
            Self::Home => "house",
            Self::User => "circle-user-round",
            Self::Folder => "folder",
            Self::File => "file",
            Self::Clipboard => "clipboard",
            Self::Terminal => "terminal",
            Self::Message => "message-square",
            Self::Controls => "sliders-horizontal",
            Self::Layers => "layers",
            Self::Volume(0) => "volume",
            Self::Volume(1) => "volume-1",
            Self::Volume(_) => "volume-2",
            Self::Mute => "volume-x",
            Self::Speaker => "speaker",
            Self::Headphones => "headphones",
            Self::Mic => "mic",
            Self::Wifi(0) => "wifi-zero",
            Self::Wifi(1) => "wifi-low",
            Self::Wifi(2) => "wifi-high",
            Self::Wifi(_) => "wifi",
            Self::Ethernet => "ethernet-port",
            Self::Offline => "wifi-off",
            Self::Hotspot => "radio-tower",
            Self::Cast => "cast",
            Self::Airplane => "plane",
            Self::Location => "map-pin",
            Self::Bluetooth => "bluetooth",
            Self::Brightness | Self::Sun => "sun",
            Self::Moon | Self::Focus => "moon",
            Self::NightLight => "sun-moon",
            Self::Saver => "leaf",
            Self::Gauge => "gauge",
            Self::Thermometer => "thermometer",
            Self::Cpu => "cpu",
            Self::Battery(0..=1) => "battery",
            Self::Battery(2..=4) => "battery-low",
            Self::Battery(5..=7) => "battery-medium",
            Self::Battery(_) => "battery-full",
            Self::Charging => "battery-charging",
            Self::BatteryWarning => "battery-warning",
            Self::Plug => "plug",
            Self::Accessibility => "accessibility",
            Self::Eye => "eye",
            Self::Camera => "camera",
            Self::Info => "info",
            Self::Warning => "triangle-alert",
            Self::Star => "star",
            Self::Heart => "heart",
            Self::Notes => "notebook-pen",
            Self::Tasks => "list-checks",
            Self::Photo => "image",
            Self::Feed => "rss",
            Self::Cloud => "cloud",
            Self::CloudSun => "cloud-sun",
            Self::CloudMoon => "cloud-moon",
            Self::Rain => "cloud-rain",
            Self::Drizzle => "cloud-drizzle",
            Self::Snow => "cloud-snow",
            Self::Snowflake => "snowflake",
            Self::Thunder => "cloud-lightning",
            Self::Fog => "cloud-fog",
            Self::Wind => "wind",
            Self::Droplet => "droplet",
            Self::Umbrella => "umbrella",
            Self::Sunrise => "sunrise",
            Self::Sunset => "sunset",
        }
    }

    /// The outline as path segments on Lucide's 24-unit grid.
    pub(crate) fn segments(self) -> Vec<Seg> {
        let name = self.lucide();
        crate::icon_paths::PATHS
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(Vec::new, |(_, d)| path(d))
    }
}

/// `icon` at `size` device pixels, centred in `rect` by its measured box (a font glyph).
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

/// One absolute path segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quad(Point, Point),
    /// SVG's elliptical arc: radii, x-axis rotation in degrees, large-arc and sweep flags.
    Arc {
        to: Point,
        rx: f32,
        ry: f32,
        rotation: f32,
        large: bool,
        sweep: bool,
    },
    Close,
}

/// Parses SVG path data into absolute segments (smooth curves expanded). Malformed data ends
/// the path where it breaks: icon data is generated, so that never happens in practice.
pub(crate) fn path(d: &str) -> Vec<Seg> {
    let mut lex = Lexer {
        s: d.as_bytes(),
        i: 0,
    };
    let mut out = Vec::new();
    let (mut at, mut start) = (Point::default(), Point::default());
    // The last control point, for S and T.
    let mut last_cubic: Option<Point> = None;
    let mut last_quad: Option<Point> = None;
    let mut command = b'M';
    while let Some(next) = lex.command_or_number() {
        if let Some(c) = next {
            command = c;
        }
        let rel = command.is_ascii_lowercase();
        let pt = |lex: &mut Lexer, at: Point| -> Option<Point> {
            let (x, y) = (lex.number()?, lex.number()?);
            Some(if rel {
                Point::new(at.x + x, at.y + y)
            } else {
                Point::new(x, y)
            })
        };
        let (mut cubic, mut quad) = (None, None);
        let ok = (|| -> Option<()> {
            match command.to_ascii_uppercase() {
                b'M' => {
                    at = pt(&mut lex, at)?;
                    start = at;
                    out.push(Seg::Move(at));
                    // Further pairs are line-tos.
                    command = if rel { b'l' } else { b'L' };
                }
                b'L' => {
                    at = pt(&mut lex, at)?;
                    out.push(Seg::Line(at));
                }
                b'H' => {
                    let x = lex.number()?;
                    at = Point::new(if rel { at.x + x } else { x }, at.y);
                    out.push(Seg::Line(at));
                }
                b'V' => {
                    let y = lex.number()?;
                    at = Point::new(at.x, if rel { at.y + y } else { y });
                    out.push(Seg::Line(at));
                }
                b'C' => {
                    let (a, b, to) = (pt(&mut lex, at)?, pt(&mut lex, at)?, pt(&mut lex, at)?);
                    out.push(Seg::Cubic(a, b, to));
                    (cubic, at) = (Some(b), to);
                }
                b'S' => {
                    let a = reflect(last_cubic, at);
                    let (b, to) = (pt(&mut lex, at)?, pt(&mut lex, at)?);
                    out.push(Seg::Cubic(a, b, to));
                    (cubic, at) = (Some(b), to);
                }
                b'Q' => {
                    let (a, to) = (pt(&mut lex, at)?, pt(&mut lex, at)?);
                    out.push(Seg::Quad(a, to));
                    (quad, at) = (Some(a), to);
                }
                b'T' => {
                    let a = reflect(last_quad, at);
                    let to = pt(&mut lex, at)?;
                    out.push(Seg::Quad(a, to));
                    (quad, at) = (Some(a), to);
                }
                b'A' => {
                    let (rx, ry, rotation) = (lex.number()?, lex.number()?, lex.number()?);
                    let (large, sweep) = (lex.flag()?, lex.flag()?);
                    let to = pt(&mut lex, at)?;
                    out.push(Seg::Arc {
                        to,
                        rx: rx.abs(),
                        ry: ry.abs(),
                        rotation,
                        large,
                        sweep,
                    });
                    at = to;
                }
                b'Z' => {
                    out.push(Seg::Close);
                    at = start;
                }
                _ => return None,
            }
            Some(())
        })();
        if ok.is_none() {
            break;
        }
        (last_cubic, last_quad) = (cubic, quad);
    }
    out
}

fn reflect(control: Option<Point>, at: Point) -> Point {
    control.map_or(at, |c| Point::new(2.0 * at.x - c.x, 2.0 * at.y - c.y))
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn skip(&mut self) {
        while self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
        {
            self.i += 1;
        }
    }

    /// `Some(Some(command))`, `Some(None)` when a number follows (an implicit repeat), or
    /// `None` at the end.
    fn command_or_number(&mut self) -> Option<Option<u8>> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(Some(c))
        } else {
            Some(None)
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        let mut dot = false;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' => {}
                b'.' if !dot => dot = true,
                b'e' | b'E' => {
                    self.i += 1;
                    if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
                        self.i += 1;
                    }
                    continue;
                }
                _ => break,
            }
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }

    /// An arc flag: one character, possibly with no separator after it.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
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

    #[test]
    fn path_data_parses_compact_arcs_relative_moves_and_smooth_curves() {
        // Flags without separators, a number starting with '.', implicit line-tos.
        let segs = path("M2 8.82a15 15 0 0120 0m-5 4l1-1 2 2h-3z");
        assert_eq!(segs[0], Seg::Move(Point::new(2.0, 8.82)));
        assert!(matches!(segs[1], Seg::Arc { to, large: false, sweep: true, .. }
            if (to.x - 22.0).abs() < 1e-4 && (to.y - 8.82).abs() < 1e-4));
        assert_eq!(segs[2], Seg::Move(Point::new(17.0, 12.82)));
        assert_eq!(segs[4], Seg::Line(Point::new(20.0, 13.82)));
        assert_eq!(segs.last(), Some(&Seg::Close));
        let smooth = path("M0 0c1 0 2 1 2 2s1 2 2 2");
        assert_eq!(
            smooth[2],
            Seg::Cubic(
                Point::new(2.0, 3.0),
                Point::new(3.0, 4.0),
                Point::new(4.0, 4.0)
            )
        );
    }

    #[test]
    fn every_icon_has_an_outline() {
        let all = [
            Icon::Play,
            Icon::Wifi(2),
            Icon::Battery(3),
            Icon::Volume(0),
            Icon::BellOff,
            Icon::Thunder,
            Icon::Controls,
            Icon::Sunset,
        ];
        for icon in all {
            let segs = icon.segments();
            assert!(
                matches!(segs.first(), Some(Seg::Move(_))) && segs.len() > 1,
                "{icon:?}"
            );
        }
        for (name, d) in crate::icon_paths::PATHS {
            let segs = path(d);
            let ends = segs.iter().filter_map(|s| match s {
                Seg::Move(p) | Seg::Line(p) | Seg::Cubic(_, _, p) | Seg::Quad(_, p) => Some(*p),
                Seg::Arc { to, .. } => Some(*to),
                Seg::Close => None,
            });
            for p in ends {
                assert!(
                    (-1.0..=25.0).contains(&p.x) && (-1.0..=25.0).contains(&p.y),
                    "{name}: {p:?} leaves the grid"
                );
            }
        }
    }
}
