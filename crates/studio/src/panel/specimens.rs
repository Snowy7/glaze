//! Small software material specimens over the current wallpaper. No GPU or engine needed.
use super::{controls::text, tokens::hex};
use crate::{app::App, docs::DocId, ui::B};
use mypaper_config::look::Style;
use mypaper_ui::controls::Align;
use mypaper_ui::{Image, Rect, Rgba};
use std::sync::Arc;

/// Keep only a page's small previews; large libraries must not grow the panel's footprint.
pub fn remember(app: &App, key: String, image: &Arc<Image>) {
    let mut cache = app.panel.as_ref().unwrap().specimens.borrow_mut();
    let bytes: usize = cache.values().map(|image| image.pixels.len()).sum();
    if cache.len() >= 80 || bytes + image.pixels.len() > 4 << 20 {
        cache.clear();
    }
    cache.insert(key, image.clone());
}

pub fn wallpaper(app: &App) -> Option<Arc<Image>> {
    let item = app
        .library
        .active()
        .and_then(|i| app.library.items.get(i))?;
    wallpaper_image(app, item)
}

pub fn wallpaper_image(app: &App, item: &crate::library::Wallpaper) -> Option<Arc<Image>> {
    app.images
        .borrow_mut()
        .get(item.preview.as_ref().or(item.video.as_ref())?, {
            let width = app.size.0.clamp(384, 1024);
            (width, width * 2 / 3)
        })
}

pub fn wallpaper_color(app: &App) -> Option<Rgba> {
    let image = wallpaper(app)?;
    average_color(&image)
}

pub fn wallpaper_color_of(app: &App, wallpaper: &crate::library::Wallpaper) -> Option<Rgba> {
    let image = wallpaper_image(app, wallpaper)?;
    average_color(&image)
}

