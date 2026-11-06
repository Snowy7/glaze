//! A compact native color popover. The owner routes pointer/key input and provides the hex
//! text field; this component owns the color model, geometry, painting and accessible targets.
use std::sync::{Arc, OnceLock};

use crate::controls::{Align, Behavior, Builder, Font, Key, key};
use crate::{Image, Op, Point, Rect, Rgba, Role, Semantics};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    SaturationValue,
    Hue,
}

impl Hsv {
    pub fn from_rgb(rgb: [u8; 3]) -> Self {
        let [r, g, b] = rgb.map(|channel| channel as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let hue = if delta <= f32::EPSILON { 0.0 }
            else if max == r { ((g - b) / delta).rem_euclid(6.0) * 60.0 }
            else if max == g { ((b - r) / delta + 2.0) * 60.0 }
            else { ((r - g) / delta + 4.0) * 60.0 };
        Self { hue, saturation: if max == 0.0 { 0.0 } else { delta / max }, value: max }
    }

    pub fn rgb(self) -> [u8; 3] {
        let hue = self.hue.rem_euclid(360.0) / 60.0;
        let chroma = self.value * self.saturation;
        let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
        let rgb = match hue as u8 {
            0 => [chroma, x, 0.0], 1 => [x, chroma, 0.0], 2 => [0.0, chroma, x],
            3 => [0.0, x, chroma], 4 => [x, 0.0, chroma], _ => [chroma, 0.0, x],
        };
        let m = self.value - chroma;
        rgb.map(|channel| ((channel + m) * 255.0).round().clamp(0.0, 255.0) as u8)
    }

    pub fn at(self, part: Part, rect: Rect, point: Point) -> Self {
        let x = ((point.x - rect.left) / rect.width().max(1.0)).clamp(0.0, 1.0);
        match part {
            Part::Hue => Self { hue: x * 359.999, ..self },
            Part::SaturationValue => Self { saturation: x, value: 1.0 - ((point.y - rect.top) / rect.height().max(1.0)).clamp(0.0, 1.0), ..self },
        }
    }
}

fn gradient(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Arc<Image> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = color(x, y);
            pixels.extend_from_slice(&[b, g, r, 255]);
        }
    }
    Arc::new(Image { width, height, pixels })
}

/// Draws a square, hue strip and hex-field well. Returns the well for the owner's native field.
pub fn draw<A: Clone>(
    b: &mut Builder<'_, A>, id: Key, rect: Rect, color: Hsv, title: &str,
    mut action: impl FnMut(Part, Rect) -> A,
) -> Rect {
    let pad = b.px(12.0);
    b.list.ops.push(Op::Shadow { rect, radius: b.px(10.0), spread: b.px(9.0), drop: b.px(3.0), color: b.p.shadow });
    b.fill(rect, b.px(10.0), b.p.panel);
    b.stroke(rect, b.px(10.0), b.px(1.0), b.p.control);
    let font = b.font(Font::Caption);
    b.text_in(Rect::new(rect.left + pad, rect.top + pad, rect.width() - 2.0 * pad, b.px(20.0)), title, &font, b.p.ink1, Align::Start);
    let size = rect.width() - pad * 2.0;
    let square = Rect::new(rect.left + pad, rect.top + b.px(38.0), size, size);
    let image = gradient(96, 96, |x, y| Hsv { hue: color.hue, saturation: x as f32 / 95.0, value: 1.0 - y as f32 / 95.0 }.rgb());
    b.list.ops.push(Op::RoundedImage { rect: square, image, opacity: 1.0, source: None, radius: b.px(4.0) });
    let point = Point::new(square.left + size * color.saturation, square.top + size * (1.0 - color.value));
    let marker = Rect::new(point.x-b.px(4.0), point.y-b.px(4.0), b.px(8.0), b.px(8.0));
    b.stroke(marker.inflate(b.px(1.0)), b.px(5.0), b.px(2.0), Rgba::new(0.0,0.0,0.0,0.8));
    b.stroke(marker, b.px(4.0), b.px(1.5), Rgba::new(1.0,1.0,1.0,1.0));
    let square_key = key((id, "sv"));
    b.hit(square_key, square, Role::ColorWell, &format!("Saturation {:.0}%, brightness {:.0}%. Arrow keys adjust color", color.saturation*100.0, color.value*100.0), action(Part::SaturationValue, square), Behavior::Drag);
    b.focus_ring(square_key, square, b.px(4.0));
    let hue = Rect::new(square.left, square.bottom + b.px(12.0), size, b.px(16.0));
    static HUE: OnceLock<Arc<Image>> = OnceLock::new();
    let image = HUE.get_or_init(|| gradient(360, 1, |x, _| Hsv { hue: x as f32, saturation: 1.0, value: 1.0 }.rgb())).clone();
    b.list.ops.push(Op::RoundedImage { rect: hue, image, opacity: 1.0, source: None, radius: b.px(4.0) });
    let x = hue.left + hue.width() * color.hue / 360.0;
    let thumb = Rect::new(x-b.px(3.0), hue.top-b.px(2.0), b.px(6.0), hue.height()+b.px(4.0));
    b.fill(thumb, b.px(3.0), Rgba::new(1.0,1.0,1.0,1.0));
    b.stroke(thumb, b.px(3.0), b.px(1.0), Rgba::new(0.0,0.0,0.0,0.7));
    let hue_key = key((id, "hue"));
    b.hit(hue_key, hue.inflate(b.px(4.0)), Role::Slider, &format!("Hue, {:.0} degrees", color.hue), action(Part::Hue, hue), Behavior::Drag);
    b.semantics(hue_key, Semantics::Range { value: color.hue as f64, min: 0.0, max: 359.0, step: 1.0 });
    b.focus_ring(hue_key, hue, b.px(4.0));
    Rect::new(square.left, hue.bottom + b.px(12.0), size, b.px(32.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_model_round_trips_primaries_greys_and_custom_colors() {
        for rgb in [[255,0,0], [0,255,0], [0,0,255], [0,0,0], [255,255,255], [128,128,128], [139,158,183], [27,211,107]] {
            assert_eq!(Hsv::from_rgb(rgb).rgb(), rgb);
        }
    }

    #[test]
    fn pointer_covers_the_square_and_hue_strip_and_clamps_at_edges() {
        let hsv = Hsv::from_rgb([255,0,0]);
        let r = Rect::new(10.0,20.0,100.0,100.0);
        assert_eq!(hsv.at(Part::SaturationValue, r, Point::new(-100.0,-100.0)).rgb(), [255;3]);
        assert_eq!(hsv.at(Part::SaturationValue, r, Point::new(999.0,999.0)).rgb(), [0;3]);
        assert_eq!(hsv.at(Part::Hue, r, Point::new(60.0,20.0)).rgb(), [0,255,255]);
    }
}
