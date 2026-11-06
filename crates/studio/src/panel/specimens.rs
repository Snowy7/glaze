//! Small software material specimens over the current wallpaper. No GPU or engine needed.
use super::{controls::text, tokens::hex};
use crate::{app::App, docs::DocId, ui::B};
use mypaper_config::look::Style;
use mypaper_ui::controls::Align;
use mypaper_ui::{Image, Rect, Rgba};
use std::sync::Arc;

pub fn wallpaper(app: &App) -> Option<Arc<Image>> {
    let item = app
        .library
        .active()
        .and_then(|i| app.library.items.get(i))?;
    app.images
        .borrow_mut()
        .get(item.preview.as_ref().or(item.video.as_ref())?, (384, 256))
}

pub fn wallpaper_color(app: &App) -> Option<Rgba> {
    let image = wallpaper(app)?;
    let mut sums = [0u64; 3];
    let mut count = 0;
    for pixel in image.pixels.chunks_exact(4).step_by(7) {
        if pixel[3] < 128 {
            continue;
        }
        for i in 0..3 {
            sums[i] += pixel[i] as u64;
        }
        count += 1;
    }
    (count > 0).then(|| {
        Rgba::rgb8(
            [
                (sums[2] / count) as u8,
                (sums[1] / count) as u8,
                (sums[0] / count) as u8,
            ],
            1.0,
        )
    })
}

pub fn backdrop(app: &App, b: &mut B, r: Rect) {
    b.fill(r, b.px(8.0), b.p.fill);
    if let Some(image) = wallpaper(app) {
        crate::stage::image_round(b, r, &image, 1.0, b.px(8.0));
    }
}

/// Two-pass box blur on a cropped, small bitmap. Its cache key includes the source and blur.
fn softened(app: &App, image: &Arc<Image>, radius: usize) -> Arc<Image> {
    let cache_key = format!("{:p}:{radius}", Arc::as_ptr(image));
    let state = app.panel.as_ref().unwrap();
    if let Some(image) = state.specimens.borrow().get(&cache_key) {
        return image.clone();
    }
    let (width, height) = (128usize, 80usize);
    let mut pixels = vec![0; width * height * 4];
    let aspect = width as f32 / height as f32;
    let sw = (image.height as f32 * aspect).min(image.width as f32);
    let sh = sw / aspect;
    for y in 0..height {
        for x in 0..width {
            let sx = ((image.width as f32 - sw) / 2.0 + x as f32 * sw / width as f32) as usize;
            let sy = ((image.height as f32 - sh) / 2.0 + y as f32 * sh / height as f32) as usize;
            let source = (sy.min(image.height as usize - 1) * image.width as usize
                + sx.min(image.width as usize - 1))
                * 4;
            pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
                .copy_from_slice(&image.pixels[source..source + 4]);
        }
    }
    for horizontal in [true, false] {
        let source = pixels.clone();
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0usize; 4];
                let mut count = 0;
                for offset in -(radius as isize)..=radius as isize {
                    let sx = if horizontal {
                        (x as isize + offset).clamp(0, width as isize - 1) as usize
                    } else {
                        x
                    };
                    let sy = if horizontal {
                        y
                    } else {
                        (y as isize + offset).clamp(0, height as isize - 1) as usize
                    };
                    for channel in 0..4 {
                        sum[channel] += source[(sy * width + sx) * 4 + channel] as usize;
                    }
                    count += 1;
                }
                for channel in 0..4 {
                    pixels[(y * width + x) * 4 + channel] = (sum[channel] / count) as u8;
                }
            }
        }
    }
    let image = Arc::new(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    });
    let mut cache = state.specimens.borrow_mut();
    if cache.len() >= 80 {
        cache.clear();
    }
    cache.insert(cache_key, image.clone());
    image
}

