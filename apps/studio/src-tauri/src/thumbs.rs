//! Cached preview thumbnails for videos, made by Windows' own thumbnail
//! provider (`IShellItemImageFactory`, the same source Explorer uses) and
//! stored as PNGs in `%LOCALAPPDATA%\mypaper\thumbs`. A thumbnail is keyed on
//! the video's path, size and modified time, so editing the file makes a new one.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY};
use windows::core::{Owned, PCWSTR};

/// Large enough for a 2x card on a high-DPI screen.
const SIZE_PX: SIZE = SIZE { cx: 640, cy: 400 };

fn cache_dir() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("mypaper").join("thumbs"))
}

/// 64-bit FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The cache file name for a video: changes with its path, size and modified time.
fn cache_name(source: &Path, len: u64, modified_nanos: u128) -> String {
    let key = format!("{}|{len}|{modified_nanos}", source.to_string_lossy().to_lowercase());
    format!("{:016x}.png", fnv1a(key.as_bytes()))
}

fn cache_path(source: &Path) -> Result<PathBuf> {
    let meta = std::fs::metadata(source).with_context(|| format!("reading {}", source.display()))?;
    let modified = meta.modified()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    Ok(cache_dir()?.join(cache_name(source, meta.len(), modified)))
}

/// The thumbnail if it was already made.
pub fn cached(source: &Path) -> Option<PathBuf> {
    cache_path(source).ok().filter(|path| path.is_file())
}

/// Makes (or reuses) the thumbnail for a video file and returns its path.
pub fn generate(source: &Path) -> Result<PathBuf> {
    let target = cache_path(source)?;
    if target.is_file() {
        return Ok(target);
    }
    let image = shell_thumbnail(source)?;
    let png = encode_png(&image)?;
    let dir = target.parent().context("cache path has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Write-then-rename so the webview never reads half a file.
    let temp = target.with_extension("png.tmp");
    std::fs::write(&temp, png)?;
    std::fs::rename(&temp, &target)?;
    Ok(target)
}

struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Balances a successful `CoInitializeEx`. A thread that was already set up
/// with another COM mode is fine to use as is, so only success needs undoing.
struct ComScope(bool);

impl ComScope {
    fn enter() -> Self {
        // SAFETY: initializes COM for this thread; paired with `CoUninitialize` in `Drop`.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful `CoInitializeEx` in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Asks the shell for the file's thumbnail (never a generic icon).
fn shell_thumbnail(source: &Path) -> Result<Rgba> {
    let _com = ComScope::enter();
    let wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    let factory: IShellItemImageFactory = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
        .with_context(|| format!("opening {} in the shell", source.display()))?;
    // SAFETY: plain COM call; the bitmap it returns is ours to delete.
    let bitmap = unsafe { factory.GetImage(SIZE_PX, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK) }
        .context("Windows has no thumbnail provider for this video")?;
    // SAFETY: `bitmap` is a fresh GDI handle that nothing else deletes.
    let bitmap = unsafe { Owned::new(bitmap) };

    let mut info = BITMAP::default();
    // SAFETY: `info` is a BITMAP-sized buffer and `bitmap` is a valid GDI bitmap.
    let got = unsafe {
        GetObjectW(HGDIOBJ((*bitmap).0), size_of::<BITMAP>() as i32, Some(std::ptr::from_mut(&mut info).cast::<c_void>()))
    };
    if got == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
        bail!("the thumbnail bitmap is empty");
    }
    let (width, height) = (info.bmWidth, info.bmHeight);

    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative = top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: the screen DC is released below; `pixels` holds `height` rows of `width` BGRA pixels.
    let rows = unsafe {
        let dc = GetDC(None);
        let rows = GetDIBits(dc, *bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast::<c_void>()), &mut header, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        rows
    };
    if rows == 0 {
        bail!("could not read the thumbnail's pixels");
    }
    bgra_to_rgba(&mut pixels);
    Ok(Rgba { width: width as u32, height: height as u32, pixels })
}

/// Swaps B and R in place. Shell thumbnails of video are opaque, but some
/// providers leave alpha at zero, so an all-transparent image is made opaque.
fn bgra_to_rgba(pixels: &mut [u8]) {
    let (px4, _) = pixels.as_chunks_mut::<4>();
    let transparent = px4.iter().all(|px| px[3] == 0);
    for px in px4 {
        px.swap(0, 2);
        if transparent {
            px[3] = 255;
        }
    }
}

fn encode_png(image: &Rgba) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.pixels)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_names_change_with_the_file_and_are_stable() {
        let video = Path::new(r"C:\Videos\Loop.mp4");
        let name = cache_name(video, 1000, 5);
        assert_eq!(name, cache_name(Path::new(r"c:\videos\loop.mp4"), 1000, 5), "case-insensitive like NTFS");
        assert_ne!(name, cache_name(video, 1001, 5));
        assert_ne!(name, cache_name(video, 1000, 6));
        assert_ne!(name, cache_name(Path::new(r"C:\Videos\Other.mp4"), 1000, 5));
        assert!(name.ends_with(".png") && name.len() == 16 + 4);
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn bgra_becomes_rgba_and_empty_alpha_becomes_opaque() {
        let mut opaque = [10, 20, 30, 255, 1, 2, 3, 128];
        bgra_to_rgba(&mut opaque);
        assert_eq!(opaque, [30, 20, 10, 255, 3, 2, 1, 128]);

        let mut unset = [10, 20, 30, 0, 1, 2, 3, 0];
        bgra_to_rgba(&mut unset);
        assert_eq!(unset, [30, 20, 10, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn png_encoding_round_trips() {
        let image = Rgba { width: 2, height: 1, pixels: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        let bytes = encode_png(&image).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(decoded[..8], image.pixels[..]);
    }
}
