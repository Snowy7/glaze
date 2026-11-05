//! The `canvas` view kind (docs/plugins.md, "Views"): a plugin's drawing commands rasterized by
//! the kit, clipped to the canvas. Damage is the kit's: the face's display list is diffed against
//! the last one, so only commands that changed repaint.
//!
//! Images arrive by id ([`put_image`]) from the plugin bridge; ids are unique across the plugin
//! runtime. An id with no image yet draws the theme's well in its rect.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mypaper_plugin_sdk::{Draw, Paint, Tone};
use mypaper_ui::kit::Frame;
use mypaper_ui::{Image, Op, Point, Rect, Rgba};

static IMAGES: Mutex<Option<HashMap<u32, Arc<Image>>>> = Mutex::new(None);

/// Makes image `id` drawable by canvases (BGRA8 premultiplied, from `ui.put-image`). The bridge
/// calls it when the runtime's shared-memory image frames land (docs/plugins.md, "Images").
#[allow(dead_code)]
pub fn put_image(id: u32, image: Image) {
    IMAGES.lock().expect("canvas images").get_or_insert_default().insert(id, Arc::new(image));
}

/// Forgets image `id` (`ui.drop-image`, or its plugin stopped).
#[allow(dead_code)]
pub fn drop_image(id: u32) {
    if let Some(images) = IMAGES.lock().expect("canvas images").as_mut() {
        images.remove(&id);
    }
}

fn image(id: u32) -> Option<Arc<Image>> {
    IMAGES.lock().expect("canvas images").as_ref()?.get(&id).cloned()
}

/// Most scanlines one path fill may take (a tall path stays a bounded number of ops).
const MAX_ROWS: usize = 1024;

/// Draws `commands` into `rect` (the canvas's slot), in kit points from its top left.
pub fn draw(f: &mut Frame<'_>, rect: Rect, commands: &[Draw], tone: impl Fn(&Frame<'_>, &Tone) -> Rgba) {
    let s = f.s;
    let at = |x: f32, y: f32| Point::new(rect.left + x * s, rect.top + y * s);
    f.push(Op::Clip { rect });
    for command in commands {
        match command {
            Draw::Rect { x, y, w, h, radius, paint } => {
                let r = Rect::new(rect.left + x * s, rect.top + y * s, w * s, h * s);
                match paint {
                    Paint::Fill(c) => f.fill(r, radius * s, tone(f, c)),
                    Paint::Stroke(c, width) => f.push(Op::Stroke { rect: r, radius: radius * s, width: width * s, color: tone(f, c) }),
                }
            }
            Draw::Circle { x, y, r, paint } => {
                let center = at(*x, *y);
                match paint {
                    Paint::Fill(c) => {
                        let d = r * s * 2.0;
                        f.fill(Rect::new(center.x - r * s, center.y - r * s, d, d), r * s, tone(f, c));
                    }
                    Paint::Stroke(c, width) => {
                        f.push(Op::Arc { center, radius: r * s, sweep: std::f32::consts::TAU, width: width * s, color: tone(f, c) })
                    }
                }
            }
            Draw::Line { x1, y1, x2, y2, color, width } => {
                f.push(Op::Line { from: at(*x1, *y1), to: at(*x2, *y2), width: (width * s).max(1.0), color: tone(f, color) });
            }
            Draw::Path { d, paint } => {
                let outlines: Vec<Vec<Point>> = flatten(d).into_iter().map(|o| o.into_iter().map(|p| at(p.x, p.y)).collect()).collect();
                let (color, width) = match paint {
                    Paint::Fill(c) => {
                        let color = tone(f, c);
                        fill(f, &outlines, color);
                        // A hairline over the scanlines antialiases the edges.
                        (color, 1.0)
                    }
                    Paint::Stroke(c, width) => (tone(f, c), (width * s).max(1.0)),
                };
                for outline in &outlines {
                    for pair in outline.windows(2) {
                        f.push(Op::Line { from: pair[0], to: pair[1], width, color });
                    }
                }
            }
            Draw::Text { x, y, text, role, color } => {
                let style = f.font(super::role(*role));
                let ink = tone(f, color);
                let (w, h) = f.text.measure(text, &style);
                let p = at(*x, *y);
                if !text.is_empty() {
                    f.push(Op::Text { rect: Rect::new(p.x.round(), p.y.round(), w, h), text: text.as_str().into(), style, color: ink });
                }
            }
            Draw::Image { x, y, w, h, id } => {
                let r = Rect::new(rect.left + x * s, rect.top + y * s, w * s, h * s);
                match image(*id) {
                    Some(image) => f.push(Op::Image { rect: r, image, opacity: 1.0, source: None }),
                    None => f.fill(r, 4.0 * s, f.theme.color.well),
                }
            }
        }
    }
    f.push(Op::Unclip);
}

