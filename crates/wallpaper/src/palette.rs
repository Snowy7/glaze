//! Wallpaper palette: once per newly loaded theme, the engine shrinks the
//! first screen's first kept frame (the wallpaper as drawn) to a thumbnail on
//! the GPU, reads that back once, and writes a dominant and an accent color to
//! `%LOCALAPPDATA%\mypaper\palette.json` for core's `"auto"` colors. Never per
//! frame: a theme load costs one thumbnail blit and a 9 KB readback, and no
//! screen-sized texture (on the integrated GPU a freed heap stays committed).

use std::path::Path;
use std::sync::mpsc;

use anyhow::{Context, Result, bail};
use mypaper_theme::{Color, Fit};
use mypaper_tweaks::color::Hsl;
use mypaper_tweaks::palette::{Palette, Snapshot};

use crate::gpu::{Gpu, KeptFrame};
use crate::sources;

/// Thumbnail size. 64 pixels of RGBA is exactly wgpu's 256-byte row alignment.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 36;
const ROW_BYTES: u32 = WIDTH * 4;

/// Pixels this dark or this bright carry no color information.
const NEAR_BLACK: u8 = 48;
const NEAR_WHITE: u8 = 235;
/// Below this saturation a pixel is gray and has no hue worth voting for.
const NEUTRAL_SATURATION: f32 = 0.15;
const HUE_BINS: usize = 24;
/// Accents must stay readable on both light and dark surfaces.
const ACCENT_SATURATION: (f32, f32) = (0.6, 1.0);
const ACCENT_LIGHTNESS: (f32, f32) = (0.45, 0.62);

/// Computes the palette of `kept` (a screen's just-drawn wallpaper) and writes it for
/// `theme`. Failures are logged: a palette is a nicety.
pub fn publish(gpu: &Gpu, kept: &KeptFrame, theme: &Path) {
    let result = capture(gpu, kept).and_then(|pixels| {
        let palette = extract(&pixels).context("the first frame has no usable color")?;
        mypaper_tweaks::palette::write(&Snapshot {
            theme: theme.to_owned(),
            palette,
        })?;
        log::info!(
            "palette: dominant {}, accent {}",
            mypaper_tweaks::color::hex(palette.dominant),
            mypaper_tweaks::color::hex(palette.accent)
        );
        Ok(())
    });
    if let Err(e) = result {
        log::warn!("no palette: {e:#}");
    }
}

/// Downsamples the kept frame with the shared blit (stretch) and reads the thumbnail back
/// as RGB.
fn capture(gpu: &Gpu, kept: &KeptFrame) -> Result<Vec<[u8; 3]>> {
    let swap_red_blue = match gpu.format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
        other => bail!("cannot read back {other:?}"),
    };
    let small = gpu.texture(
        "palette thumbnail",
        (WIDTH, HEIGHT),
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("palette readback"),
        size: u64::from(ROW_BYTES * HEIGHT),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let small_view = small.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let size = (kept.texture.width(), kept.texture.height());
    let quad = gpu.blit.quad(&gpu.device, &kept.view);
    quad.set(&gpu.queue, size, (WIDTH, HEIGHT), Fit::Stretch, 1.0);
    {
        let mut pass = sources::begin(
            &mut encoder,
            &small_view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        quad.draw(&gpu.blit, &mut pass);
    }
    encoder.copy_texture_to_buffer(
        small.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_BYTES),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv()??;

    let mapped = readback.slice(..).get_mapped_range()?;
    let pixels = mapped
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            if swap_red_blue {
                [p[2], p[1], p[0]]
            } else {
                [p[0], p[1], p[2]]
            }
        })
        .collect();
    drop(mapped);
    readback.unmap();
    Ok(pixels)
}

