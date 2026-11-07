//! Small software material specimens over the current wallpaper. No GPU or engine needed.
use super::{
    controls::text,
    tokens::{hex, radius},
};
use crate::{app::App, docs::DocId, ui::B};
use mypaper_config::look::Style;
use mypaper_ui::controls::Align;
use mypaper_ui::{Image, Rect, Rgba};
use std::sync::Arc;

/// Small immutable kit drawings share source artwork, never native widget models or canvases.
#[derive(Debug, Clone)]
pub(super) struct Drawing {
    pub ops: Arc<[mypaper_ui::Op]>,
    pub size: (f32, f32),
}

pub(super) fn draw_picture(b: &mut B, drawing: &Drawing, target: Rect) {
    b.list.ops.push(mypaper_ui::Op::Clipped {
        rect: target,
        ops: crate::stage::offset(&drawing.ops, target.left, target.top)
            .collect::<Vec<_>>()
            .into(),
    });
}

/// Keep only a page's small previews; large libraries must not grow the panel's footprint.
pub fn remember(app: &App, key: String, image: &Arc<Image>) {
    if image.pixels.len() > 2 << 20 {
        return;
    }
    let state = app.panel.as_ref().unwrap();
    state.chooser.borrow_mut().image_live.insert(key.clone());
    let mut cache = state.specimens.borrow_mut();
    let bytes: usize = cache.values().map(|image| image.pixels.len()).sum();
    if cache.len() >= 48 || bytes + image.pixels.len() > 2 << 20 {
        cache.clear();
        state.reclaim_heap.set(true);
    }
    cache.insert(key, image.clone());
}

/// Retire crops superseded by an arriving still, or by a row scrolling out of view.
fn cached_image(app: &App, key: &str) -> Option<Arc<Image>> {
    let state = app.panel.as_ref().unwrap();
    state.chooser.borrow_mut().image_live.insert(key.to_owned());
    state.specimens.borrow().get(key).cloned()
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
    b.fill(r, b.px(radius::CONTROL), b.p.fill);
    if let Some(image) = wallpaper(app) {
        crate::stage::image_round(b, r, &image, 1.0, b.px(radius::CONTROL));
    }
}