fn average_color(image: &Image) -> Option<Rgba> {
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

/// Fractional pixel coverage preserves detail when a wallpaper crop is reduced.
fn area_crop(image: &Image, crop: Rect, size: (u32, u32)) -> Image {
    let (width, height) = (size.0.max(1) as usize, size.1.max(1) as usize);
    let mut pixels = vec![0; width * height * 4];
    for y in 0..height {
        let top = crop.top + y as f32 * crop.height() / height as f32;
        let bottom = crop.top + (y + 1) as f32 * crop.height() / height as f32;
        for x in 0..width {
            let left = crop.left + x as f32 * crop.width() / width as f32;
            let right = crop.left + (x + 1) as f32 * crop.width() / width as f32;
            if crop.width() < width as f32 && crop.height() < height as f32 {
                let sx = ((left + right) * 0.5 - 0.5).clamp(0.0, image.width as f32 - 1.0);
                let sy = ((top + bottom) * 0.5 - 0.5).clamp(0.0, image.height as f32 - 1.0);
                let (ix, iy) = (sx.floor() as usize, sy.floor() as usize);
                let (fx, fy) = (sx.fract(), sy.fract());
                let at = |x, y| (y * image.width as usize + x) * 4;
                for channel in 0..4 {
                    let a = image.pixels[at(ix, iy) + channel] as f32;
                    let b = image.pixels[at((ix + 1).min(image.width as usize - 1), iy) + channel]
                        as f32;
                    let c = image.pixels[at(ix, (iy + 1).min(image.height as usize - 1)) + channel]
                        as f32;
                    let d = image.pixels[at(
                        (ix + 1).min(image.width as usize - 1),
                        (iy + 1).min(image.height as usize - 1),
                    ) + channel] as f32;
                    pixels[(y * width + x) * 4 + channel] = ((a * (1.0 - fx) + b * fx) * (1.0 - fy)
                        + (c * (1.0 - fx) + d * fx) * fy)
                        .round() as u8;
                }
                continue;
            }
            let mut sum = [0.0; 4];
            let mut area = 0.0;
            for sy in
                top.floor().max(0.0) as usize..(bottom.ceil() as usize).min(image.height as usize)
            {
                let h = (bottom.min(sy as f32 + 1.0) - top.max(sy as f32)).max(0.0);
                for sx in left.floor().max(0.0) as usize
                    ..(right.ceil() as usize).min(image.width as usize)
                {
                    let weight = h * (right.min(sx as f32 + 1.0) - left.max(sx as f32)).max(0.0);
                    let at = (sy * image.width as usize + sx) * 4;
                    for channel in 0..4 {
                        sum[channel] += image.pixels[at + channel] as f32 * weight;
                    }
                    area += weight;
                }
            }
            for channel in 0..4 {
                pixels[(y * width + x) * 4 + channel] =
                    (sum[channel] / area.max(f32::EPSILON)).round() as u8;
            }
        }
    }
    Image {
        width: width as u32,
        height: height as u32,
        pixels,
    }
}

/// Two-pass box blur on a crop rendered at the specimen's actual pixel dimensions.
fn softened(app: &App, image: &Arc<Image>, radius: usize, size: (u32, u32)) -> Arc<Image> {
    let cache_key = format!(
        "soft:{}:{:p}:{radius}:{size:?}",
        app.library
            .active()
            .map_or("", |i| app.library.items[i].id.as_str()),
        Arc::as_ptr(image)
    );
    let state = app.panel.as_ref().unwrap();
    if let Some(image) = state.specimens.borrow().get(&cache_key) {
        return image.clone();
    }
    let aspect = size.0 as f32 / size.1.max(1) as f32;
    let sw = (image.height as f32 * aspect).min(image.width as f32);
    let sh = sw / aspect;
    let crop = area_crop(
        image,
        Rect::new(
            (image.width as f32 - sw) / 2.0,
            (image.height as f32 - sh) / 2.0,
            sw,
            sh,
        ),
        size,
    );
    let pixels = blur_crop(crop, radius);
    let image = Arc::new(Image {
        width: size.0,
        height: size.1,
        pixels,
    });
    remember(app, cache_key, &image);
    image
}

/// Native style defaults, shared by the pictures and the widget look projection.
pub fn style_theme(app: &App, style: Style) -> mypaper_ui::Theme {
    mypaper_ui::Theme::new(
        match style {
            Style::Liquid => mypaper_ui::Style::Liquid,
            Style::Oneui => mypaper_ui::Style::OneUi,
            Style::Fluent => mypaper_ui::Style::Fluent,
            Style::Solid => mypaper_ui::Style::Solid,
            Style::Mono => mypaper_ui::Style::Mono,
        },
        if desktop_dark(app) {
            mypaper_ui::Scheme::Dark
        } else {
            mypaper_ui::Scheme::Light
        },
        wallpaper_color(app).unwrap_or(hex(0x8b9eb7)),
    )
}

/// Pick a bright, detailed crop so glass can be judged even on a quiet wallpaper.
fn material_backdrop(app: &App, size: (u32, u32)) -> Arc<Image> {
    material_backdrop_from(app, size, wallpaper(app))
}

fn material_backdrop_from(app: &App, size: (u32, u32), source: Option<Arc<Image>>) -> Arc<Image> {
    let cache_key = format!(
        "material-crop:{size:?}:{}:{:p}",
        desktop_dark(app),
        source.as_ref().map_or(std::ptr::null(), Arc::as_ptr)
    );
    if let Some(image) = app
        .panel
        .as_ref()
        .unwrap()
        .specimens
        .borrow()
        .get(&cache_key)
    {
        return image.clone();
    }
    let (width, height) = (size.0.max(1) as usize, size.1.max(1) as usize);
    let pixels = if let Some(source) = source {
        let aspect = width as f32 / height as f32;
        let sw = (source.width as f32 * 0.58).min(source.height as f32 * aspect);
        let sh = sw / aspect;
        let mut best = (f32::MIN, 0.0, 0.0);
        for row in 0..4 {
            for col in 0..4 {
                let x = (source.width as f32 - sw) * col as f32 / 3.0;
                let y = (source.height as f32 - sh) * row as f32 / 3.0;
                let mut sum = 0.0;
                let mut squares = 0.0;
                for j in 0..12 {
                    for i in 0..12 {
                        let sx = (x + i as f32 * sw / 12.0) as usize;
                        let sy = (y + j as f32 * sh / 12.0) as usize;
                        let at = (sy.min(source.height as usize - 1) * source.width as usize
                            + sx.min(source.width as usize - 1))
                            * 4;
                        let light = (source.pixels[at] as f32
                            + source.pixels[at + 1] as f32
                            + source.pixels[at + 2] as f32)
                            / 765.0;
                        sum += light;
                        squares += light * light;
                    }
                }
                let mean = sum / 144.0;
                let score = (squares / 144.0 - mean * mean) * 4.0 + mean * 0.04;
                if score > best.0 {
                    best = (score, x, y);
                }
            }
        }
        area_crop(&source, Rect::new(best.1, best.2, sw, sh), size).pixels
    } else {
        // Neutral context makes the material legible without suggesting a different wallpaper.
        let mut pixels = vec![255; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let t = (x as f32 / width as f32 * 0.65 + y as f32 / height as f32 * 0.35)
                    .clamp(0.0, 1.0);
                let gray = if desktop_dark(app) {
                    48.0 + t * 58.0
                } else {
                    235.0 - t * 55.0
                } as u8;
                pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].fill(gray);
            }
        }
        pixels
    };
    let image = Arc::new(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    });
    remember(app, cache_key, &image);
    image
}

