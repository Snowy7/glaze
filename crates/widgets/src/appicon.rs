//! `appIcon`: the icon of an exe, shortcut, file, folder or `shell:AppsFolder\<AUMID>` as a
//! PNG data URL.
//!
//! Windows makes the icon through `IShellItemImageFactory`, which can be slow (shell
//! extensions, network paths), so one worker thread does it, started on first use. Icons are
//! cached by `(target, size)`; a failure is answered with `null` and not cached.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
};
use windows::core::HSTRING;

use crate::bridge::{Reply, Target};
use crate::media::base64;
use crate::wake::{Msg, Waker};

/// More cached icons than this and the cache starts over.
const MAX_CACHED: usize = 256;

type Cache = Arc<Mutex<HashMap<(String, u32), String>>>;

struct Job {
    target: Target,
    path: String,
    size: u32,
}

pub struct Icons {
    worker: Option<Sender<Job>>,
    cache: Cache,
    waker: Waker,
}

impl Icons {
    pub fn new(waker: Waker) -> Self {
        Self {
            worker: None,
            cache: Cache::default(),
            waker,
        }
    }

    /// The icon of `path` at `size` pixels. A cached icon is answered at once; otherwise the
    /// worker makes it and answers later (`None`).
    pub fn get(&mut self, target: Target, path: &str, size: u32) -> Option<Reply> {
        let key = (path.to_owned(), size);
        if let Some(url) = self.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(Ok(Value::String(url)));
        }
        let job = Job {
            target,
            path: key.0,
            size,
        };
        let worker = self.worker.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let (cache, waker) = (self.cache.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new()
                .name("app-icon".into())
                .spawn(move || {
                    // SAFETY: COM for the shell's icon extraction, for the life of this thread.
                    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                    for Job { target, path, size } in rx {
                        let icon = icon(&path, size);
                        if let (Some(url), Ok(mut cache)) = (&icon, cache.lock()) {
                            if cache.len() >= MAX_CACHED {
                                cache.clear();
                            }
                            cache.insert((path, size), url.clone());
                        }
                        waker.send(Msg::Answered(
                            target,
                            Ok(icon.map_or(Value::Null, Value::String)),
                        ));
                    }
                });
            if let Err(e) = spawned {
                log::warn!("no icon worker: {e}");
            }
            tx
        });
        // A worker that could not start would never answer; say `null` rather than hang.
        match worker.send(job) {
            Ok(()) => None,
            Err(_) => Some(Ok(Value::Null)),
        }
    }
}

/// The PNG data URL of `path`'s icon, or `None` if Windows has none.
fn icon(path: &str, size: u32) -> Option<String> {
    let (width, height, mut bgra) = shell_bitmap(path, size)?;
    to_rgba(&mut bgra);
    let png = encode_png(width, height, &bgra)?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// The shell's bitmap for `path` as `(width, height, BGRA pixels, top row first)`.
fn shell_bitmap(path: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let side = i32::try_from(size).ok()?;
    // SAFETY: COM and GDI calls on objects created here; the bitmap is deleted on every path.
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let bitmap = factory
            .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)
            .ok()?;
        let pixels = read_bitmap(bitmap);
        let _ = DeleteObject(bitmap.into());
        pixels
    }
}

/// # Safety
///
/// `bitmap` is a valid bitmap handle.
unsafe fn read_bitmap(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    // SAFETY: per the contract; `info` and `pixels` outlive the calls that fill them.
    unsafe {
        let mut object = BITMAP::default();
        let described = GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some((&raw mut object).cast()),
        );
        let (width, height) = (
            u32::try_from(object.bmWidth).ok()?,
            u32::try_from(object.bmHeight).ok()?,
        );
        if described == 0 || width == 0 || height == 0 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: object.bmWidth,
                biHeight: -object.bmHeight, // top row first
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let screen = GetDC(None);
        let rows = GetDIBits(
            screen,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, screen);
        (rows != 0).then_some((width, height, pixels))
    }
}

/// Turns the shell's BGRA pixels into RGBA in place. The shell's alpha is straight (measured:
/// colors exceed alpha in its icons); a bitmap without any alpha at all is opaque.
fn to_rgba(pixels: &mut [u8]) {
    let (pixels, _) = pixels.as_chunks_mut::<4>();
    let has_alpha = pixels.iter().any(|p| p[3] != 0);
    for pixel in pixels {
        let [b, g, r, a] = *pixel;
        *pixel = [r, g, b, if has_alpha { a } else { 255 }];
    }
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_becomes_rgba_with_alpha_kept() {
        // Half-transparent red, opaque blue, transparent.
        let mut pixels = vec![0, 0, 255, 128, 255, 0, 0, 255, 0, 0, 0, 0];
        to_rgba(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn a_bitmap_without_alpha_is_opaque() {
        let mut pixels = vec![10, 20, 30, 0, 1, 2, 3, 0];
        to_rgba(&mut pixels);
        assert_eq!(pixels, [30, 20, 10, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn png_round_trips() {
        let rgba: Vec<u8> = (0..4 * 3 * 2).map(|i| (i * 7) as u8).collect();
        let png = encode_png(3, 2, &rgba).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((frame.width, frame.height), (3, 2));
        assert_eq!(&decoded[..frame.buffer_size()], rgba);
    }

    #[test]
    fn the_shell_makes_an_icon_for_a_system_file() {
        // SAFETY: COM for the shell on this test thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let url = icon(r"C:\Windows\System32\notepad.exe", 48).expect("an icon");
        assert!(
            url.starts_with("data:image/png;base64,iVBOR"),
            "{}",
            &url[..40]
        );
        assert_eq!(icon(r"C:\no\such\file.exe", 48), None);
    }
}
