//! The two built-in cursor sets (`mypaper-dark`, `mypaper-light`), drawn in
//! code: simple monochrome shapes with a thin contrasting edge, rasterized
//! from signed distance fields so they are crisp (anti-aliased) at every size.
//! There are no third-party assets: every `.cur` and `.ani` is generated here.
//!
//! Shapes are described on a 32x32 grid ("design units") and rendered at
//! [`SIZES`]; Windows picks the closest size for the user's cursor scale.

use crate::cursors::Role;

/// Which of the two built-in sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Near-black pointers with a white edge: for light and dark backgrounds alike.
    Dark,
    /// White pointers with a near-black edge.
    Light,
}

impl Look {
    /// The set's name, as used in `windows.cursors`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dark => "mypaper-dark",
            Self::Light => "mypaper-light",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Dark, Self::Light]
            .into_iter()
            .find(|look| look.name().eq_ignore_ascii_case(name))
    }

    /// (fill, edge) colors.
    fn colors(self) -> ([u8; 3], [u8; 3]) {
        const INK: [u8; 3] = [0x14, 0x16, 0x1b];
        const PAPER: [u8; 3] = [0xff, 0xff, 0xff];
        match self {
            Self::Dark => (INK, PAPER),
            Self::Light => (PAPER, INK),
        }
    }
}

/// Pixel sizes stored in every cursor file.
pub const SIZES: [u32; 3] = [32, 48, 64];

/// Edge width, in design units (1.5 px at 32 px).
const EDGE: f32 = 1.45;
/// Frames of an animated cursor, and how long each shows (1/60 s).
pub const FRAMES: usize = 12;
const JIFFIES_PER_FRAME: u32 = 5;

type P = (f32, f32);

/// A shape with a signed distance (negative inside), in design units.
#[derive(Clone)]
enum Prim {
    Poly(Vec<P>),
    /// A segment with round ends: a line of some thickness (`radius` is half of it).
    Capsule(P, P, f32),
    Disc(P, f32),
    /// A circle line: center, radius, half thickness.
    Ring(P, f32, f32),
}

impl Prim {
    fn sd(&self, p: P) -> f32 {
        match self {
            Self::Poly(points) => polygon_sd(points, p),
            Self::Capsule(a, b, r) => segment_distance(p, *a, *b) - r,
            Self::Disc(c, r) => hypot(p.0 - c.0, p.1 - c.1) - r,
            Self::Ring(c, r, half) => (hypot(p.0 - c.0, p.1 - c.1) - r).abs() - half,
        }
    }

    fn map(&self, f: impl Fn(P) -> P) -> Self {
        match self {
            Self::Poly(points) => Self::Poly(points.iter().copied().map(f).collect()),
            Self::Capsule(a, b, r) => Self::Capsule(f(*a), f(*b), *r),
            Self::Disc(c, r) => Self::Disc(f(*c), *r),
            Self::Ring(c, r, half) => Self::Ring(f(*c), *r, *half),
        }
    }
}

fn hypot(x: f32, y: f32) -> f32 {
    x.hypot(y)
}

fn segment_distance(p: P, a: P, b: P) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    };
    hypot(apx - t * abx, apy - t * aby)
}

fn polygon_sd(points: &[P], p: P) -> f32 {
    let mut distance = f32::MAX;
    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for &current in points {
        distance = distance.min(segment_distance(p, current, previous));
        // Even-odd crossing test along +x.
        if (current.1 > p.1) != (previous.1 > p.1)
            && p.0
                < (previous.0 - current.0) * (p.1 - current.1) / (previous.1 - current.1)
                    + current.0
        {
            inside = !inside;
        }
        previous = current;
    }
    if inside { -distance } else { distance }
}

/// One cursor image: the union of its shapes, and where it points.
struct Art {
    shapes: Vec<Prim>,
    hotspot: P,
}

impl Art {
    fn sd(&self, p: P) -> f32 {
        self.shapes
            .iter()
            .map(|shape| shape.sd(p))
            .fold(f32::MAX, f32::min)
    }

