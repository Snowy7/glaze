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
    let kit_style = match style {
        Style::Liquid => mypaper_ui::Style::Liquid,
        Style::Oneui => mypaper_ui::Style::OneUi,
        Style::Fluent => mypaper_ui::Style::Fluent,
        Style::Solid => mypaper_ui::Style::Solid,
        Style::Mono => mypaper_ui::Style::Mono,
    };
    let dark = desktop_dark(app);
    let theme = mypaper_ui::Theme::new(
        kit_style,
        if dark {
            mypaper_ui::Scheme::Dark
        } else {
            mypaper_ui::Scheme::Light
        },
        wallpaper_color(app).unwrap_or(hex(0x8b9eb7)),
    );
    let material = mypaper_ui::theme::material(kit_style, theme.color.surface);
    let blur = material.map_or(0, |m| (m.blur / 2.0) as usize);
    let alpha = material.map_or(theme.color.surface.a, |m| 1.0 - m.clarity);
    let radius = theme.radius.card;
    let highlight = material.map_or(if style == Style::Solid { 0.12 } else { 0.0 }, |m| {
        m.highlight
    });
    let radius = b.px(radius).min(surface.height() / 2.0);
    if style != Style::Solid
        && let Some(image) = wallpaper(app)
    {
        crate::stage::image_round(b, surface, &softened(app, &image, blur), 1.0, radius);
    }
    let tint = theme.color.surface;
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
    let ink = if dark { hex(0xffffff) } else { hex(0x1a1a1a) };
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
    let radius = b.px(if look == 3 {
        0.0
    } else if look == 0 {
        3.0
    } else {
        6.0
    });
    if let Some(image) = wallpaper(app) {
        crate::stage::image_round(
            b,
            win,
            &softened(app, &image, if look == 2 { 9 } else { 5 }),
            1.0,
            radius,
        );
    }
    let light = if look == 0 {
        !windows_dark(app)
    } else {
        !desktop_dark(app)
    };
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
            hex(if light { 0xb5b5b5 } else { 0x555555 })
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
        let mut cache = app.panel.as_ref().unwrap().specimens.borrow_mut();
        if cache.len() >= 80 {
            cache.clear();
        }
        cache.insert(cache_key, image.clone());
        Some(image)
    });
    if let Some(image) = image {
        crate::stage::image_round(b, rect, &image, 1.0, b.px(6.0));
    }
}