/// Reuse the owned crop; only one row or column is copied for each separable pass.
fn blur_crop(mut source: Image, radius: usize) -> Vec<u8> {
    let (width, height) = (source.width as usize, source.height as usize);
    if radius == 0 || width == 0 || height == 0 {
        return source.pixels;
    }
    let mut line = vec![0; width.max(height) * 4];
    for horizontal in [true, false] {
        let (lines, length) = if horizontal {
            (height, width)
        } else {
            (width, height)
        };
        for fixed in 0..lines {
            let at = |i: usize| {
                if horizontal {
                    (fixed * width + i) * 4
                } else {
                    (i * width + fixed) * 4
                }
            };
            for i in 0..length {
                line[i * 4..i * 4 + 4].copy_from_slice(&source.pixels[at(i)..at(i) + 4]);
            }
            for i in 0..length {
                let mut sum = [0usize; 3];
                for offset in -(radius as isize)..=radius as isize {
                    let sample = (i as isize + offset).clamp(0, length as isize - 1) as usize;
                    for channel in 0..3 {
                        sum[channel] += line[sample * 4 + channel] as usize;
                    }
                }
                for channel in 0..3 {
                    source.pixels[at(i) + channel] = (sum[channel] / (radius * 2 + 1)) as u8;
                }
            }
        }
    }
    source.pixels
}

