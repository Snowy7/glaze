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
    let cache_key = format!(
        "{}:{:p}:{radius}",
        app.library
            .active()
            .map_or("", |i| app.library.items[i].id.as_str()),
        Arc::as_ptr(image)
    );
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
fn material_backdrop(app: &App) -> Arc<Image> {
    let source = wallpaper(app);
    let cache_key = format!(
        "material-crop:{:p}",
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
    let (width, height) = (180usize, 144usize);
    let mut pixels = vec![255; width * height * 4];
    if let Some(source) = source {
        let sw = (source.width as f32 * 0.58).min(source.height as f32 * 1.25);
        let sh = sw / 1.25;
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
        for y in 0..height {
            for x in 0..width {
                let sx = (best.1 + x as f32 * sw / width as f32) as usize;
                let sy = (best.2 + y as f32 * sh / height as f32) as usize;
                let at = (sy.min(source.height as usize - 1) * source.width as usize
                    + sx.min(source.width as usize - 1))
                    * 4;
                for channel in 0..3 {
                    pixels[(y * width + x) * 4 + channel] =
                        (source.pixels[at + channel] as f32 * 1.32 + 12.0).min(255.0) as u8;
                }
            }
        }
    } else {
        // A colorful desktop crop when no wallpaper is available; no external asset or service.
        for y in 0..height {
            for x in 0..width {
                let band = ((x + y / 2) / 24) % 4;
                let color = [
                    [228, 174, 80],
                    [92, 181, 209],
                    [174, 125, 199],
                    [112, 205, 165],
                ][band];
                pixels[(y * width + x) * 4..(y * width + x) * 4 + 3].copy_from_slice(&color);
            }
        }
    }
    let image = Arc::new(Image {
        width: width as u32,
        height: height as u32,
        pixels,
    });
    remember(app, cache_key, &image);
    image
}

fn blur_crop(source: &Image, radius: usize) -> Vec<u8> {
    let (width, height) = (source.width as usize, source.height as usize);
    let mut pixels = source.pixels.clone();
    for horizontal in [true, false] {
        let input = pixels.clone();
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0usize; 3];
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
                    for channel in 0..3 {
                        sum[channel] += input[(sy * width + sx) * 4 + channel] as usize;
                    }
                    count += 1;
                }
                for channel in 0..3 {
                    pixels[(y * width + x) * 4 + channel] = (sum[channel] / count) as u8;
                }
            }
        }
    }
    pixels
}

pub fn frosted_wallpaper(app: &App, blur: f32) -> Option<Arc<Image>> {
    let source = wallpaper(app)?;
    Some(softened(app, &source, (blur / 2.0).round() as usize))
}

