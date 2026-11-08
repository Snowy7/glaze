//! Pictures for cards, strips and stages: decoded at the size they're shown (WIC on a worker
//! thread, so the first frame never waits), a video's still from Windows' thumbnail provider,
//! and a byte-bounded cache that lets go of what isn't shown.

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
    source_size: Option<(u32, u32)>,
    received: Option<Sender<()>>,
}

enum Slot {
    Pending,
    Ready(Arc<Image>, u64, Option<(u32, u32)>),
    Failed,
}

/// A decode for the worker: the cache key, the file, the size wanted.
enum Job {
    Decode(u64, String, PathBuf, (u32, u32)),
    Release,
}

#[derive(Default)]
pub struct Images {
    slots: HashMap<String, Slot>,
    clock: u64,
    frame_start: Option<u64>,
    budget: Option<usize>,
    worker: Option<Sender<Job>>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// Goldens decode synchronously, with their own WIC factory.
    sync: Option<Decoder>,
}

impl Images {
    /// Panel pages keep only stills requested by the frame that was actually presented.
    pub fn begin_frame(&mut self) {
        self.frame_start = Some(self.clock);
    }

    pub fn finish_frame(&mut self) -> bool {
        let Some(start) = self.frame_start.take() else { return false };
        let before = self.slots.len();
        self.slots.retain(|_, slot| !matches!(slot, Slot::Ready(_, used, _) if *used <= start));
        before != self.slots.len()
    }