    /// Straight-alpha BGRA pixels, top row first, `size` x `size`.
    fn render(&self, size: u32, look: Look) -> Vec<u8> {
        let (fill, edge) = look.colors();
        let k = size as f32 / 32.0;
        let coverage = |distance_px: f32| (0.5 - distance_px).clamp(0.0, 1.0);
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let p = ((x as f32 + 0.5) / k, (y as f32 + 0.5) / k);
                let d = self.sd(p) * k;
                let fill_a = coverage(d);
                let edge_a = coverage(d - EDGE * k);
                // The edge layer is the grown shape; the fill lies over it.
                let alpha = fill_a + edge_a * (1.0 - fill_a);
                let mut pixel = [0u8; 4];
                if alpha > 0.0 {
                    for c in 0..3 {
                        let premultiplied =
                            f32::from(fill[c]) * fill_a + f32::from(edge[c]) * edge_a * (1.0 - fill_a);
                        pixel[2 - c] = (premultiplied / alpha).round().clamp(0.0, 255.0) as u8;
                    }
                    pixel[3] = (alpha * 255.0).round() as u8;
                }
                out.extend_from_slice(&pixel);
            }
        }
        out
    }
}

/// The file for one cursor role: a `.cur`, or an `.ani` for animated roles.
pub fn render(look: Look, role: &Role) -> Vec<u8> {
    if role.animated {
        let frames: Vec<Vec<u8>> = (0..FRAMES)
            .map(|phase| cur_file(&art(role.value, phase), look))
            .collect();
        ani_file(&frames)
    } else {
        cur_file(&art(role.value, 0), look)
    }
}

// --- shapes -------------------------------------------------------------

fn rotate(p: P, degrees: f32) -> P {
    let (s, c) = degrees.to_radians().sin_cos();
    let (x, y) = (p.0 - 16.0, p.1 - 16.0);
    (16.0 + x * c - y * s, 16.0 + x * s + y * c)
}

fn rotated(shapes: &[Prim], degrees: f32) -> Vec<Prim> {
    shapes.iter().map(|s| s.map(|p| rotate(p, degrees))).collect()
}

/// The pointer, `scale` times its size around the tip at (3, 2).
fn arrow(scale: f32) -> Prim {
    let points = [
        (3.0, 2.0),
        (3.0, 24.5),
        (8.6, 19.2),
        (12.6, 28.0),
        (16.4, 26.3),
        (12.5, 17.7),
        (20.0, 17.7),
    ];
    Prim::Poly(
        points
            .iter()
            .map(|&(x, y)| (3.0 + (x - 3.0) * scale, 2.0 + (y - 2.0) * scale))
            .collect(),
    )
}

const ARROW_TIP: P = (3.0, 2.0);
/// The pointer is smaller when it carries a badge.
const BADGED: f32 = 0.8;

fn question_mark() -> Vec<Prim> {
    let path = [
        (19.5, 19.5),
        (19.5, 18.0),
        (21.0, 16.2),
        (23.5, 16.2),
        (26.0, 17.6),
        (26.0, 20.0),
        (24.4, 21.8),
        (23.5, 23.0),
        (23.5, 24.5),
    ];
    let mut shapes: Vec<Prim> = path
        .windows(2)
        .map(|pair| Prim::Capsule(pair[0], pair[1], 1.15))
        .collect();
    shapes.push(Prim::Disc((23.5, 28.0), 1.4));
    shapes
}

fn unavailable_badge() -> Vec<Prim> {
    vec![
        Prim::Ring((23.5, 23.5), 6.3, 0.9),
        Prim::Capsule((19.2, 28.0), (27.8, 19.0), 0.9),
    ]
}

fn person_badge() -> Vec<Prim> {
    vec![
        Prim::Disc((23.5, 19.0), 2.7),
        Prim::Poly(vec![
            (17.5, 30.0),
            (18.4, 25.6),
            (21.0, 23.4),
            (26.0, 23.4),
            (28.6, 25.6),
            (29.5, 30.0),
        ]),
    ]
}

fn pin_badge() -> Vec<Prim> {
    vec![
        Prim::Disc((23.5, 20.5), 4.3),
        Prim::Poly(vec![(19.6, 22.6), (23.5, 30.6), (27.4, 22.6)]),
    ]
}