pub fn material(app: &App, b: &mut B, r: Rect, style: Style) {
    backdrop(app, b, r);
    let surface = Rect::new(
        r.left + r.width() * 0.13,
        r.top + r.height() * 0.18,
        r.width() * 0.74,
        r.height() * 0.64,
    );
    let (blur, alpha, radius, highlight) = match style {
        Style::Liquid => (2, 0.16, 12.0, 0.8),
        Style::Oneui => (6, 0.76, 18.0, 0.15),
        Style::Solid => (0, 1.0, 7.0, 0.12),
        Style::Fluent => (11, 0.67, 6.0, 0.35),
        Style::Mono => (0, 0.82, 2.0, 0.0),
    };
    let blur = app
        .get(DocId::Config, "/look/material/blur")
        .and_then(serde_json::Value::as_f64)
        .map_or(blur, |v| (v / 2.0) as usize)
        .min(16);
    let alpha = if style == Style::Solid {
        1.0
    } else {
        app.get(DocId::Config, "/look/material/clarity")
            .and_then(serde_json::Value::as_f64)
            .map_or(alpha, |c| (1.0 - c) as f32)
    };
    let highlight = if style == Style::Liquid {
        app.get(DocId::Config, "/look/material/highlight")
            .and_then(serde_json::Value::as_f64)
            .map_or(highlight, |v| v as f32)
    } else {
        highlight
    };
    let radius = app
        .get(DocId::Config, "/look/shape/corners")
        .and_then(serde_json::Value::as_f64)
        .map_or(radius, |v| v as f32);
    let radius = b.px(radius).min(surface.height() / 2.0);
    if style != Style::Solid
        && let Some(image) = wallpaper(app)
    {
        crate::stage::image_round(b, surface, &softened(app, &image, blur), 1.0, radius);
    }
    let paint = app
        .get(DocId::Config, "/look/material/tint")
        .and_then(serde_json::Value::as_str);
    let tint = paint
        .and_then(mypaper_config::Color::parse)
        .map(|c| Rgba::rgb8(c.0, 1.0))
        .or_else(|| {
            (paint == Some("auto"))
                .then(|| wallpaper_color(app))
                .flatten()
        })
        .unwrap_or_else(|| {
            if b.p.dark {
                hex(0x20242c)
            } else {
                hex(0xf3f5f8)
            }
        });
    b.fill(surface, radius, tint.with_alpha(alpha));
    if highlight > 0.0 {
        b.stroke(
            surface,
            radius,
            b.px(1.0),
            hex(0xffffff).with_alpha(highlight),
        );
        b.hline(
            surface.left + radius,
            surface.right - radius,
            surface.top + b.px(1.0),
            hex(0xffffff).with_alpha(highlight),
        );
    }
    let ink = if b.p.dark {
        hex(0xffffff)
    } else {
        hex(0x1a1a1a)
    };
    text(b, surface, "10:42", 22.0, 500, ink, Align::Center);
}

pub fn window(app: &App, b: &mut B, r: Rect, look: usize) {
    backdrop(app, b, r);
    let win = Rect::new(
        r.left + r.width() * 0.13,
        r.top + r.height() * 0.17,
        r.width() * 0.74,
        r.height() * 0.7,
    );
    let radius = b.px(if look == 3 { 0.0 } else { 5.0 });
    if let Some(image) = wallpaper(app) {
        crate::stage::image_round(
            b,
            win,
            &softened(app, &image, if look == 2 { 9 } else { 5 }),
            1.0,
            radius,
        );
    }
    let light = look == 0 || !b.p.dark;
    b.fill(
        win,
        radius,
        hex(if light { 0xf5f5f5 } else { 0x24262b }).with_alpha(if look == 2 {
            0.52
        } else {
            0.94
        }),
    );
    b.stroke(
        win,
        radius,
        b.px(1.0),
        if look == 0 {
            b.p.edge
        } else {
            wallpaper_color(app).unwrap_or(b.p.ink2)
        },
    );
    let ink = hex(if light { 0x444444 } else { 0xeeeeee });
    for n in 0..3 {
        b.fill(
            Rect::new(
                win.right - b.px(10.0 + n as f32 * 8.0),
                win.top + b.px(7.0),
                b.px(3.0),
                b.px(2.0),
            ),
            0.0,
            ink,
        );
    }
    b.fill(
        Rect::new(
            win.left + b.px(7.0),
            win.top + b.px(19.0),
            win.width() * 0.22,
            win.height() - b.px(25.0),
        ),
        b.px(2.0),
        ink.with_alpha(0.1),
    );
    for n in 0..3 {
        b.fill(
            Rect::new(
                win.left + win.width() * 0.4,
                win.top + b.px(22.0 + n as f32 * 9.0),
                win.width() * (0.4 - n as f32 * 0.06),
                b.px(3.0),
            ),
            b.px(1.0),
            ink.with_alpha(0.4),
        );
    }
}