/// Even-odd scanline fill of closed `outlines`, one op per span per device-pixel row.
fn fill(f: &mut Frame<'_>, outlines: &[Vec<Point>], color: Rgba) {
    let (top, bottom) = outlines.iter().flatten().fold((f32::MAX, f32::MIN), |(t, b), p| (t.min(p.y), b.max(p.y)));
    if top >= bottom {
        return;
    }
    let rows = ((bottom - top).ceil() as usize).min(MAX_ROWS);
    let mut xs = Vec::new();
    for row in 0..rows {
        let y = top.floor() + row as f32 + 0.5;
        xs.clear();
        for outline in outlines {
            let closed = outline.iter().zip(outline.iter().cycle().skip(1)).take(outline.len());
            for (a, b) in closed {
                if (a.y <= y) != (b.y <= y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        for span in xs.chunks_exact(2) {
            f.fill(Rect::new(span[0], y - 0.5, span[1] - span[0], 1.0), 0.0, color);
        }
    }
}

/// SVG path data (`M L H V C Q Z`, upper case absolute, lower case relative) as polylines in
/// the path's own units. Curves are flattened to 12 segments; unknown commands end the path.
pub fn flatten(d: &str) -> Vec<Vec<Point>> {
    let mut tokens = Tokens::new(d);
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let (mut p, mut start) = (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
    let mut command = None;
    while let Some(c) = tokens.command().or(command) {
        command = Some(match c {
            'M' => 'L',
            'm' => 'l',
            other => other,
        });
        let relative = c.is_ascii_lowercase();
        let base = |p: Point| if relative { p } else { Point::new(0.0, 0.0) };
        let point = |tokens: &mut Tokens, p: Point| Some(Point::new(base(p).x + tokens.number()?, base(p).y + tokens.number()?));
        match c.to_ascii_uppercase() {
            'M' => {
                let Some(next) = point(&mut tokens, p) else { break };
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
                p = next;
                start = next;
                current.push(p);
            }
            'L' => {
                let Some(next) = point(&mut tokens, p) else { break };
                p = next;
                current.push(p);
            }
            'H' => {
                let Some(x) = tokens.number() else { break };
                p = Point::new(if relative { p.x + x } else { x }, p.y);
                current.push(p);
            }
            'V' => {
                let Some(y) = tokens.number() else { break };
                p = Point::new(p.x, if relative { p.y + y } else { y });
                current.push(p);
            }
            'C' => {
                let (Some(c1), Some(c2), Some(end)) = (point(&mut tokens, p), point(&mut tokens, p), point(&mut tokens, p)) else { break };
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    current.push(Point::new(
                        w[0] * p.x + w[1] * c1.x + w[2] * c2.x + w[3] * end.x,
                        w[0] * p.y + w[1] * c1.y + w[2] * c2.y + w[3] * end.y,
                    ));
                }
                p = end;
            }
            'Q' => {
                let (Some(c1), Some(end)) = (point(&mut tokens, p), point(&mut tokens, p)) else { break };
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    current.push(Point::new(u * u * p.x + 2.0 * u * t * c1.x + t * t * end.x, u * u * p.y + 2.0 * u * t * c1.y + t * t * end.y));
                }
                p = end;
            }
            'Z' => {
                current.push(start);
                p = start;
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.push(p);
                command = None;
            }
            _ => break,
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// A cursor over path data: command letters and numbers, separated by spaces or commas.
struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    fn new(d: &'a str) -> Self {
        Self { rest: d }
    }
    fn skip(&mut self) {
        self.rest = self.rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    }
    fn command(&mut self) -> Option<char> {
        self.skip();
        let c = self.rest.chars().next().filter(|c| c.is_ascii_alphabetic())?;
        self.rest = &self.rest[1..];
        Some(c)
    }
    fn number(&mut self) -> Option<f32> {
        self.skip();
        let bytes = self.rest.as_bytes();
        let mut end = 0;
        let mut dot = false;
        while end < bytes.len() {
            match bytes[end] {
                b'-' | b'+' if end == 0 => {}
                b'0'..=b'9' => {}
                b'.' if !dot => dot = true,
                _ => break,
            }
            end += 1;
        }
        let value = self.rest[..end].parse().ok()?;
        self.rest = &self.rest[end..];
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_flatten_with_relative_commands_and_closing() {
        let shapes = flatten("M0 0 L10,0 v10 h-10 Z");
        assert_eq!(shapes.len(), 1);
        let pts: Vec<(f32, f32)> = shapes[0].iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(pts, [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]);
        // A curve ends where it says, and implicit repeats of `l` keep going.
        let curve = flatten("m5 5 c0 10 10 10 10 0 l5 0 5 0");
        let last = curve[0].last().unwrap();
        assert_eq!((last.x, last.y), (25.0, 5.0));
        assert!(curve[0].len() > 12);
    }
}
