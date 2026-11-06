//! Pictures for cards, strips and stages: decoded at the size they're shown (WIC on a worker
//! thread, so the first frame never waits), a video's still from Windows' thumbnail provider,
//! and a bounded cache that lets go of what isn't shown (the 60 MB budget).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use mypaper_ui::Image;

/// Decoded pixels kept at most (bytes); the least recently used go first.
const BUDGET: usize = 20 << 20;

/// A finished decode, posted back to the UI thread.
pub struct Loaded {
    pub key: String,
    pub image: Option<Arc<Image>>,
}

enum Slot {
    Pending,
    Ready(Arc<Image>, u64),
    Failed,
}

/// A decode for the worker: the cache key, the file, the size wanted.
type Job = (u64, String, PathBuf, (u32, u32));

#[derive(Default)]
pub struct Images {
    slots: HashMap<String, Slot>,
    clock: u64,
    budget: Option<usize>,
    worker: Option<Sender<Job>>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// Goldens decode synchronously, with their own WIC factory.
    sync: Option<mypaper_ui::Ui>,
}

impl Images {
    /// A smaller window can bound its decoded-picture cache independently.
    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = Some(bytes.max(64 << 10));
        self.trim();
    }

    /// Release a page's decoded pixels and skip its queued work; the decoder stays connected.
    pub fn clear_page(&mut self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.slots.clear();
    }

    /// Decodes on a worker thread from now on, waking the UI thread when one is ready.
    pub fn connect(&mut self, inbox: Sender<crate::app::Msg>, thread: u32) {
        let (tx, rx) = channel::<Job>();
        let generation = self.generation.clone();
        std::thread::spawn(move || {
            let Ok(ui) = mypaper_ui::Ui::new() else { return };
            for (page, key, path, max) in rx {
                if page != generation.load(std::sync::atomic::Ordering::Relaxed) { continue; }
                let image = decode(&ui, &path, max).map(Arc::new);
                if page != generation.load(std::sync::atomic::Ordering::Relaxed) { continue; }
                if inbox.send(crate::app::Msg::Image(Loaded { key, image })).is_err() {
                    break;
                }
                crate::wake(thread);
            }
        });
        self.worker = Some(tx);
    }

    /// The picture at `path`, at most `max` pixels; `None` while it's on its way (or broken).
    pub fn get(&mut self, path: &Path, max: (u32, u32)) -> Option<Arc<Image>> {
        // Sizes are bucketed so a resizing window doesn't decode at every pixel.
        let max = (max.0.next_multiple_of(64), max.1.next_multiple_of(64));
        let page = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let key = format!("{page}:{}|{}x{}", path.display(), max.0, max.1);
        self.clock += 1;
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(image, used)) => {
                *used = self.clock;
                return Some(image.clone());
            }
            Some(Slot::Pending | Slot::Failed) => return None,
            None => {}
        }
        if let Some(worker) = &self.worker {
            self.slots.insert(key.clone(), Slot::Pending);
            let _ = worker.send((page, key, path.to_path_buf(), max));
            return None;
        }
        let ui = match &self.sync {
            Some(ui) => ui,
            None => self.sync.insert(mypaper_ui::Ui::new().ok()?),
        };
        let image = decode(ui, path, max).map(Arc::new);
        self.arrived(Loaded { key, image: image.clone() });
        image
    }

    pub fn arrived(&mut self, loaded: Loaded) {
        if !loaded.key.starts_with(&format!("{}:", self.generation.load(std::sync::atomic::Ordering::Relaxed))) { return; }
        self.clock += 1;
        let slot = match loaded.image {
            Some(image) => Slot::Ready(image, self.clock),
            None => Slot::Failed,
        };
        self.slots.insert(loaded.key, slot);
        self.trim();
    }

    /// Drops the least recently used pictures past the budget.
    fn trim(&mut self) {
        let size = |s: &Slot| match s {
            Slot::Ready(i, _) => i.pixels.len(),
            _ => 0,
        };
        let budget = self.budget.unwrap_or(BUDGET);
        let mut total: usize = self.slots.values().map(size).sum();
        if total <= budget {
            return;
        }
        let mut by_age: Vec<(u64, String, usize)> = self
            .slots
            .iter()
            .filter_map(|(k, s)| match s {
                Slot::Ready(i, used) => Some((*used, k.clone(), i.pixels.len())),
                _ => None,
            })
            .collect();
        by_age.sort_unstable();
        for (_, key, bytes) in by_age {
            if total <= budget * 3 / 4 {
                break;
            }
            self.slots.remove(&key);
            total -= bytes;
        }
    }
}

/// A picture with WIC, or a video's still from the shell.
fn decode(ui: &mypaper_ui::Ui, path: &Path, max: (u32, u32)) -> Option<Image> {
    let video = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["mp4", "mkv", "webm", "mov", "m4v"].contains(&e.to_ascii_lowercase().as_str()));
    if video {
        return shell_thumbnail(path, max)
            .inspect_err(|e| log::debug!("thumbnail {}: {e:#}", path.display()))
            .ok();
    }
    let bytes = std::fs::read(path).ok()?;
    ui.decode_image(&bytes, max)
        .inspect_err(|e| log::debug!("decoding {}: {e:#}", path.display()))
        .ok()
}

/// Windows' own thumbnail of a file (what Explorer shows), as premultiplied BGRA.
fn shell_thumbnail(path: &Path, max: (u32, u32)) -> anyhow::Result<Image> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
        GetObjectW, HGDIOBJ, ReleaseDC,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
    };
    // SAFETY: COM on this worker thread (kept for its life); the bitmap is ours to delete; the
    // pixel buffer matches the header.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&windows::core::HSTRING::from(path.as_os_str()), None)?;
        let bitmap = factory.GetImage(
            SIZE {
                cx: max.0 as i32,
                cy: max.1 as i32,
            },
            SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
        )?;
        let mut info = BITMAP::default();
        GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some((&raw mut info).cast()),
        );
        let (w, h) = (info.bmWidth.max(1), info.bmHeight.max(1));
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        let dc = GetDC(None);
        let rows = GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut header, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(bitmap.into());
        anyhow::ensure!(rows > 0, "no thumbnail pixels");
        // Video thumbnails are opaque; some providers leave alpha at 0.
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Ok(Image {
            width: w as u32,
            height: h as u32,
            pixels,
        })
    }
}