/// The selected still rendered with the same four transforms as the wallpaper host.
pub fn fit_tiles(
    app: &App,
    b: &mut B,
    c: &mut crate::ui::Col,
    current: &str,
    edit: crate::actions::Edit,
    image: Option<Arc<Image>>,
) {
    use super::controls::height;
    use mypaper_ui::{
        Op, Role,
        controls::{Behavior, key},
    };
    text(
        b,
        Rect::new(c.x, c.y, c.w, height(b, 24.0)),
        "Fit",
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    c.y += height(b, 28.0);
    let columns = if c.w < b.px(340.0) || b.t.text_scale > 1.0 {
        2
    } else {
        4
    };
    let gap = b.px(12.0);
    let width = (c.w - (columns - 1) as f32 * gap) / columns as f32;
    let display = app.display().map_or((1920, 1080), |display| {
        (
            display.rect.width.max(1) as u32,
            display.rect.height.max(1) as u32,
        )
    });
    let picture_height = width * display.1 as f32 / display.0 as f32;
    let row_height = picture_height + height(b, 29.0) + b.px(12.0);
    let top = c.y;
    let values = [
        ("cover", "Fill"),
        ("contain", "Fit"),
        ("stretch", "Stretch"),
        ("center", "Center"),
    ];
    b.group(key(("panel-fit", edit.path.as_ref())), |b| {
        for (i, (value, label)) in values.into_iter().enumerate() {
            let rect = Rect::new(
                c.x + (i % columns) as f32 * (width + gap),
                top + (i / columns) as f32 * (row_height + gap),
                width,
                row_height,
            );
            let k = key(("panel-fit", edit.path.as_ref(), i));
            let on = current == value;
            super::controls::card(b, rect, b.hovered(k));
            let screen = Rect::new(rect.left, rect.top, width, picture_height);
            super::controls::card_picture(b, rect, screen, |b, screen| {
                b.fill(screen, 0.0, hex(0x111114));
                if let Some(image) = &image {
                    let native = app
                        .images
                        .borrow()
                        .source_size(image)
                        .unwrap_or((image.width, image.height));
                    let (target, source) =
                        fit_geometry(screen, (image.width, image.height), native, display, i);
                    b.list.ops.push(Op::RoundedImage {
                        rect: target,
                        image: image.clone(),
                        opacity: 1.0,
                        source,
                        radius: 0.0,
                    });
                } else {
                    b.icon(screen, crate::theme::glyph::PHOTO, 18.0, b.p.ink3);
                }
            });
            text(
                b,
                Rect::new(
                    rect.left + b.px(8.0), screen.bottom + b.px(2.0),
                    width - b.px(32.0), height(b, 29.0),
                ),
                label,
                12.0,
                if on { 600 } else { 400 },
                b.p.ink1,
                Align::Start,
            );
            if on {
                super::controls::card_selection(b, rect);
                b.icon(
                    Rect::new(rect.right - b.px(24.0), screen.bottom + b.px(2.0), b.px(16.0), height(b, 29.0)),
                    crate::theme::glyph::CHECK, 12.0, b.p.ink1,
                );
            }
            b.hit_checked(
                k,
                rect,
                Role::RadioButton,
                &format!("Fit: {label}"),
                Some(on),
                crate::actions::Action::Set(edit.clone(), serde_json::json!(value)),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(radius::CARD));
        }
    });
    c.y += 4usize.div_ceil(columns) as f32 * (row_height + gap);
}

fn fit_geometry(
    screen: Rect,
    decoded: (u32, u32),
    native: (u32, u32),
    display: (u32, u32),
    fit: usize,
) -> (Rect, Option<Rect>) {
    let (iw, ih) = (decoded.0.max(1) as f32, decoded.1.max(1) as f32);
    let centered = |width, height| {
        Rect::new(
            screen.left + (screen.width() - width) / 2.0,
            screen.top + (screen.height() - height) / 2.0,
            width,
            height,
        )
    };
    match fit {
        0 => {
            let scale = (screen.width() / iw).max(screen.height() / ih);
            let (width, height) = (screen.width() / scale, screen.height() / scale);
            (
                screen,
                Some(Rect::new(
                    (iw - width) / 2.0,
                    (ih - height) / 2.0,
                    width,
                    height,
                )),
            )
        }
        1 => {
            let scale = (screen.width() / iw).min(screen.height() / ih);
            (centered(iw * scale, ih * scale), None)
        }
        3 => {
            let (nw, nh) = (native.0.max(1) as f32, native.1.max(1) as f32);
            let (dw, dh) = (display.0.max(1) as f32, display.1.max(1) as f32);
            let (width, height) = (nw.min(dw), nh.min(dh));
            (
                centered(screen.width() * width / dw, screen.height() * height / dh),
                Some(Rect::new(
                    (nw - width) / nw * iw / 2.0,
                    (nh - height) / nh * ih / 2.0,
                    width / nw * iw,
                    height / nh * ih,
                )),
            )
        }
        _ => (screen, None),
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
    if let Some(image) = cached_image(app, &cache_key) {
        return image;
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

/// Keep bright detail behind the plate: clear, frosted and opaque must read at card size.
fn material_backdrop(app: &App, size: (u32, u32)) -> Arc<Image> {
    material_backdrop_from(app, size, wallpaper(app))
}

fn material_backdrop_from(app: &App, size: (u32, u32), source: Option<Arc<Image>>) -> Arc<Image> {
    let cache_key = format!(
        "material-crop:{size:?}:{}:{:p}",
        desktop_dark(app),
        source.as_ref().map_or(std::ptr::null(), Arc::as_ptr)
    );
    if let Some(image) = cached_image(app, &cache_key) {
        return image;
    }
    let (width, height) = (size.0.max(1) as usize, size.1.max(1) as usize);
    let pixels = if let Some(source) = source {
        let aspect = width as f32 / height as f32;
        let sw = (source.width as f32 * 0.46).min(source.height as f32 * aspect);
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
                let score = mean * 0.45 + (squares / 144.0 - mean * mean) * 5.0;
                if score > best.0 {
                    best = (score, x, y);
                }
            }
        }
        let mut pixels = area_crop(&source, Rect::new(best.1, best.2, sw, sh), size).pixels;
        // Expose a dim wallpaper crop so clear glass and opaque plates remain comparable.
        let mean = pixels
            .chunks_exact(4)
            .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 765.0)
            .sum::<f32>() / (width * height) as f32;
        let lift = ((0.52 - mean) / (1.0 - mean).max(0.01)).clamp(0.0, 0.3);
        for pixel in pixels.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel = (*channel as f32 * (1.0 - lift) + 255.0 * lift).round() as u8;
            }
        }
        pixels
    } else {
        // Neutral context makes the material legible without suggesting a different wallpaper.
        let mut pixels = vec![255; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let t = (x as f32 / width as f32 * 0.65 + y as f32 / height as f32 * 0.35)
                    .clamp(0.0, 1.0);
                let gray = (240.0 - t * 70.0) as u8;
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
            edge: matches!(style, Style::Solid | Style::Fluent | Style::Mono),
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

/// A software lens uses the kit's own defaults. Saved material controls have a separate
/// editor; choice pictures stay comparable even after the current style has been tuned.
pub fn material(app: &App, b: &mut B, r: Rect, style: Style) {
    material_sample(app, b, r, MaterialSample::defaults(app, style), None);
}

/// Optical details need pixel coverage even in a 130-pixel card. The material values
/// still come from the kit; the lens and grain are drawn here rather than by the engine.
fn paint_material(
    background: &Image,
    sample: MaterialSample,
    surface: Rect,
    scale: f32,
    radius: f32,
) -> Image {
    let (width, height) = (background.width as usize, background.height as usize);
    let style = sample.style;
    let alpha = match style {
        Style::Solid => 1.0,
        Style::Oneui => (1.0 - sample.clarity) * (1.0 + sample.clarity * sample.tint.a),
        _ => 1.0 - sample.clarity,
    };
    let frost = if style == Style::Liquid { 0.16 } else { 0.6 };
    let frosted = blur_crop(
        background.clone(),
        (sample.blur * scale * frost).round() as usize,
    );
    let mut pixels = background.pixels.clone();
    let radius = radius
        .min(surface.height() / 2.0)
        .min(surface.width() / 2.0);
    let (cx, cy) = (
        (surface.left + surface.right) / 2.0,
        (surface.top + surface.bottom) / 2.0,
    );
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let (qx, qy) = (
                dx.abs() - (surface.width() / 2.0 - radius),
                dy.abs() - (surface.height() / 2.0 - radius),
            );
            let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            if distance > 0.5 {
                continue;
            }
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            let rim = (1.0 + distance / (12.0 * scale).max(3.0)).clamp(0.0, 1.0);
            let bend = if style == Style::Liquid {
                rim * rim * sample.refraction * 26.0 * scale
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
                rim.powf(3.0) * sample.highlight * if dx + dy < 0.0 { 1.15 } else { 0.5 }
            } else if sample.edge && style == Style::Solid {
                // A narrow bevel reads as an opaque raised plate at actual card size.
                let bevel = (1.0 + distance / (2.0 * scale).max(2.0)).clamp(0.0, 1.0);
                bevel * if dy < 0.0 { 0.24 } else { -0.28 }
            } else if sample.edge && distance > -0.85 {
                match style {
                    Style::Mono => 0.24,
                    _ => 0.65,
                }
            } else {
                0.0
            };
            let grain = if style == Style::Fluent {
                // Stable, neutral acrylic grain; no random state or image allocation.
                let hash = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
                (((hash ^ (hash >> 13)).wrapping_mul(1274126177) >> 24) as f32 / 255.0 - 0.5) * 15.0
            } else {
                0.0
            };
            let at = (y * width + x) * 4;
            for (channel, color) in [sample.tint.b, sample.tint.g, sample.tint.r]
                .into_iter()
                .enumerate()
            {
                let body =
                    frosted[from + channel] as f32 * (1.0 - alpha) + color * 255.0 * alpha + grain;
                let body = if edge < 0.0 {
                    body * (1.0 + edge)
                } else {
                    body * (1.0 - edge) + 255.0 * edge
                };
                pixels[at + channel] = (pixels[at + channel] as f32 * (1.0 - coverage)
                    + body * coverage)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    Image {
        width: background.width,
        height: background.height,
        pixels,
    }
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
    let cache_key = format!(
        "material:{sample:?}:{}:{}:{:p}",
        desktop_dark(app),
        b.s,
        Arc::as_ptr(&background)
    );
    let cached = cached_image(app, &cache_key);
    let image = cached.unwrap_or_else(|| {
        // Compose a whole weather widget, leaving room around every edge.
        let scale = (r.width() / 280.0).min(r.height() / 180.0);
        let surface = Rect::new(
            r.width() * 0.08,
            r.height() * 0.10,
            r.width() * 0.84,
            r.height() * 0.80,
        );
        // Preserve the style's silhouette after reducing a whole widget into a card.
        let radius = sample.radius
            * scale
            * match sample.style {
                Style::Oneui => 1.7,
                Style::Solid => 0.55,
                _ => 1.0,
            };
        let image = Arc::new(paint_material(&background, sample, surface, scale, radius));
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
        r.left + r.width() * 0.08,
        r.top + r.height() * 0.10,
        r.width() * 0.84,
        r.height() * 0.80,
    );
    let content_start = b.list.ops.len();
    if let Some(widget) = app.registry.get("weather") {
        let instance = mypaper_widgets::layout::Instance {
            instance: "style-weather".into(),
            widget: "weather".into(),
            monitor: String::new(),
            cell: [0, 0],
            size: [3, 2],
            settings: Default::default(),
            appearance: mypaper_theme::Appearance {
                surface: Some(mypaper_theme::Surface::None),
                text: Some(if desktop_dark(app) {
                    mypaper_theme::TextTone::Light
                } else {
                    mypaper_theme::TextTone::Dark
                }),
                ..Default::default()
            },
        };
        super::editors::widgets::face(app, b, &instance, &widget.manifest, "", "", surface);
    }
    let content = b.list.ops.split_off(content_start);
    b.list.ops.push(mypaper_ui::Op::Clipped {
        rect: r,
        ops: content.into(),
    });
}

pub fn window(app: &App, b: &mut B, r: Rect, look: usize) {
    window_choice(app, b, r, Some(look));
}

pub fn window_choice(app: &App, b: &mut B, r: Rect, look: Option<usize>) {
    window_at(app, b, r, look, true);
}

/// The selected Windows look at readable native size, using the chooser's same drawing.
pub fn window_stage(app: &App, b: &mut B, r: Rect) {
    window_at(app, b, r, None, false);
}

fn window_at(app: &App, b: &mut B, r: Rect, look: Option<usize>, thumbnail: bool) {
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
    let win = if thumbnail {
        Rect::new(
            r.left + r.width() * 0.08,
            r.top + r.height() * 0.12,
            r.width() * 0.84,
            r.height() * 0.76,
        )
    } else {
        let width = b.px(520.0).min(r.width() - b.px(48.0));
        let height = b.px(340.0).min(r.height() - b.px(48.0));
        Rect::new(r.left + (r.width() - width) / 2.0, r.top + (r.height() - height) / 2.0, width, height)
    };
    let content_start = b.list.ops.len();
    let scale = if thumbnail {
        (win.width() / 180.0).min(win.height() / 112.0)
    } else {
        b.s
    };
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
            * scale
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
        let sample = if look == 1 {
            MaterialSample::look(app, style, true)
        } else {
            MaterialSample {
                clarity: 0.48,
                tint: plate,
                ..MaterialSample::defaults(app, Style::Fluent)
            }
        };
        let surface = win.offset(-r.left, -r.top);
        let cache_key = format!(
            "window-material:{sample:?}:{surface:?}:{scale}:{radius}:{:p}",
            Arc::as_ptr(&background)
        );
        let image = cached_image(app, &cache_key).unwrap_or_else(|| {
            let image = Arc::new(paint_material(&background, sample, surface, scale, radius));
            remember(app, cache_key, &image);
            image
        });
        crate::stage::image_round(b, r, &image, 1.0, b.px(8.0));
    } else {
        crate::stage::image_round(b, r, &background, 1.0, b.px(8.0));
        b.fill(win, radius, plate);
    }
    let caption = Rect::new(
        win.left,
        win.top,
        win.width(),
        (28.0 * scale).min(win.height() * 0.3),
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
            (win.width() - 76.0 * scale).max(1.0),
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
    let content = b.list.ops.split_off(content_start);
    b.list.ops.push(mypaper_ui::Op::Clipped {
        rect: r,
        ops: content.into(),
    });
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
    // A bar strip, rather than a whole monitor: modules and the silhouette remain readable.
    let width = 400;
    let height = match only {
        Some(crate::actions::Bar::Top) => 100,
        Some(crate::actions::Bar::Dock) => 140,
        None => 250,
    };
    let mut specimen = raw.clone();
    if only != Some(crate::actions::Bar::Top) {
        specimen["dock"]["enabled"] = serde_json::json!(true);
    }
    let background = material_backdrop(
        app,
        (
            rect.width().ceil().max(1.0) as u32,
            rect.height().ceil().max(1.0) as u32,
        ),
    );
    crate::stage::image_round(b, rect, &background, 1.0, b.px(6.0));
    bars_at(app, b, rect, &specimen, only, (width, height), true);
}

/// A compact desktop for starter cards, with enough room for the actual bar thickness.
pub fn starter_bars(app: &App, b: &mut B, rect: Rect, raw: &serde_json::Value) {
    bars_at(app, b, rect, raw, None, (560, 315), false);
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
    bars_at(app, b, rect, raw, None, size, false);
}

fn bars_at(
    app: &App,
    b: &mut B,
    rect: Rect,
    raw: &serde_json::Value,
    only: Option<crate::actions::Bar>,
    size: (u32, u32),
    strip: bool,
) {
    use mypaper_shell::preview::bars as shell;
    use mypaper_ui::{DisplayList, Op};
    let compact = size.0 <= 560;
    let icon_stamp: Vec<_> = raw
        .pointer("/dock/pinned")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|_| only != Some(crate::actions::Bar::Top))
        .take(if compact { 4 } else { 8 })
        .filter_map(|pin| pin["path"].as_str())
        .map(|path| {
            super::icons::image(app, path)
                .as_ref()
                .map_or(0, |image| Arc::as_ptr(image) as usize)
        })
        .collect();
    let identity = format!(
        "bars:{raw}:{only:?}:{size:?}:{strip}:{:.0}x{:.0}:{}:",
        rect.width(),
        rect.height(),
        desktop_dark(app)
    );
    let cache_key = format!("{identity}{icon_stamp:?}");
    let cached = super::choosers::drawing(app, &cache_key);
    let drawing = cached.or_else(|| {
        if !super::choosers::claim_raster(app) {
            return None;
        }
        let mut config = mypaper_bars::BarsConfig::parse(&raw.to_string()).ok()?;
        config.resolve(shell::default_pins);
        if compact {
            // Keep the design's native geometry with a small, plausible set of modules.
            // A monitor's entire status area cannot fit in a 200-DIP choice card.
            use mypaper_bars::config::{Module, ModuleRef};
            config.top.left.retain(|module| {
                matches!(
                    module,
                    ModuleRef::Builtin(Module::Start | Module::App) | ModuleRef::Break
                )
            });
            config.top.right.retain(|module| {
                matches!(
                    module,
                    ModuleRef::Builtin(Module::Volume | Module::Quick | Module::Power)
                        | ModuleRef::Break
                )
            });
            config.top.clock.format = Some("HH:mm".into());
            config
                .dock
                .left
                .retain(|module| matches!(module, ModuleRef::Builtin(Module::Start)));
            config.dock.right.retain(|module| {
                matches!(module, ModuleRef::Builtin(Module::Clock | Module::Volume))
            });
            config.dock.clock.format = Some("HH:mm".into());
            config.dock.titles = false;
        }
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
        let raster = (
            raster_width,
            if strip {
                rect.height().ceil().max(1.0) as u32
            } else {
                (size.1 as f32 * scale).ceil().max(1.0) as u32
            },
        );
        let mut list = DisplayList::default();
        let mut vertical = None::<(f32, f32)>;
        let mut append = |mut frame: shell::Frame, x: f32, y: f32, tokens: &shell::Tokens| {
            let (top, bottom) = (y - 8.0 * scale, y + frame.size.1 as f32 + 8.0 * scale);
            vertical = Some(vertical.map_or((top, bottom), |(a, b)| (a.min(top), b.max(bottom))));
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
            let y = if strip {
                if only.is_some() {
                    (raster.1 as f32 - frame.size.1 as f32) / 2.0
                } else {
                    raster.1 as f32 * if config.dock.enabled { 0.12 } else { 0.4 }
                }
            } else {
                0.0
            };
            append(frame, 0.0, y, &tokens);
        }
        if only != Some(crate::actions::Bar::Top) && config.dock.enabled {
            let tokens = shell::Tokens::resolve(&config.dock.look, &config.appearance, &system);
            let apps: Vec<mypaper_bars::apps::Item> = config
                .dock
                .pinned
                .clone()
                .unwrap_or_default()
                .into_iter()
                .take(if compact { 4 } else { 8 })
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
            let y = if only.is_some() || strip && !config.top.enabled {
                (raster.1 as f32 - frame.size.1 as f32) / 2.0
            } else if strip {
                (raster.1 as f32 * 0.62)
                    .min(raster.1 as f32 - frame.size.1 as f32 - 6.0 * scale)
                    .max(0.0)
            } else {
                raster.1 as f32 - frame.size.1 as f32 - 16.0 * scale
            };
            append(frame, x, y, &tokens);
        }
        let output_height = rect.height().round().max(1.0) as u32;
        let vertical_offset = (output_height as f32 - raster.1 as f32) / 2.0;
        // A single bar uses a narrow canvas. Its wallpaper already belongs to the card;
        // retaining the transparent area above and below it only multiplies native copies.
        let (crop_top, crop_height) = if strip {
            let (top, bottom) = vertical.unwrap_or((0.0, 1.0));
            let top = top.floor().clamp(0.0, output_height as f32 - 1.0);
            let bottom = bottom.ceil().clamp(top + 1.0, output_height as f32);
            (top, (bottom - top) as u32)
        } else {
            (0.0, output_height)
        };
        let drawing = Drawing {
            ops: crate::stage::offset(&list.ops, 0.0, vertical_offset - crop_top)
                .collect::<Vec<_>>()
                .into(),
            size: (raster.0 as f32, crop_height as f32),
        };
        drop(ui);
        super::measure::finish_raster(app);
        super::choosers::remember_drawing(app, cache_key, drawing.clone(), Some(&identity));
        Some(drawing)
    });
    if let Some(drawing) = drawing {
        let target = if strip {
            Rect::new(
                rect.left,
                rect.top + (rect.height() - drawing.size.1) / 2.0,
                rect.width(),
                drawing.size.1,
            )
        } else {
            rect
        };
        draw_picture(b, &drawing, target);
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
    fn card_materials_keep_their_optical_signatures() {
        let ui = Ui::new().unwrap();
        let app = crate::goldens::app(true, &ui);
        let mut background = Image {
            width: 130,
            height: 82,
            pixels: vec![255; 130 * 82 * 4],
        };
        for (index, pixel) in background.pixels.chunks_exact_mut(4).enumerate() {
            pixel[..3].fill(if index % 130 / 6 % 2 == 0 { 40 } else { 220 });
        }
        let surface = Rect::new(10.0, 8.0, 110.0, 66.0);
        let draw = |sample| paint_material(&background, sample, surface, 0.46, 8.0);
        let center = (41 * 130 + 65) * 4;
        let solid = MaterialSample::defaults(&app, Style::Solid);
        let opaque = draw(solid);
        assert_eq!(&opaque.pixels[center..center + 3], &[30, 28, 28]);
        let mono = MaterialSample::defaults(&app, Style::Mono);
        let flat = draw(mono);
        for (channel, tint) in [mono.tint.b, mono.tint.g, mono.tint.r]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                flat.pixels[center + channel],
                (background.pixels[center + channel] as f32 * mono.clarity
                    + tint * 255.0 * (1.0 - mono.clarity))
                    .round() as u8,
                "Mono uses the original backdrop pixel, without frost"
            );
        }
        let liquid = MaterialSample::defaults(&app, Style::Liquid);
        assert_ne!(
            draw(liquid).pixels,
            draw(MaterialSample {
                refraction: 0.0,
                ..liquid
            })
            .pixels
        );
        assert_ne!(
            draw(liquid).pixels,
            draw(MaterialSample {
                highlight: 0.0,
                ..liquid
            })
            .pixels
        );
        let oneui = MaterialSample::defaults(&app, Style::Oneui);
        assert_eq!(
            &draw(MaterialSample {
                clarity: 1.0,
                blur: 0.0,
                ..oneui
            })
            .pixels[center..center + 3],
            &background.pixels[center..center + 3],
            "the clarity control still reaches clear"
        );
        let uniform = Image {
            width: 130,
            height: 82,
            pixels: vec![255; 130 * 82 * 4],
        };
        let acrylic = paint_material(
            &uniform,
            MaterialSample::defaults(&app, Style::Fluent),
            surface,
            0.46,
            4.0,
        );
        let row = (30..100)
            .map(|x| acrylic.pixels[(41 * 130 + x) * 4])
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            row.len() > 5,
            "Fluent retains grain even over a uniform backdrop"
        );
    }

    #[test]
    fn solid_has_a_raised_rim_while_mono_has_only_a_flat_hairline() {
        let ui = Ui::new().unwrap();
        for dark in [true, false] {
            let app = crate::goldens::app(dark, &ui);
            let background = Image { width: 130, height: 82, pixels: [80, 80, 80, 255].repeat(130 * 82) };
            let surface = Rect::new(10.0, 8.0, 110.0, 66.0);
            let draw = |style| paint_material(&background, MaterialSample::defaults(&app, style), surface, 0.46, 8.0);
            let pixel = |image: &Image, y: usize| image.pixels[(y * 130 + 65) * 4] as i32;
            let solid = draw(Style::Solid);
            assert!(pixel(&solid, 9) > pixel(&solid, 41) + 8, "Solid needs a visible raised top edge");
            assert!(pixel(&solid, 72) < pixel(&solid, 41), "Solid's lower rim gives the plate depth");
            let mono = draw(Style::Mono);
            assert!(MaterialSample::defaults(&app, Style::Mono).clarity >= 0.2,
                "Mono must transmit enough backdrop to read as a translucent tint at card size");
            assert_eq!(pixel(&mono, 9), pixel(&mono, 41), "Mono has no inner bevel or shading");
            assert_eq!(pixel(&mono, 72), pixel(&mono, 41));
        }
    }

    #[test]
    fn superseded_crops_retire_after_the_replacement_frame_is_presented() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        let make = || {
            Arc::new(Image {
                width: 4,
                height: 4,
                pixels: vec![255; 64],
            })
        };
        let old = make();
        let current = make();
        super::super::choosers::begin(&app);
        remember(&app, "material:pending".into(), &old);
        super::super::choosers::presented(&app);
        super::super::choosers::begin(&app);
        remember(&app, "material:decoded".into(), &current);
        assert_eq!(
            Arc::strong_count(&old),
            2,
            "keep the shown frame until presentation"
        );
        super::super::choosers::presented(&app);
        assert_eq!(Arc::strong_count(&old), 1);
        super::super::choosers::begin(&app);
        assert!(cached_image(&app, "material:decoded").is_some());
        super::super::choosers::presented(&app);
        assert_eq!(
            Arc::strong_count(&current),
            2,
            "a visible cache hit stays cached"
        );
        super::super::choosers::begin(&app);
        super::super::choosers::presented(&app);
        assert_eq!(
            Arc::strong_count(&current),
            1,
            "a departed row keeps no crop"
        );
    }

    #[test]
    fn individual_bar_previews_keep_compact_native_drawings_without_raster_copies() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        for bar in [crate::actions::Bar::Top, crate::actions::Bar::Dock] {
            let mut b = Builder::new(
                &mut ui.text,
                &app.theme.palette,
                &app.theme.ty,
                1.0,
                &app.input,
            );
            bars(
                &app,
                &mut b,
                Rect::new(0.0, 0.0, 200.0, 112.0),
                &serde_json::json!({"version":1,"preset":"macos","dock":{"pinned":[]}}),
                Some(bar),
            );
            let picture = b
                .list
                .ops
                .iter()
                .rev()
                .find_map(|op| match op {
                    mypaper_ui::Op::Clipped { rect, ops } if !ops.is_empty() => Some(rect),
                    _ => None,
                })
                .unwrap();
            assert_eq!(picture.width(), 200.0);
            assert!(
                picture.height() < 70.0,
                "{bar:?} kept a {}-pixel band",
                picture.height()
            );
            assert!(
                !app.panel
                    .as_ref()
                    .unwrap()
                    .specimens
                    .borrow()
                    .keys()
                    .any(|key| key.starts_with("bars:"))
            );
        }
    }

    #[test]
    fn an_arriving_app_icon_replaces_the_bar_card_instead_of_caching_both_versions() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        let raw = serde_json::json!({"version":1,"preset":"macos",
            "dock":{"pinned":[{"path":"raster-test-missing.exe","name":"Sample"}]}});
        let draw = |text: &mut mypaper_ui::Text| {
            let mut b = Builder::new(text, &app.theme.palette, &app.theme.ty, 1.0, &app.input);
            bars(&app, &mut b, Rect::new(0.0, 0.0, 200.0, 112.0), &raw, None);
        };
        draw(&mut ui.text);
        app.panel.as_ref().unwrap().specimens.borrow_mut().insert(
            "app-icon:raster-test-missing.exe".into(),
            Arc::new(Image {
                width: 4,
                height: 4,
                pixels: vec![255; 64],
            }),
        );
        draw(&mut ui.text);
        let chooser = app.panel.as_ref().unwrap().chooser.borrow();
        let cache = &chooser.drawings;
        assert_eq!(
            cache.keys().filter(|key| key.starts_with("bars:")).count(),
            1
        );
    }

    #[test]
    fn wallpaper_fits_preserve_the_still_and_center_uses_native_pixels() {
        let screen = Rect::new(0.0, 0.0, 160.0, 90.0);
        let decoded = (100, 100);
        let native = (800, 800);
        let display = (1600, 900);
        let (target, crop) = fit_geometry(screen, decoded, native, display, 0);
        assert_eq!(target, screen);
        assert_eq!(crop, Some(Rect::new(0.0, 21.875, 100.0, 56.25)));
        assert_eq!(
            fit_geometry(screen, decoded, native, display, 1),
            (Rect::new(35.0, 0.0, 90.0, 90.0), None)
        );
        assert_eq!(
            fit_geometry(screen, decoded, native, display, 2),
            (screen, None)
        );
        assert_eq!(
            fit_geometry(screen, decoded, native, display, 3),
            (
                Rect::new(40.0, 5.0, 80.0, 80.0),
                Some(Rect::new(0.0, 0.0, 100.0, 100.0))
            )
        );
        let (_, crop) = fit_geometry(screen, decoded, (3200, 1800), display, 3);
        assert_eq!(crop, Some(Rect::new(25.0, 25.0, 50.0, 50.0)));
    }

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