/// `count` dots on a circle; the one at `head` is biggest and they shrink behind it.
fn spinner(center: P, radius: f32, count: usize, head: usize, big: f32, small: f32) -> Vec<Prim> {
    (0..count)
        .map(|i| {
            let angle = (i as f32 / count as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let age = (head + count - i) % count;
            let r = big - (big - small) * age as f32 / (count - 1) as f32;
            Prim::Disc(
                (center.0 + radius * angle.cos(), center.1 + radius * angle.sin()),
                r,
            )
        })
        .collect()
}

fn arrow_up() -> Vec<Prim> {
    vec![Prim::Poly(vec![
        (16.0, 2.0),
        (23.0, 11.0),
        (18.2, 11.0),
        (18.2, 29.0),
        (13.8, 29.0),
        (13.8, 11.0),
        (9.0, 11.0),
    ])]
}

/// Double-headed vertical arrow centered on (16, 16).
fn resize_vertical() -> Vec<Prim> {
    vec![
        Prim::Poly(vec![(16.0, 2.5), (22.0, 9.5), (10.0, 9.5)]),
        Prim::Capsule((16.0, 8.0), (16.0, 24.0), 1.2),
        Prim::Poly(vec![(16.0, 29.5), (22.0, 22.5), (10.0, 22.5)]),
    ]
}

fn move_all() -> Vec<Prim> {
    let head = Prim::Poly(vec![(16.0, 1.5), (21.5, 7.5), (10.5, 7.5)]);
    let mut shapes = vec![
        Prim::Capsule((16.0, 7.0), (16.0, 25.0), 1.1),
        Prim::Capsule((7.0, 16.0), (25.0, 16.0), 1.1),
    ];
    for turn in [0.0, 90.0, 180.0, 270.0] {
        shapes.push(head.map(|p| rotate(p, turn)));
    }
    shapes
}

fn hand() -> Vec<Prim> {
    vec![
        Prim::Capsule((11.0, 5.0), (11.0, 15.0), 2.0),
        Prim::Capsule((15.2, 12.5), (15.2, 16.0), 2.0),
        Prim::Capsule((19.3, 13.0), (19.3, 16.0), 1.9),
        Prim::Capsule((23.4, 14.0), (23.4, 17.0), 1.8),
        Prim::Poly(vec![
            (9.0, 14.0),
            (25.2, 14.0),
            (25.6, 21.0),
            (22.5, 28.5),
            (12.5, 28.5),
            (9.0, 22.5),
        ]),
        Prim::Capsule((9.4, 20.0), (5.6, 16.2), 1.9),
    ]
}

fn pencil() -> Vec<Prim> {
    vec![Prim::Poly(vec![
        (3.0, 29.0),
        (5.6, 20.4),
        (21.4, 4.6),
        (27.4, 10.6),
        (11.6, 26.4),
    ])]
}

fn text_beam() -> Vec<Prim> {
    vec![
        Prim::Capsule((16.0, 6.5), (16.0, 25.5), 0.9),
        Prim::Capsule((12.5, 5.6), (19.5, 5.6), 0.9),
        Prim::Capsule((12.5, 26.4), (19.5, 26.4), 0.9),
    ]
}

fn crosshair() -> Vec<Prim> {
    vec![
        Prim::Capsule((16.0, 3.0), (16.0, 13.0), 0.9),
        Prim::Capsule((16.0, 19.0), (16.0, 29.0), 0.9),
        Prim::Capsule((3.0, 16.0), (13.0, 16.0), 0.9),
        Prim::Capsule((19.0, 16.0), (29.0, 16.0), 0.9),
    ]
}

/// The image for `role` (by its registry value name) at animation `phase`.
fn art(role: &str, phase: usize) -> Art {
    let pointer = |scale: f32, badge: Vec<Prim>| {
        let mut shapes = vec![arrow(scale)];
        shapes.extend(badge);
        Art {
            shapes,
            hotspot: ARROW_TIP,
        }
    };
    let centered = |shapes: Vec<Prim>| Art {
        shapes,
        hotspot: (16.0, 16.0),
    };
    match role {
        "Help" => pointer(BADGED, question_mark()),
        "No" => pointer(BADGED, unavailable_badge()),
        "Person" => pointer(BADGED, person_badge()),
        "Pin" => pointer(BADGED, pin_badge()),
        "AppStarting" => pointer(
            BADGED,
            spinner((23.5, 23.5), 5.6, 8, phase * 8 / FRAMES, 1.5, 0.6),
        ),
        "Wait" => centered(spinner((16.0, 16.0), 9.5, FRAMES, phase, 2.3, 0.9)),
        "Crosshair" => centered(crosshair()),
        "IBeam" => centered(text_beam()),
        "SizeNS" => centered(resize_vertical()),
        "SizeWE" => centered(rotated(&resize_vertical(), 90.0)),
        "SizeNESW" => centered(rotated(&resize_vertical(), 45.0)),
        "SizeNWSE" => centered(rotated(&resize_vertical(), -45.0)),
        "SizeAll" => centered(move_all()),
        "UpArrow" => Art {
            shapes: arrow_up(),
            hotspot: (16.0, 2.0),
        },
        "Hand" => Art {
            shapes: hand(),
            hotspot: (11.0, 3.0),
        },
        "NWPen" => Art {
            shapes: pencil(),
            hotspot: (3.0, 29.0),
        },
        // "Arrow", and any role without art of its own.
        _ => pointer(1.0, Vec::new()),
    }
}

// --- file formats -------------------------------------------------------

/// A `.cur` with every size in [`SIZES`].
fn cur_file(art: &Art, look: Look) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&size| (size, dib(&art.render(size, look), size)))
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 2, 0]); // reserved, type 2 = cursor
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, image) in &images {
        let k = *size as f32 / 32.0;
        let hot = |v: f32| (v * k).round() as u16;
        out.push(*size as u8); // width (0 would mean 256)
        out.push(*size as u8);
        out.extend_from_slice(&[0, 0]); // palette size, reserved
        out.extend_from_slice(&hot(art.hotspot.0).to_le_bytes());
        out.extend_from_slice(&hot(art.hotspot.1).to_le_bytes());
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for (_, image) in images {
        out.extend_from_slice(&image);
    }
    out
}

