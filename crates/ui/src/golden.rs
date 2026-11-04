//! Golden images: a surface rendered headless (the same software target as on screen) and
//! compared with a PNG in the repo. `MYPAPER_BLESS=1 cargo test` writes the PNGs anew.
//!
//! PNGs hold straight alpha so they open normally; the comparison runs on premultiplied pixels
//! with a small tolerance for antialiasing differences between font versions.

use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::render::Image;

/// A channel may differ by this much...
const CHANNEL_TOLERANCE: u8 = 24;
/// ...and this share of the pixels may differ by more.
const PIXEL_TOLERANCE: f64 = 0.002;

/// Checks `image` against the golden PNG at `path` (written instead when `MYPAPER_BLESS` is
/// set). On a mismatch the actual image is saved to `%TEMP%\mypaper-golden` and named in the
/// error.
pub fn check(image: &Image, path: &Path) -> Result<()> {
    if std::env::var_os("MYPAPER_BLESS").is_some() {
        return write(image, path);
    }
    let golden = read(path).with_context(|| {
        format!(
            "golden {} missing or unreadable; run with MYPAPER_BLESS=1 to write it",
            path.display()
        )
    })?;
    if (golden.width, golden.height) != (image.width, image.height) {
        let actual = save_actual(image, path)?;
        bail!(
            "{}: size {}x{}, golden {}x{} (actual: {})",
            path.display(),
            image.width,
            image.height,
            golden.width,
            golden.height,
            actual.display()
        );
    }
    let differing = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(golden.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE))
        .count();
    let total = (image.width * image.height) as usize;
    if differing as f64 > total as f64 * PIXEL_TOLERANCE {
        let actual = save_actual(image, path)?;
        bail!(
            "{}: {differing} of {total} pixels differ (actual: {})",
            path.display(),
            actual.display()
        );
    }
    Ok(())
}

fn save_actual(image: &Image, golden: &Path) -> Result<PathBuf> {
    let name = golden.file_name().context("golden has no file name")?;
    let path = std::env::temp_dir().join("mypaper-golden").join(name);
    write(image, &path)?;
    Ok(path)
}

/// Writes premultiplied BGRA as a straight-alpha RGBA PNG.
pub fn write(image: &Image, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let rgba: Vec<u8> = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[b, g, r, a]| {
            let straight = |c: u8| {
                if a == 0 {
                    0
                } else {
                    ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                }
            };
            [straight(r), straight(g), straight(b), a]
        })
        .collect();
    let file = std::fs::File::create(path).with_context(|| format!("{}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Reads an RGBA PNG as premultiplied BGRA.
pub fn read(path: &Path) -> Result<Image> {
    let file = std::fs::File::open(path)?;
    let mut reader = png::Decoder::new(BufReader::new(file)).read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().context("PNG too large")?];
    let info = reader.next_frame(&mut buffer)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("{}: not an 8-bit RGBA PNG", path.display());
    }
    let pixels = buffer[..info.buffer_size()]
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let pre = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
            [pre(b), pre(g), pre(r), a]
        })
        .collect();
    Ok(Image {
        width: info.width,
        height: info.height,
        pixels,
    })
}