pub fn frosted_wallpaper(app: &App, blur: f32, picture_width: u32) -> Option<Arc<Image>> {
    let source = wallpaper(app)?;
    let native_width = app.size.0.clamp(384, 1024);
    let width = picture_width.clamp(1, native_width);
    Some(softened(
        app,
        &source,
        (blur / 2.0 * width as f32 / native_width as f32).round() as usize,
        (width, (width * 2 / 3).max(1)),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialSample {
    pub style: Style,
    pub clarity: f32,
    pub blur: f32,
    pub refraction: f32,
    pub highlight: f32,
    pub tint: Rgba,
    pub radius: f32,
    pub edge: bool,
}

impl MaterialSample {
    pub fn defaults(app: &App, style: Style) -> Self {
        let theme = style_theme(app, style);
        let material = mypaper_ui::theme::material(theme.style, theme.color.surface);
        Self {
            style,
            clarity: material.map_or(1.0 - theme.color.surface.a, |m| m.clarity),
            blur: material.map_or(0.0, |m| m.blur),
            refraction: material.map_or(0.0, |m| m.refraction),
            highlight: material.map_or(0.0, |m| m.highlight),
            tint: theme.color.surface,
            radius: theme.radius.surface,
            edge: style == Style::Fluent,
        }
    }

    pub fn look(app: &App, style: Style, applied: bool) -> Self {
        let mut sample = Self::defaults(app, style);
        sample.tint = super::material::look_tint(app, style);
        if applied {
            sample.clarity = super::material::look_value(app, style, "clarity") as f32;
            sample.blur = super::material::look_value(app, style, "blur") as f32;
            sample.refraction = super::material::look_value(app, style, "refraction") as f32;
            sample.highlight = super::material::look_value(app, style, "highlight") as f32;
            sample.radius = app
                .get(DocId::Config, "/look/shape/corners")
                .and_then(serde_json::Value::as_f64)
                .map_or(sample.radius, |v| v as f32);
            sample.edge = app
                .get(DocId::Config, "/look/shape/edge")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(sample.edge);
        }
        if style == Style::Solid {
            sample.clarity = 0.0;
        }
        sample
    }
}

/// A software lens uses the same defaults as the kit. Its backdrop stays registered through
/// the plate; Liquid bends the crop at its rim, the other styles only frost or tint it.
pub fn material(app: &App, b: &mut B, r: Rect, style: Style) {
    let current: Style = app
        .get(DocId::Config, "/look/style")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    material_sample(
        app,
        b,
        r,
        MaterialSample::look(app, style, current == style),
        None,
    );
}

pub fn material_sample(
    app: &App,
    b: &mut B,
    r: Rect,
    sample: MaterialSample,
    wallpaper: Option<&crate::library::Wallpaper>,
) {
    let size = (
        r.width().round().max(1.0) as u32,
        r.height().round().max(1.0) as u32,
    );
    let background = if let Some(wallpaper) = wallpaper {
        material_backdrop_from(app, size, wallpaper_image(app, wallpaper))
    } else {
        material_backdrop(app, size)
    };
    let style = sample.style;
    let theme = style_theme(app, style);
    let cache_key = format!(
        "material:{sample:?}:{}:{}:{:p}",
        desktop_dark(app),
        b.s,
        Arc::as_ptr(&background)
    );
    let cached = app
        .panel
        .as_ref()
        .unwrap()
        .specimens
        .borrow()
        .get(&cache_key)
        .cloned();
    let image = cached.unwrap_or_else(|| {
        let (width, height) = (background.width as usize, background.height as usize);
        let scale = b.s
            * (r.width() / b.px(220.0))
                .min(r.height() / b.px(130.0))
                .min(1.0);
        let alpha = 1.0 - sample.clarity;
        let frosted = blur_crop(
            background.as_ref().clone(),
            (sample.blur * scale * 0.35).round() as usize,
        );
        let mut pixels = background.pixels.clone();
        let radius = (sample.radius * scale)
            .min(height as f32 * 0.36)
            .min(width as f32 * 0.39);
        let tint = sample.tint;
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (
                    x as f32 - width as f32 / 2.0,
                    y as f32 - height as f32 / 2.0,
                );
                let (qx, qy) = (
                    dx.abs() - (width as f32 * 0.39 - radius),
                    dy.abs() - (height as f32 * 0.36 - radius),
                );
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let at = (y * width + x) * 4;
                if distance > 0.6 {
                    continue;
                }
                let coverage = (0.6 - distance).clamp(0.0, 1.0);
                let rim = (1.0 + distance / (5.0 * scale).max(1.0)).clamp(0.0, 1.0);
                let bend = if style == Style::Liquid {
                    rim * rim * sample.refraction * 10.0 * scale
                } else {
                    0.0
                };
                let sx = (x as f32 - dx.signum() * bend)
                    .round()
                    .clamp(0.0, width as f32 - 1.0) as usize;
                let sy = (y as f32 - dy.signum() * bend)
                    .round()
                    .clamp(0.0, height as f32 - 1.0) as usize;
                let from = (sy * width + sx) * 4;
                let edge = if style == Style::Liquid {
                    rim.powf(4.0) * sample.highlight * if dx + dy < 0.0 { 0.95 } else { 0.38 }
                } else if sample.edge && distance > -1.1 * b.s {
                    0.24
                } else {
                    0.0
                };
                for (channel, color) in [tint.b, tint.g, tint.r].into_iter().enumerate() {
                    let sample = frosted[from + channel] as f32;
                    let body = sample * (1.0 - alpha) + color * 255.0 * alpha;
                    let body = body * (1.0 - edge) + 255.0 * edge;
                    pixels[at + channel] = (pixels[at + channel] as f32 * (1.0 - coverage)
                        + body * coverage)
                        .round() as u8;
                }
            }
        }
        let image = Arc::new(Image {
            width: background.width,
            height: background.height,
            pixels,
        });
        remember(app, cache_key, &image);
        image
    });
    b.list.ops.push(mypaper_ui::Op::RoundedImage {
        rect: r,
        image,
        opacity: 1.0,
        source: None,
        radius: b.px(7.0),
    });
    let surface = Rect::new(
        r.left + r.width() * 0.1,
        r.top + r.height() * 0.26,
        r.width() * 0.8,
        r.height() * 0.5,
    );
    let ink = theme.color.label;
    text(
        b,
        surface,
        "10:42",
        (r.width() / b.s * 0.10).clamp(10.0, 36.0),
        if style == Style::Mono { 400 } else { 600 },
        ink,
        Align::Center,
    );
}

pub fn window(app: &App, b: &mut B, r: Rect, look: usize) {
    window_choice(app, b, r, Some(look));
}

pub fn window_choice(app: &App, b: &mut B, r: Rect, look: Option<usize>) {
    let is_custom = look.is_none();
    let custom = app.get(DocId::Config, "/look/windows");
    let look = look.unwrap_or_else(|| {
        match custom
            .and_then(|v| v.get("backdrop"))
            .and_then(serde_json::Value::as_str)
        {
            Some("acrylic") => 2,
            Some("mica" | "tabbed") => 1,
            _ => {
                if custom
                    .and_then(|v| v.get("corners"))
                    .and_then(serde_json::Value::as_str)
                    == Some("square")
                {
                    3
                } else {
                    0
                }
            }
        }
    });
    let size = (
        r.width().round().max(1.0) as u32,
        r.height().round().max(1.0) as u32,
    );
    let background = material_backdrop(app, size);
    crate::stage::image_round(b, r, &background, 1.0, b.px(8.0));
    let win = Rect::new(
        r.left + r.width() * 0.09,
        r.top + r.height() * 0.12,
        r.width() * 0.82,
        r.height() * 0.77,
    );
    let scale = win.width() / 180.0;
    let style = app
        .get(DocId::Config, "/look/style")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let current_theme = style_theme(app, style);
    let mut radius = if look == 3 {
        0.0
    } else if look == 1 {
        app.get(DocId::Config, "/look/shape/corners")
            .and_then(serde_json::Value::as_f64)
            .map_or(current_theme.radius.surface, |v| v as f32)
            * b.s
    } else {
        8.0 * scale
    };
    if is_custom {
        radius = match custom
            .and_then(|v| v.get("corners"))
            .and_then(serde_json::Value::as_str)
        {
            Some("square") => 0.0,
            Some("small") => 4.0 * scale,
            Some("round") => 8.0 * scale,
            _ => radius,
        };
    }
    let mut light = if look == 0 {
        !windows_dark(app)
    } else {
        !desktop_dark(app)
    };
    if is_custom {
        light = match custom
            .and_then(|v| v.get("apps"))
            .and_then(serde_json::Value::as_str)
        {
            Some("light") => true,
            Some("dark") => false,
            _ => light,
        };
    }
    let ink = hex(if light { 0x1a1a1a } else { 0xf5f5f5 });
    let custom_color = |field| {
        is_custom
            .then(|| {
                custom?
                    .get(field)?
                    .as_str()
                    .and_then(mypaper_config::Color::parse)
            })
            .flatten()
            .map(|color| Rgba::rgb8(color.0, 1.0))
    };
    radius = radius.min(win.height() * 0.22);
    let caption_ink = custom_color("caption_text").unwrap_or(ink);
    let plate = if look == 1 {
        super::material::look_tint(app, style)
    } else {
        hex(if light { 0xf5f5f5 } else { 0x24262b })
    };
    let accent = wallpaper_color(app).unwrap_or(hex(0x8b9eb7));
    if look == 1 || look == 2 {
        let blur = if look == 1 {
            super::material::look_value(app, style, "blur") as f32
        } else {
            14.0
        };
        crate::stage::image_round(
            b,
            win,
            &softened(
                app,
                &background,
                (blur * b.s / 2.0).round() as usize,
                (
                    win.width().round().max(1.0) as u32,
                    win.height().round().max(1.0) as u32,
                ),
            ),
            1.0,
            radius,
        );
    }
    b.fill(
        win,
        radius,
        plate.with_alpha(match look {
            0 | 3 => 1.0,
            1 => {
                if style == Style::Solid {
                    1.0
                } else {
                    1.0 - super::material::look_value(app, style, "clarity") as f32
                }
            }
            _ => 0.45,
        }),
    );
    let caption = Rect::new(
        win.left,
        win.top,
        win.width(),
        b.px(28.0).min(win.height() * 0.3),
    );
    b.fill(
        caption,
        radius,
        custom_color("caption").unwrap_or_else(|| match look {
            1 => current_theme.color.label.with_alpha(0.06),
            2 => hex(0xffffff).with_alpha(0.12),
            _ => ink.with_alpha(0.06),
        }),
    );
    b.stroke(
        win,
        radius,
        b.px(if look == 3 { 1.5 } else { 0.8 }),
        custom_color("border").unwrap_or_else(|| match look {
            0 => ink.with_alpha(0.25),
            2 => hex(0xffffff).with_alpha(0.8),
            _ => accent,
        }),
    );
    if look == 3 {
        b.fill(
            Rect::new(win.left, win.top, win.width(), 2.0 * scale),
            0.0,
            accent,
        );
    }
    text(
        b,
        Rect::new(
            win.left + 8.0 * scale,
            caption.top,
            win.width() * 0.6,
            caption.height(),
        ),
        "Files",
        10.0 * scale / b.s,
        500,
        caption_ink,
        Align::Start,
    );
    for (n, symbol) in ["–", "□", "×"].into_iter().enumerate() {
        text(
            b,
            Rect::new(
                win.right - (3 - n) as f32 * 20.0 * scale,
                caption.top,
                20.0 * scale,
                caption.height(),
            ),
            symbol,
            13.0 * scale / b.s,
            400,
            caption_ink,
            Align::Center,
        );
    }
    let body = Rect::new(
        win.left + 10.0 * scale,
        caption.bottom + 8.0 * scale,
        win.width() - 20.0 * scale,
        (win.bottom - caption.bottom - 16.0 * scale).max(1.0),
    );
    let sidebar = Rect::new(body.left, body.top, body.width() * 0.25, body.height());
    b.fill(sidebar, 3.0 * scale, ink.with_alpha(0.08));
    for n in 0..3 {
        b.fill(
            Rect::new(
                win.left + win.width() * 0.36,
                body.top + body.height() * (0.12 + n as f32 * 0.28),
                win.width() * (0.5 - n as f32 * 0.08),
                (body.height() * 0.08).max(1.0),
            ),
            1.5 * scale,
            ink.with_alpha(if look == 2 { 0.75 } else { 0.35 }),
        );
    }
}

/// Specimens follow the desktop, independently of Studio's own appearance.
pub fn desktop_dark(app: &App) -> bool {
    match app
        .get(DocId::Config, "/look/appearance")
        .and_then(serde_json::Value::as_str)
    {
        Some("light") => false,
        Some("dark") => true,
        _ => windows_dark(app),
    }
}

fn windows_dark(app: &App) -> bool {
    if app.fixture {
        true
    } else {
        crate::theme::Theme::resolve_with(
            crate::theme::Appearance::MatchWindows,
            app.theme.ty.clone(),
        )
        .palette
        .dark
    }
}

/// The shell's real anatomy, rendered in software. Transparent pixels reveal the crop below.
pub fn bars(
    app: &App,
    b: &mut B,
    rect: Rect,
    raw: &serde_json::Value,
    only: Option<crate::actions::Bar>,
) {
    // Lay out at desktop width before reducing: narrow layouts overlap Islands' pills.
    let width = 1280;
    let height = match only {
        Some(crate::actions::Bar::Top) => 180,
        Some(crate::actions::Bar::Dock) => 240,
        None => 540,
    };
    let mut specimen = raw.clone();
    if only != Some(crate::actions::Bar::Top) {
        specimen["dock"]["enabled"] = serde_json::json!(true);
    }
    bars_at(app, b, rect, &specimen, only, (width, height));
}

pub fn desktop_bars(app: &App, b: &mut B, rect: Rect, raw: &serde_json::Value) {
    let size = app
        .display()
        .map(|monitor| {
            let dpi = monitor.dpi as f32 / 96.0;
            (
                (monitor.rect.width as f32 / dpi).max(1.0) as u32,
                (monitor.rect.height as f32 / dpi).max(1.0) as u32,
            )
        })
        .unwrap_or((1280, 720));
    bars_at(app, b, rect, raw, None, size);
}

fn bars_at(
    app: &App,
    b: &mut B,
    rect: Rect,
    raw: &serde_json::Value,
    only: Option<crate::actions::Bar>,
    size: (u32, u32),
) {
    use mypaper_shell::preview::bars as shell;
    use mypaper_ui::{DisplayList, Op};
    let icon_stamp: Vec<_> = raw
        .pointer("/dock/pinned")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pin| pin["path"].as_str())
        .map(|path| {
            super::icons::image(app, path)
                .as_ref()
                .map_or(0, |image| Arc::as_ptr(image) as usize)
        })
        .collect();
    let cache_key = format!(
        "bars:{raw}:{only:?}:{size:?}:{icon_stamp:?}:{:.0}x{:.0}:{}",
        rect.width(),
        rect.height(),
        desktop_dark(app)
    );
    let cached = app
        .panel
        .as_ref()
        .unwrap()
        .specimens
        .borrow()
        .get(&cache_key)
        .cloned();
    let image = cached.or_else(|| {
        let mut config = mypaper_bars::BarsConfig::parse(&raw.to_string()).ok()?;
        config.resolve(shell::default_pins);
        let mut ui = super::measure::raster_ui(app)?;
        ui.text.clear_cache();
        let mut images = shell::Images::default();
        let mut system = mypaper_bars::accent::read();
        system.scheme = if desktop_dark(app) {
            mypaper_bars::accent::Scheme::Dark
        } else {
            mypaper_bars::accent::Scheme::Light
        };
        let model = shell::sample_model(
            windows::Win32::Foundation::SYSTEMTIME {
                wHour: 10,
                wMinute: 42,
                wYear: 2026,
                wMonth: 11,
                wDay: 6,
                ..Default::default()
            },
            "Explorer",
        );
        // Keep desktop proportions without allocating a monitor-sized software bitmap.
        let raster_width = rect.width().round().max(1.0) as u32;
        let scale = raster_width as f32 / size.0.max(1) as f32;
        let raster = (raster_width, (size.1 as f32 * scale).ceil().max(1.0) as u32);
        let mut list = DisplayList::default();
        let mut append = |mut frame: shell::Frame, x: f32, y: f32, tokens: &shell::Tokens| {
            real_pinned_icons(app, &mut frame);
            for (shape, radius) in &frame.shapes {
                let color = tokens.material.map_or(tokens.theme.color.surface, |m| {
                    m.tint.with_alpha((1.0 - m.clarity).max(0.35))
                });
                list.ops.push(Op::Fill {
                    rect: shape.offset(x, y),
                    radius: *radius,
                    color,
                });
            }
            list.ops.extend(crate::stage::offset(&frame.list.ops, x, y));
        };
        if only != Some(crate::actions::Bar::Dock) && config.top.enabled {
            let tokens = shell::Tokens::resolve(&config.top.look, &config.appearance, &system);
            let frame = shell::top(
                raster.0,
                &config,
                &model,
                &tokens,
                scale,
                &mut ui,
                &mut images,
                None,
            );
            append(frame, 0.0, 0.0, &tokens);
        }
        if only != Some(crate::actions::Bar::Top) && config.dock.enabled {
            let tokens = shell::Tokens::resolve(&config.dock.look, &config.appearance, &system);
            let apps: Vec<mypaper_bars::apps::Item> = config
                .dock
                .pinned
                .clone()
                .unwrap_or_default()
                .into_iter()
                .take(8)
                .map(|pin| mypaper_bars::apps::Item {
                    key: mypaper_bars::apps::pin_key(&pin.path),
                    name: pin
                        .name
                        .clone()
                        .unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path)),
                    icon: None,
                    path: pin.path,
                    pinned: true,
                    windows: vec![],
                    aumid: None,
                    exe: None,
                })
                .collect();
            let frame = shell::dock(
                &config,
                &model,
                &apps,
                &tokens,
                scale,
                raster,
                &[],
                &[],
                &mut ui,
                &mut images,
                None,
            );
            let x = (raster.0 as f32 - frame.size.0 as f32) / 2.0;
            let y = if only.is_some() {
                (raster.1 as f32 - frame.size.1 as f32) / 2.0
            } else {
                raster.1 as f32 - frame.size.1 as f32 - 16.0 * scale
            };
            append(frame, x, y, &tokens);
        }
        let output_height = rect.height().round().max(1.0) as u32;
        let vertical_offset = (output_height as f32 - raster.1 as f32) / 2.0;
        let list = DisplayList {
            ops: crate::stage::offset(&list.ops, 0.0, vertical_offset).collect(),
            ..Default::default()
        };
        let image = Arc::new(mypaper_ui::render(&mut ui, &list, raster.0, output_height).ok()?);
        drop(ui);
        super::measure::finish_raster(app);
        remember(app, cache_key, &image);
        Some(image)
    });
    if let Some(image) = image {
        b.list.ops.push(Op::RoundedImage {
            rect,
            image,
            opacity: 1.0,
            source: None,
            radius: b.px(6.0),
        });
    }
}