/// A software lens uses the same defaults as the kit. Its backdrop stays registered through
/// the plate; Liquid bends the crop at its rim, the other styles only frost or tint it.
pub fn material(app: &App, b: &mut B, r: Rect, style: Style) {
    let background = material_backdrop(app);
    let theme = style_theme(app, style);
    let cache_key = format!(
        "material:{style:?}:{}:{:p}",
        desktop_dark(app),
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
        let defaults = mypaper_ui::theme::material(theme.style, theme.color.surface);
        let blur = defaults.map_or(0.0, |m| m.blur);
        let alpha = defaults.map_or(
            if style == Style::Solid {
                1.0
            } else {
                theme.color.surface.a
            },
            |m| 1.0 - m.clarity,
        );
        let frosted = blur_crop(&background, (blur * 0.35).round() as usize);
        let mut pixels = background.pixels.clone();
        let radius = theme.radius.surface * 0.72;
        let tint = theme.color.surface;
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as f32 - 89.5, y as f32 - 71.5);
                let (qx, qy) = (dx.abs() - (74.0 - radius), dy.abs() - (49.0 - radius));
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let at = (y * width + x) * 4;
                if distance > 0.6 {
                    continue;
                }
                let coverage = (0.6 - distance).clamp(0.0, 1.0);
                let rim = (1.0 + distance / 5.0).clamp(0.0, 1.0);
                let bend = if style == Style::Liquid {
                    rim * rim * defaults.unwrap().refraction * 10.0
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
                    rim.powf(4.0)
                        * defaults.unwrap().highlight
                        * if dx + dy < 0.0 { 0.95 } else { 0.38 }
                } else if style == Style::Fluent && distance > -1.1 {
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
    crate::stage::image_round(b, r, &image, 1.0, b.px(7.0));
    let surface = Rect::new(
        r.left + r.width() * 0.1,
        r.top + r.height() * 0.24,
        r.width() * 0.8,
        r.height() * 0.5,
    );
    let ink = theme.color.label;
    text(
        b,
        surface,
        "10:42",
        (r.width() / b.s * 0.22).clamp(10.0, 21.0),
        if style == Style::Mono { 400 } else { 600 },
        ink,
        Align::Center,
    );
}

pub fn window(app: &App, b: &mut B, r: Rect, look: usize) {
    let background = material_backdrop(app);
    crate::stage::image_round(b, r, &background, 1.0, b.px(8.0));
    let win = Rect::new(
        r.left + r.width() * 0.09,
        r.top + r.height() * 0.12,
        r.width() * 0.82,
        r.height() * 0.77,
    );
    let scale = win.width() / 180.0;
    let radius = if look == 3 { 0.0 } else { 8.0 * scale };
    let light = if look == 0 {
        !windows_dark(app)
    } else {
        !desktop_dark(app)
    };
    let ink = hex(if light { 0x1a1a1a } else { 0xf5f5f5 });
    let plate = hex(if light { 0xf5f5f5 } else { 0x24262b });
    let accent = wallpaper_color(app).unwrap_or(hex(0x8b9eb7));
    if look == 2 {
        crate::stage::image_round(b, win, &softened(app, &background, 7), 1.0, radius);
    }
    b.fill(
        win,
        radius,
        plate.with_alpha(match look {
            0 | 3 => 1.0,
            1 => 0.9,
            _ => 0.45,
        }),
    );
    let caption = Rect::new(win.left, win.top, win.width(), 25.0 * scale);
    b.fill(
        caption,
        radius,
        match look {
            1 => accent.with_alpha(if light { 0.35 } else { 0.5 }),
            2 => hex(0xffffff).with_alpha(0.12),
            _ => ink.with_alpha(0.06),
        },
    );
    b.stroke(
        win,
        radius,
        b.px(if look == 3 { 1.5 } else { 0.8 }),
        match look {
            0 => ink.with_alpha(0.25),
            2 => hex(0xffffff).with_alpha(0.8),
            _ => accent,
        },
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
        10.0,
        500,
        ink,
        Align::Start,
    );
    for n in 0..3 {
        b.fill(
            Rect::new(
                win.right - (11.0 + n as f32 * 13.0) * scale,
                win.top + 11.0 * scale,
                5.0 * scale,
                1.2 * scale,
            ),
            0.0,
            ink,
        );
    }
    let sidebar = Rect::new(
        win.left + 6.0 * scale,
        caption.bottom + 5.0 * scale,
        win.width() * 0.25,
        win.bottom - caption.bottom - 11.0 * scale,
    );
    b.fill(sidebar, 3.0 * scale, ink.with_alpha(0.08));
    for n in 0..3 {
        b.fill(
            Rect::new(
                win.left + win.width() * 0.36,
                caption.bottom + (12.0 + n as f32 * 16.0) * scale,
                win.width() * (0.5 - n as f32 * 0.08),
                5.0 * scale,
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
    bars_at(app, b, rect, raw, only, (800, 360));
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
    use mypaper_ui::{DisplayList, Op, Ui};
    let cache_key = format!("bars:{raw}:{only:?}:{size:?}:{}", desktop_dark(app));
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
        let mut ui = Ui::new().ok()?;
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
        let raster_width = size.0.min(800).max(1);
        let scale = raster_width as f32 / size.0.max(1) as f32;
        let raster = (raster_width, (size.1 as f32 * scale).ceil().max(1.0) as u32);
        let mut list = DisplayList::default();
        let mut append = |frame: shell::Frame, x: f32, y: f32, tokens: &shell::Tokens| {
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
            let y = if only.is_some() {
                (raster.1 as f32 - frame.size.1 as f32) / 2.0
            } else {
                16.0 * scale
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
                .take(8)
                .map(|pin| mypaper_bars::apps::Item {
                    key: mypaper_bars::apps::pin_key(&pin.path),
                    name: pin
                        .name
                        .clone()
                        .unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path)),
                    path: pin.path,
                    icon: None,
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
        let native = Arc::new(mypaper_ui::render(&mut ui, &list, raster.0, raster.1).ok()?);
        let reduced_height = (384 * size.1 / size.0).max(1);
        let mut reduced = DisplayList::default();
        reduced.ops.push(Op::Image {
            rect: Rect::new(0.0, 0.0, 384.0, reduced_height as f32),
            image: native,
            opacity: 1.0,
            source: None,
        });
        let image = Arc::new(mypaper_ui::render(&mut ui, &reduced, 384, reduced_height).ok()?);
        remember(app, cache_key, &image);
        Some(image)
    });
    if let Some(image) = image {
        crate::stage::image_round(b, rect, &image, 1.0, b.px(6.0));
    }
}