/// Picks a dominant and an accent color from RGB pixels, or `None` if there
/// is nothing but black.
///
/// Near-black and near-white pixels are ignored (a wallpaper that is *only*
/// dark still yields a palette from its non-black pixels). Pixels vote for a
/// hue bin: by count for the dominant color, and by squared vividness
/// (saturation times midtone-ness) for the accent, so a small vivid streak can
/// win the accent while the dominant stays the broad color. The accent is then
/// pushed to a saturation and lightness that works as a UI color.
pub fn extract(pixels: &[[u8; 3]]) -> Option<Palette> {
    let kept = usable(pixels, NEAR_BLACK, NEAR_WHITE);
    let kept = if kept.is_empty() {
        usable(pixels, 8, 247)
    } else {
        kept
    };
    if kept.is_empty() {
        return None;
    }

    // The last bin holds the grays.
    let mut bins = [Bin::default(); HUE_BINS + 1];
    for &rgb in &kept {
        let hsl = Hsl::from(Color(rgb));
        let index = if hsl.s < NEUTRAL_SATURATION {
            HUE_BINS
        } else {
            (hsl.h / 360.0 * HUE_BINS as f32) as usize % HUE_BINS
        };
        let bin = &mut bins[index];
        bin.count += 1;
        bin.vividness += (hsl.s * (1.0 - (2.0 * hsl.l - 1.0).abs())).powi(2);
        for (sum, channel) in bin.sum.iter_mut().zip(rgb) {
            *sum += u32::from(channel);
        }
    }

    let dominant = bins.iter().max_by_key(|b| b.count)?.mean()?;
    let vivid = bins[..HUE_BINS]
        .iter()
        .filter(|b| b.count > 0)
        .max_by(|a, b| a.vividness.total_cmp(&b.vividness))
        .and_then(Bin::mean);
    let accent = match vivid {
        Some(color) => accentuate(color),
        // Only grays: keep the neutral, just make it legible.
        None => {
            let hsl = Hsl::from(dominant);
            Hsl {
                l: hsl.l.clamp(ACCENT_LIGHTNESS.0, ACCENT_LIGHTNESS.1),
                ..hsl
            }
            .into()
        }
    };
    Some(Palette { dominant, accent })
}

fn usable(pixels: &[[u8; 3]], min_peak: u8, max_floor: u8) -> Vec<[u8; 3]> {
    pixels
        .iter()
        .copied()
        .filter(|p| p.iter().max() >= Some(&min_peak) && p.iter().min() <= Some(&max_floor))
        .collect()
}

/// Boosts saturation and clamps lightness so a color works as an accent.
fn accentuate(color: Color) -> Color {
    let hsl = Hsl::from(color);
    Hsl {
        h: hsl.h,
        s: (hsl.s * 1.25).clamp(ACCENT_SATURATION.0, ACCENT_SATURATION.1),
        l: hsl.l.clamp(ACCENT_LIGHTNESS.0, ACCENT_LIGHTNESS.1),
    }
    .into()
}

/// Pixels that voted for one hue range.
#[derive(Clone, Copy, Default)]
struct Bin {
    count: u32,
    vividness: f32,
    sum: [u32; 3],
}

impl Bin {
    fn mean(&self) -> Option<Color> {
        (self.count > 0).then(|| Color(self.sum.map(|s| (s / self.count) as u8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hue(color: Color) -> f32 {
        Hsl::from(color).h
    }

    #[test]
    fn a_vivid_streak_wins_the_accent_while_the_broad_color_stays_dominant() {
        // Mostly muted teal, a few saturated violet pixels, black and white filler.
        let mut pixels = vec![[40, 90, 100]; 60];
        pixels.extend([[150, 40, 230]; 8]);
        pixels.extend([[0, 0, 0]; 20]);
        pixels.extend([[255, 255, 255]; 20]);
        let palette = extract(&pixels).unwrap();

        assert!(
            (170.0..200.0).contains(&hue(palette.dominant)),
            "{palette:?}"
        );
        assert!((260.0..290.0).contains(&hue(palette.accent)), "{palette:?}");
        let accent = Hsl::from(palette.accent);
        assert!(accent.s >= ACCENT_SATURATION.0 && (0.45..=0.62).contains(&accent.l));
    }

    #[test]
    fn black_and_white_are_skipped_not_averaged() {
        let mut pixels = vec![[0, 0, 0]; 90];
        pixels.extend([[200, 30, 30]; 10]);
        let palette = extract(&pixels).unwrap();
        assert!(hue(palette.dominant) < 10.0 || hue(palette.dominant) > 350.0);
        assert_eq!(extract(&[[0, 0, 0]; 50]), None);
        assert_eq!(extract(&[[255, 255, 255]; 50]), None);
    }

    #[test]
    fn a_dark_only_wallpaper_still_gets_a_palette_and_grays_stay_gray() {
        let navy = extract(&[[4, 8, 30]; 50]).unwrap();
        assert!((200.0..260.0).contains(&hue(navy.dominant)), "{navy:?}");

        let gray = extract(&[[120, 120, 120]; 50]).unwrap();
        assert_eq!(Hsl::from(gray.accent).s, 0.0);
    }
}