    /// A smaller window can bound its decoded-picture cache independently.
    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = Some(bytes.max(64 << 10));
        self.trim();
    }

    /// Release a page's decoded pixels and skip its queued work; the decoder stays connected.
    pub fn clear_page(&mut self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.slots.clear();
        self.sync = None;
        if let Some(worker) = &self.worker {
            let _ = worker.send(Job::Release);
        }
    }

    /// Decodes on a worker thread from now on, waking the UI thread when one is ready.
    pub fn connect(&mut self, inbox: Sender<crate::app::Msg>, thread: u32) {
        let (tx, rx) = channel::<Job>();
        let generation = self.generation.clone();
        let release_decoders = self.budget.is_some();
        std::thread::spawn(move || {
            let mut decoder = None;
            for job in rx {
                let Job::Decode(page, key, path, max) = job else {
                    decoder = None;
                    if release_decoders {
                        crate::panel::optimize_heap();
                    }
                    continue;
                };
                if page != generation.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let decoder = match &decoder {
                    Some(decoder) => decoder,
                    None => {
                        let Ok(ready) = Decoder::new() else { continue };
                        decoder.insert(ready)
                    }
                };
                let decoded = decode(decoder, &path, max);
                let source_size = decoded.as_ref().map(|(_, size)| *size);
                let image = decoded.map(|(image, _)| Arc::new(image));
                // The ready pixels outlive the decoder. Return its freed native buffers
                // before the UI allocates a snapshot canvas or another specimen.
                if release_decoders {
                    crate::panel::optimize_heap();
                }
                if page != generation.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                // A compact panel consumes one decode before the worker starts another.
                // Otherwise ready rasters pile up while its UI is painting gallery faces.
                let (received, wait) = if release_decoders {
                    let (tx, rx) = channel();
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
                if inbox
                    .send(crate::app::Msg::Image(Loaded {
                        key,
                        image,
                        source_size,
                        received,
                    }))
                    .is_err()
                {
                    break;
                }
                crate::wake(thread);
                if let Some(wait) = wait {
                    let _ = wait.recv();
                }
            }
        });
        self.worker = Some(tx);
    }

    /// The picture at `path`, at most `max` pixels; `None` while it's on its way (or broken).
    pub fn get(&mut self, path: &Path, max: (u32, u32)) -> Option<Arc<Image>> {
        // Sizes are bucketed so a resizing window doesn't decode at every pixel.
        let mut max = (
            max.0.max(1).saturating_add(63) / 64 * 64,
            max.1.max(1).saturating_add(63) / 64 * 64,
        );
        // An oversized request must not evict itself and start decoding again every frame.
        // Bound transient decode buffers too, preserving the requested aspect ratio.
        let pixels = u64::from(max.0) * u64::from(max.1);
        let limit = (self.budget.unwrap_or(BUDGET) / 4) as u64;
        if pixels > limit {
            let scale = (limit as f64 / pixels as f64).sqrt();
            max = (
                (max.0 as f64 * scale).floor().max(1.0) as u32,
                (max.1 as f64 * scale).floor().max(1.0) as u32,
            );
        }
        let page = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let key = format!("{page}:{}|{}x{}", path.display(), max.0, max.1);
        self.clock += 1;
        match self.slots.get_mut(&key) {
            Some(Slot::Ready(image, used, _)) => {
                *used = self.clock;
                return Some(image.clone());
            }
            Some(Slot::Pending) => return self.previous_size(path),
            Some(Slot::Failed) => return None,
            None => {}
        }
        if let Some(worker) = &self.worker {
            self.slots.insert(key.clone(), Slot::Pending);
            let _ = worker.send(Job::Decode(page, key, path.to_path_buf(), max));
            return self.previous_size(path);
        }
        let decoder = match &self.sync {
            Some(decoder) => decoder,
            None => self.sync.insert(Decoder::new().ok()?),
        };
        let decoded = decode(decoder, path, max);
        let source_size = decoded.as_ref().map(|(_, size)| *size);
        let image = decoded.map(|(image, _)| Arc::new(image));
        self.arrived(Loaded {
            key,
            image: image.clone(),
            source_size,
            received: None,
        });
        image
    }

    /// Keep the last decoded size visible while its replacement is on the worker.
    fn previous_size(&mut self, path: &Path) -> Option<Arc<Image>> {
        let page = self.generation.load(std::sync::atomic::Ordering::Relaxed);
        let prefix = format!("{page}:{}|", path.display());
        let (_, slot) = self.slots.iter_mut()
            .filter(|(key, slot)| key.starts_with(&prefix) && matches!(slot, Slot::Ready(..)))
            .max_by_key(|(_, slot)| match slot { Slot::Ready(_, used, _) => *used, _ => 0 })?;
        let Slot::Ready(image, used, _) = slot else { return None };
        *used = self.clock;
        Some(image.clone())
    }

    pub fn arrived(&mut self, loaded: Loaded) {
        if let Some(received) = loaded.received {
            let _ = received.send(());
        }
        if !loaded.key.starts_with(&format!(
            "{}:",
            self.generation.load(std::sync::atomic::Ordering::Relaxed)
        )) {
            return;
        }
        self.clock += 1;
        let slot = match loaded.image {
            Some(image) => Slot::Ready(image, self.clock, loaded.source_size),
            None => Slot::Failed,
        };
        self.slots.insert(loaded.key, slot);
        self.trim();
    }

    /// Native still dimensions, retained alongside its reduced pixels for Center's 1:1 crop.
    pub fn source_size(&self, image: &Arc<Image>) -> Option<(u32, u32)> {
        self.slots.values().find_map(|slot| match slot {
            Slot::Ready(ready, _, size) if Arc::ptr_eq(ready, image) => *size,
            _ => None,
        })
    }

    /// Drops the least recently used pictures past the budget.
    fn trim(&mut self) {
        let size = |s: &Slot| match s {
            Slot::Ready(i, _, _) => i.pixels.len(),
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
                Slot::Ready(i, used, _) => Some((*used, k.clone(), i.pixels.len())),
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
fn decode(decoder: &Decoder, path: &Path, max: (u32, u32)) -> Option<(Image, (u32, u32))> {
    let video = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["mp4", "mkv", "webm", "mov", "m4v"].contains(&e.to_ascii_lowercase().as_str())
    });
    if video {
        return shell_thumbnail(path, max)
            .inspect_err(|e| log::debug!("thumbnail {}: {e:#}", path.display()))
            .ok()
            .map(|image| {
                let size = (image.width, image.height);
                (image, size)
            });
    }
    decoder
        .image(path, max)
        .inspect_err(|e| log::debug!("decoding {}: {e:#}", path.display()))
        .ok()
}

/// The decode worker needs WIC alone. A complete Ui also keeps native font and drawing
/// factories alive, and reading through a WIC file stream avoids two compressed-file copies.
struct Decoder {
    factory: windows::Win32::Graphics::Imaging::IWICImagingFactory,
    _apartment: Apartment,
}

/// Balance each successful initialization after this thread's decoder objects are dropped.
struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: called and dropped on the image worker (or the synchronous UI thread).
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: each successful CoInitializeEx on this thread owns one matching call.
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

impl Decoder {
    fn new() -> anyhow::Result<Self> {
        use windows::Win32::{
            Graphics::Imaging::CLSID_WICImagingFactory,
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        };
        // SAFETY: all decoder objects stay on this thread. An existing COM apartment is kept.
        unsafe {
            let apartment = Apartment::new();
            Ok(Self {
                factory: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
                _apartment: apartment,
            })
        }
    }

    fn image(&self, path: &Path, max: (u32, u32)) -> anyhow::Result<(Image, (u32, u32))> {
        use windows::Win32::{
            Foundation::GENERIC_READ,
            Graphics::Imaging::{
                GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone,
                WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
                WICDecodeMetadataCacheOnLoad,
            },
        };
        // SAFETY: valid local COM objects, bounded dimensions and a correctly sized BGRA buffer.
        unsafe {
            let decoder = self.factory.CreateDecoderFromFilename(
                &windows::core::HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnLoad,
            )?;
            let frame = decoder.GetFrame(0)?;
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&mut width, &mut height)?;
            let source_size = (width, height);
            anyhow::ensure!(
                width > 0 && height > 0 && width <= 32768 && height <= 32768,
                "invalid image dimensions"
            );
            let scale = (max.0 as f64 / width as f64)
                .min(max.1 as f64 / height as f64)
                .min(1.0);
            let width = (width as f64 * scale).round().max(1.0) as u32;
            let height = (height as f64 * scale).round().max(1.0) as u32;
            let scaler = self.factory.CreateBitmapScaler()?;
            scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant)?;
            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let mut pixels = vec![0; width as usize * height as usize * 4];
            converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
            Ok((
                Image {
                    width,
                    height,
                    pixels,
                },
                source_size,
            ))
        }
    }
}