/// Replace the shell's missing-icon initials with the panel's existing read-only app artwork.
fn real_pinned_icons(app: &App, frame: &mut mypaper_shell::preview::bars::Frame) {
    use mypaper_shell::preview::bars::Target;
    use mypaper_ui::Op;
    for (index, target) in frame.targets.iter().enumerate() {
        let Target::Dock(item) = target else {
            continue;
        };
        let Some(slot) = frame
            .list
            .hits
            .iter()
            .find(|hit| hit.id as usize == index + 1)
            .map(|hit| hit.rect)
        else {
            continue;
        };
        let Some(image) = super::icons::image(app, &item.path) else {
            continue;
        };
        let initial = item
            .name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        let mut replaced = None;
        for op in &mut frame.list.ops {
            if let Op::Fill { rect, radius, .. } = op
                && rect.intersects(&slot)
                && (rect.width() - rect.height()).abs() < 0.1
                && (*radius - rect.width() * 0.225).abs() < 0.2
            {
                let target = *rect;
                *op = Op::Image {
                    rect: target,
                    image: image.clone(),
                    opacity: 1.0,
                    source: None,
                };
                replaced = Some(target);
                break;
            }
        }
        if let Some(icon) = replaced {
            frame.list.ops.retain(|op| !matches!(op,Op::Text {rect,text,..} if text.as_ref() == initial && rect.intersects(&icon)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_clamped_edges_and_the_original_alpha() {
        let source = Image {
            width: 3,
            height: 2,
            pixels: vec![
                0, 15, 30, 10, 30, 45, 60, 20, 60, 75, 90, 30, 90, 105, 120, 40, 120, 135, 150, 50,
                150, 165, 180, 60,
            ],
        };
        assert_eq!(blur_crop(source.clone(), 0), source.pixels);
        assert_eq!(
            blur_crop(source, 1),
            [
                40, 55, 70, 10, 60, 75, 90, 20, 80, 95, 110, 30, 70, 85, 100, 40, 90, 105, 120, 50,
                110, 125, 140, 60,
            ]
        );
    }
    use mypaper_ui::{Ui, controls::Builder};

    #[test]
    fn shrinking_averages_fractional_source_coverage() {
        let image = Image {
            width: 4,
            height: 1,
            pixels: [
                0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
            ]
            .to_vec(),
        };
        let reduced = area_crop(&image, Rect::new(0.0, 0.0, 4.0, 1.0), (2, 1));
        assert_eq!(reduced.pixels, [128, 128, 128, 255, 128, 128, 128, 255]);
        let cropped = area_crop(&image, Rect::new(0.5, 0.0, 1.5, 1.0), (1, 1));
        assert_eq!(cropped.pixels, [170, 170, 170, 255]);
    }

    #[test]
    fn missing_wallpaper_uses_a_neutral_gradient_at_the_real_pixel_size() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.library.items.clear();
        let image = material_backdrop(&app, (672, 246));
        assert_eq!((image.width, image.height), (672, 246));
        assert!(
            image
                .pixels
                .chunks_exact(4)
                .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[3] == 255)
        );
        assert_ne!(&image.pixels[..4], &image.pixels[image.pixels.len() - 4..]);
    }

    #[test]
    fn current_material_picture_and_cache_follow_every_knob_and_tint() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.size = (480, 760);
        let draw = |ui: &mut Ui, sample| {
            let mut b = Builder::new(
                &mut ui.text,
                &app.theme.palette,
                &app.theme.ty,
                1.0,
                &app.input,
            );
            material_sample(
                &app,
                &mut b,
                Rect::new(0.0, 0.0, 424.0, 164.0),
                sample,
                None,
            );
            b.list
                .ops
                .into_iter()
                .find_map(|op| match op {
                    mypaper_ui::Op::RoundedImage { image, .. } => Some(image),
                    _ => None,
                })
                .unwrap()
        };
        let sample = MaterialSample::look(&app, Style::Liquid, true);
        let initial = draw(&mut ui, sample);
        assert_eq!((initial.width, initial.height), (424, 164));
        let changed = [
            MaterialSample {
                clarity: 0.05,
                ..sample
            },
            MaterialSample {
                blur: 32.0,
                ..sample
            },
            MaterialSample {
                refraction: 0.0,
                ..sample
            },
            MaterialSample {
                highlight: 0.0,
                ..sample
            },
            MaterialSample {
                tint: hex(0x239569),
                ..sample
            },
        ];
        for (index, sample) in changed.into_iter().enumerate() {
            assert_ne!(
                draw(&mut ui, sample).pixels,
                initial.pixels,
                "material knob {index}"
            );
        }
        assert!(
            Arc::ptr_eq(&draw(&mut ui, sample), &initial),
            "the applied sample is reused after browsing"
        );
    }
}
