//! What's behind a bar (docs/bars.md, "Liquid glass"): Liquid Glass bends the pixels behind it,
//! and a WebView can't see those. So the page asks (`capture`), and the host copies that part of
//! the screen with GDI (`BitBlt` from the screen DC) while every bar window is excluded from
//! capture (`SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, followed by `DwmFlush`). The copy is
//! the wallpaper's current frame over the bare desktop, or
//! the windows behind a flyout. It's written as a 24-bit BMP under `backdrop\behind\` (served from
//! `backdrop.bars.mypaper`), and the page lenses it.
//!
//! Cost: one copy per request (a 380×260 area took 3–7 ms, the affinity change under 1 ms);
//! nothing runs between requests. While a bar is excluded (those few milliseconds) a screen
//! recording doesn't show it.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

/// The most pixels one capture may cover (a 4K screen).
const MAX_PIXELS: i64 = 3840 * 2160;
/// Captures kept per bar: the one shown, and the one the page may still be loading.
const KEEP: usize = 2;

/// The captures folder and the files each bar has there.
pub struct Behind {
    dir: PathBuf,
    next: u64,
    files: HashMap<u32, VecDeque<PathBuf>>,
}

impl Behind {
    /// `dir` is emptied: captures never outlive the run that took them.
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("no folder for liquid glass captures: {e}");
        }
        Self {
            dir,
            next: 0,
            files: HashMap::new(),
        }
    }

    /// Copies `rect` (screen px) as it looks without our `windows`, and writes it for `bar`.
    /// Returns the file's name in the folder.
    pub fn capture(&mut self, bar: u32, windows: &[HWND], rect: Rect) -> Result<String> {
        let start = std::time::Instant::now();
        let pixels = copy_screen(windows, rect)?;
        let copied = start.elapsed();
        self.next += 1;
        let name = format!("{bar}-{}.bmp", self.next);
        let path = self.dir.join(&name);
        std::fs::write(&path, bmp(rect.width, rect.height, &pixels))
            .with_context(|| format!("writing {}", path.display()))?;
        if !self.files.contains_key(&bar) {
            log::info!("liquid glass: first capture for bar {bar}, {rect:?}");
        }
        log::debug!(
            "liquid glass: captured {}x{} for bar {bar} in {copied:?} (+{:?} to write)",
            rect.width,
            rect.height,
            start.elapsed() - copied
        );
        let kept = self.files.entry(bar).or_default();
        kept.push_back(path);
        while kept.len() > KEEP {
            if let Some(old) = kept.pop_front() {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(name)
    }
}

/// The screen's pixels in `rect` (BGRA, top-down), with `windows` left out.
fn copy_screen(windows: &[HWND], rect: Rect) -> Result<Vec<u8>> {
    let Rect {
        x,
        y,
        width,
        height,
    } = rect;
    if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > MAX_PIXELS {
        bail!("can't capture a {width}×{height} area");
    }
    let copied = (|| {
        for &hwnd in windows {
            // SAFETY: our own windows; restored below whatever the copy does.
            unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
                .context("excluding bar windows from liquid capture")?;
        }
        // SAFETY: apply exclusion before copying, so a lens cannot capture its own shadows.
        unsafe { DwmFlush() }.context("applying capture exclusion")?;
        blit(x, y, width, height)
    })();
    for &hwnd in windows {
        // SAFETY: as above.
        let _ = unsafe { SetWindowDisplayAffinity(hwnd, WDA_NONE) };
    }
    copied
}

fn blit(x: i32, y: i32, width: i32, height: i32) -> Result<Vec<u8>> {
    // SAFETY: GDI objects created, selected and released here; `bits` points into the DIB
    // section, valid until it's deleted, and is read only before that.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            bail!("no screen DC");
        }
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .context("CreateDIBSection")
            .and_then(|bitmap| {
                let old = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    x,
                    y,
                    SRCCOPY | CAPTUREBLT,
                )
                .context("BitBlt")
                .map(|()| {
                    std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize)
                        .to_vec()
                });
                SelectObject(memory, old);
                let _ = DeleteObject(bitmap.into());
                copied
            });
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// A top-down 24-bit BMP of BGRA `pixels` (alpha dropped: GDI leaves it undefined).
pub fn bmp(width: i32, height: i32, pixels: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&(-height).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for line in pixels.chunks_exact(w * 4).take(h) {
        for px in line.as_chunks::<4>().0 {
            out.extend_from_slice(&px[..3]);
        }
        out.resize(out.len() + row - w * 3, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_padded_top_down_bmp() {
        // 3×2 BGRA: rows of 9 bytes padded to 12.
        let pixels: Vec<u8> = (0..24).collect();
        let file = bmp(3, 2, &pixels);
        assert_eq!(&file[..2], b"BM");
        assert_eq!(file.len(), 54 + 12 * 2);
        assert_eq!(
            u32::from_le_bytes(file[2..6].try_into().unwrap()) as usize,
            file.len()
        );
        assert_eq!(
            i32::from_le_bytes(file[22..26].try_into().unwrap()),
            -2,
            "top-down"
        );
        assert_eq!(&file[54..60], &[0, 1, 2, 4, 5, 6], "alpha dropped");
        assert_eq!(&file[63..66], &[0, 0, 0], "row padding");
    }
}