/// Windows' own thumbnail of a file (what Explorer shows), as premultiplied BGRA.
fn shell_thumbnail(path: &Path, max: (u32, u32)) -> anyhow::Result<Image> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
    };
    // SAFETY: COM on this worker thread (kept for its life); the bitmap is ours to delete; the
    // pixel buffer matches the header.
    unsafe {
        let _apartment = Apartment::new();
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&windows::core::HSTRING::from(path.as_os_str()), None)?;
        let bitmap = factory.GetImage(
            SIZE {
                cx: max.0 as i32,
                cy: max.1 as i32,
            },
            SIIGBF_THUMBNAILONLY,
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
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_frames_release_hidden_stills_and_keep_visible_source_metadata() {
        let mut images = Images::default();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon-32.png");
        let hidden = images.get(&path, (32, 32)).unwrap();
        images.begin_frame();
        let visible = images.get(&path, (128, 128)).unwrap();
        assert!(images.finish_frame());
        assert_eq!(images.source_size(&hidden), None);
        assert_eq!(images.source_size(&visible), Some((32, 32)));
        images.begin_frame();
        assert!(images.finish_frame());
        assert_eq!(images.source_size(&visible), None);
    }

    #[test]
    fn resizing_keeps_the_last_picture_visible_until_the_worker_replaces_it() {
        let mut images = Images::default();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon-32.png");
        let old = images.get(&path, (16, 16)).unwrap();
        let (tx, rx) = channel();
        images.worker = Some(tx);
        images.begin_frame();
        let during = images.get(&path, (128, 128)).unwrap();
        assert!(Arc::ptr_eq(&old, &during));
        assert!(!images.finish_frame());
        let Job::Decode(_, key, _, _) = rx.try_recv().unwrap() else { panic!("expected resize decode") };
        let replacement = Arc::new(Image { width: 128, height: 128, pixels: vec![255; 128 * 128 * 4] });
        images.arrived(Loaded { key, image: Some(replacement.clone()), source_size: Some((32, 32)), received: None });
        assert!(Arc::ptr_eq(&replacement, &images.get(&path, (128, 128)).unwrap()));
    }

    #[test]
    fn the_wic_only_decoder_preserves_the_ui_decoders_pixels() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon-32.png");
        let bytes = std::fs::read(&path).unwrap();
        let ui = mypaper_ui::Ui::new().unwrap();
        for size in [(16, 16), (48, 48)] {
            let expected = ui.decode_image(&bytes, size).unwrap();
            assert_eq!(
                Decoder::new().unwrap().image(&path, size).unwrap().0,
                expected
            );
        }
    }

    #[test]
    fn page_release_rejects_old_worker_results_and_enforces_a_byte_budget() {
        let mut images = Images::default();
        images.set_budget(64 << 10);
        let loaded = |key: &str, bytes: u32| Loaded {
            key: key.into(),
            source_size: None,
            received: None,
            image: Some(Arc::new(Image {
                width: 128,
                height: bytes / 512,
                pixels: vec![0; bytes as usize],
            })),
        };
        images.arrived(loaded("0:first", 48 << 10));
        images.arrived(loaded("0:second", 48 << 10));
        assert_eq!(
            images.slots.len(),
            1,
            "bytes, not the number of images, bound the cache"
        );
        images.clear_page();
        images.arrived(loaded("0:late", 48 << 10));
        assert!(
            images.slots.is_empty(),
            "old-page work cannot retain decoded pixels"
        );
        images.arrived(loaded("1:current", 48 << 10));
        assert_eq!(images.slots.len(), 1);
    }

    #[test]
    fn stale_page_results_release_the_worker_and_still_metadata_survives_reduction() {
        let mut images = Images::default();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon-32.png");
        let image = images.get(&path, (16, 16)).unwrap();
        assert_eq!(images.source_size(&image), Some((32, 32)));
        images.clear_page();
        assert_eq!(images.source_size(&image), None);
        let (tx, rx) = channel();
        images.arrived(Loaded {
            key: "0:stale".into(),
            image: Some(image),
            source_size: Some((32, 32)),
            received: Some(tx),
        });
        rx.try_recv()
            .expect("discarded decode must unblock the next job");
        assert!(images.slots.is_empty());
    }
}