/// A cursor image as a BMP without file header: the 32-bit color bitmap
/// (bottom row first) followed by an all-zero 1-bit AND mask (alpha does the cutting).
fn dib(top_down_bgra: &[u8], size: u32) -> Vec<u8> {
    let row = (size * 4) as usize;
    let mask_row = size.div_ceil(32) as usize * 4;
    let mask_len = mask_row * size as usize;
    let mut out = Vec::with_capacity(40 + top_down_bgra.len() + mask_len);
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&(size as i32 * 2).to_le_bytes()); // XOR bitmap + AND mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((top_down_bgra.len() + mask_len) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution, palette counts
    for line in top_down_bgra.chunks_exact(row).rev() {
        out.extend_from_slice(line);
    }
    out.resize(out.len() + mask_len, 0);
    out
}

/// An animated cursor: a RIFF `ACON` file holding each frame as a `.cur`.
fn ani_file(frames: &[Vec<u8>]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }
    let mut header = Vec::new();
    for field in [
        36,                  // size of this structure
        frames.len() as u32, // frames
        frames.len() as u32, // steps
        0,                   // width, height, bit count, planes: from the frames
        0,
        0,
        0,
        JIFFIES_PER_FRAME,
        1, // AF_ICON: the frames are icon/cursor files
    ] {
        header.extend_from_slice(&u32::to_le_bytes(field));
    }
    let mut list = b"fram".to_vec();
    for frame in frames {
        chunk(&mut list, b"icon", frame);
    }
    let mut body = b"ACON".to_vec();
    chunk(&mut body, b"anih", &header);
    chunk(&mut body, b"LIST", &list);
    let mut out = Vec::new();
    chunk(&mut out, b"RIFF", &body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursors::ROLES;

    fn role(value: &str) -> &'static Role {
        ROLES.iter().find(|r| r.value == value).unwrap()
    }

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    /// (size, hotspot, bgra rows top-down) of every image in a `.cur`.
    fn read_cur(file: &[u8]) -> Vec<(u32, (u16, u16), Vec<u8>)> {
        assert_eq!(&file[..4], [0, 0, 2, 0], "a cursor, not an icon");
        (0..usize::from(u16_at(file, 4)))
            .map(|i| {
                let entry = &file[6 + i * 16..6 + (i + 1) * 16];
                let size = u32::from(entry[0]);
                let hotspot = (u16_at(entry, 4), u16_at(entry, 6));
                let (len, offset) = (u32_at(entry, 8) as usize, u32_at(entry, 12) as usize);
                let dib = &file[offset..offset + len];
                assert_eq!(u32_at(dib, 0), 40);
                assert_eq!(u32_at(dib, 4), size);
                assert_eq!(u32_at(dib, 8), size * 2, "XOR bitmap and AND mask");
                assert_eq!(u16_at(dib, 14), 32);
                let row = (size * 4) as usize;
                let mut top_down: Vec<u8> = Vec::new();
                for line in dib[40..40 + row * size as usize].chunks_exact(row).rev() {
                    top_down.extend_from_slice(line);
                }
                (size, hotspot, top_down)
            })
            .collect()
    }

    fn pixel(image: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        image[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn a_cursor_has_every_size_with_a_scaled_hotspot_and_transparent_corners() {
        let file = render(Look::Dark, role("Arrow"));
        let images = read_cur(&file);
        assert_eq!(images.iter().map(|i| i.0).collect::<Vec<_>>(), SIZES);
        assert_eq!(images[0].1, (3, 2));
        assert_eq!(images[2].1, (6, 4), "the 64 px image scales the hotspot");
        for (size, _, image) in &images {
            assert_eq!(pixel(image, *size, size - 1, 0)[3], 0, "top right is empty");
            // The tip is inside the shape: opaque somewhere around the hotspot.
            assert!(pixel(image, *size, size / 8, size / 8)[3] > 0);
        }
    }

    #[test]
    fn dark_has_dark_fill_with_a_light_edge_and_light_is_the_inverse() {
        let inside = |look| {
            let images = read_cur(&render(look, role("Arrow")));
            pixel(&images[0].2, 32, 6, 12) // well inside the pointer
        };
        let (dark, light) = (inside(Look::Dark), inside(Look::Light));
        assert_eq!(dark, [0x1b, 0x16, 0x14, 255]); // BGRA
        assert_eq!(light, [255, 255, 255, 255]);

        // Just outside the shape, the edge color shows (partly transparent at the very rim).
        let images = read_cur(&render(Look::Dark, role("Arrow")));
        let edge = pixel(&images[0].2, 32, 2, 12);
        assert_eq!(edge, [255, 255, 255, 255], "white edge left of the pointer");
    }

    #[test]
    fn every_role_renders_something_visible_with_its_hotspot_on_the_canvas() {
        for role in &ROLES {
            let file = render(Look::Light, role);
            let first_cur = if role.animated {
                // RIFF....ACON anih(36) LIST....fram icon<len> <.cur>
                let at = file.windows(4).position(|w| w == b"icon").unwrap() + 8;
                file[at..].to_vec()
            } else {
                file
            };
            for (size, (hx, hy), image) in read_cur(&first_cur) {
                assert!(u32::from(hx) < size && u32::from(hy) < size, "{}", role.value);
                let visible = image.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count();
                assert!(visible > 40, "{} draws almost nothing at {size}px", role.value);
            }
        }
    }

    #[test]
    fn animated_cursors_are_riff_acon_files_with_distinct_frames() {
        for value in ["Wait", "AppStarting"] {
            let file = render(Look::Dark, role(value));
            assert_eq!(&file[..4], b"RIFF");
            assert_eq!(u32_at(&file, 4) as usize, file.len() - 8);
            assert_eq!(&file[8..12], b"ACON");
            assert_eq!(&file[12..16], b"anih");
            assert_eq!(u32_at(&file, 16), 36);
            assert_eq!(u32_at(&file, 20), 36);
            assert_eq!(u32_at(&file, 24) as usize, FRAMES);
            assert_eq!(u32_at(&file, 28) as usize, FRAMES, "steps");
            assert_eq!(u32_at(&file, 48), JIFFIES_PER_FRAME);
            assert_eq!(u32_at(&file, 52), 1, "AF_ICON");
            assert_eq!(&file[56..60], b"LIST");
            assert_eq!(&file[64..68], b"fram");
            let icons = file.windows(4).filter(|w| *w == b"icon").count();
            assert!(icons >= FRAMES);
        }
        // The spinner turns: consecutive phases differ.
        let a = art("Wait", 0).render(32, Look::Dark);
        let b = art("Wait", 1).render(32, Look::Dark);
        assert_ne!(a, b);
    }

    #[test]
    fn set_names_round_trip_case_insensitively() {
        assert_eq!(Look::from_name("MyPaper-Dark"), Some(Look::Dark));
        assert_eq!(Look::from_name(Look::Light.name()), Some(Look::Light));
        assert_eq!(Look::from_name("mypaper"), None);
    }

    #[test]
    fn polygon_distance_is_negative_inside_and_exact_on_edges() {
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert_eq!(polygon_sd(&square, (5.0, 5.0)), -5.0);
        assert_eq!(polygon_sd(&square, (13.0, 5.0)), 3.0);
        assert!((polygon_sd(&square, (13.0, 14.0)) - 5.0).abs() < 1e-5);
    }
}
