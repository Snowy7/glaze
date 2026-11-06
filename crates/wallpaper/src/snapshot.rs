//! Static snapshot: a still of the theme as the *Windows* desktop wallpaper, so Windows shows the
//! theme's look even before mypaper starts at login.
//!
//! Once per theme load, right after the first presented frame, the engine calls
//! [`Snapshot::capture`]. That draws the source once more per screen into an offscreen texture at
//! the screen's size and copies it into a readback buffer: the only main-thread work, no waiting
//! for the GPU. A worker thread (own COM apartment, below-normal priority) then waits for the
//! copy, encodes a JPEG (quality 90) through WIC, writes it atomically and sets it as that
//! monitor's wallpaper through [`mypaper_desktop::desktop_wallpaper`], which journals the
//! user's original so it can be put back.
//!
//! Files are named `<theme>-<screen>-<key>.jpg` under `%LOCALAPPDATA%\mypaper\snapshot`. The key
//! hashes the theme folder, the name, size and modification time of every file in it, the screen
//! index and its size, so any change gives a new name (Windows caches a transcoded copy per path)
//! and an unchanged theme is skipped: no draw, no encode, and no `SetWallpaper` when the monitor
//! already shows the file. Files no monitor shows any more are deleted.
//!
//! Policy:
//! - desktop mode only (never `--windowed`), and only while `static_snapshot` is on; turning it
//!   off restores the user's wallpaper at once, and so does a theme that can't be snapshotted
//!   (`web` and `app` sources draw in windows of their own, wgpu can't read them back);
//! - an animated theme's snapshot is its first frame. A frame that is entirely black (a video
//!   whose first frame is not decoded yet) is never saved: the engine asks again a little later;
//! - a slideshow or Spotlight background of the user's is left alone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use mypaper_desktop::Rect;
use mypaper_desktop::desktop_wallpaper::{self, Apartment, SNAPSHOT_DIR, Wallpapers};
use windows::Win32::Foundation::{GENERIC_WRITE, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::PROPBAG2;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_NULL};
use windows::core::{HSTRING, PWSTR, w};

use crate::gpu::Gpu;
use crate::sources::{Frame, Source};

/// JPEG quality, 0 to 1.
const QUALITY: f32 = 0.9;
/// How soon, and how often, a black first frame is tried again.
const RETRY_EVERY: Duration = Duration::from_millis(300);
const MAX_RETRIES: u32 = 10;
/// Rows packed and handed to the encoder at a time.
const CHUNK_ROWS: usize = 64;
/// A theme folder is fingerprinted by looking at no more than this many entries, this deep.
const MAX_ENTRIES: usize = 2000;
const MAX_DEPTH: usize = 4;

/// One screen to snapshot.
pub struct Target {
    /// The monitor's rect, to match Windows' wallpaper monitors by.
    pub rect: Rect,
    /// The screen's size in pixels (the image is exactly this big).
    pub size: (u32, u32),
    /// The cursor relative to the screen, as the first frame saw it.
    pub mouse: [f32; 2],
}

/// The engine's side of the feature: decides when to capture and hands the work to a thread.
pub struct Snapshot {
    /// `%LOCALAPPDATA%\mypaper`; `None` if it can't be found (the feature is off).
    base: Option<PathBuf>,
    enabled: bool,
    /// The current theme loaded (it isn't the "broken" screen).
    usable: bool,
    /// The current theme has not been captured yet.
    due: bool,
    /// Not before this (a retry after a black frame).
    not_before: Option<Instant>,
    retries: u32,
    /// Set by the worker when it met an all-black frame.
    blank: Arc<AtomicBool>,
    /// Set by the worker once every screen shows the current theme's still. A new one per theme
    /// load, so a late answer about an older theme can't count.
    shown: Arc<AtomicBool>,
    jobs: Option<mpsc::Sender<Job>>,
}

impl Snapshot {
    /// Off until [`set_enabled`](Self::set_enabled); files go under `%LOCALAPPDATA%\mypaper`.
    pub fn new() -> Self {
        let base = desktop_wallpaper::local_dir()
            .inspect_err(|e| log::warn!("no static snapshot: {e:#}"))
            .ok();
        Self::in_dir(base)
    }

    fn in_dir(base: Option<PathBuf>) -> Self {
        Self {
            base,
            enabled: false,
            usable: false,
            due: false,
            not_before: None,
            retries: 0,
            blank: Arc::default(),
            shown: Arc::default(),
            jobs: None,
        }
    }

    /// A theme was just loaded (`usable`: it didn't fail to the "broken" screen). Its first
    /// frame is due to be captured, if enabled.
    pub fn theme_loaded(&mut self, usable: bool) {
        self.usable = usable;
        self.due = usable;
        self.not_before = None;
        self.retries = 0;
        self.blank.store(false, Ordering::Relaxed);
        self.shown = Arc::default();
    }

    /// Does Windows show the current theme's still on every screen? Then a screen without our
    /// window (hidden, or without a frame yet) shows the theme, not the user's own wallpaper.
    pub fn showing(&self) -> bool {
        self.enabled && self.shown.load(Ordering::Relaxed)
    }

    /// Turns the feature on or off. Returns whether it was just turned on, so the engine draws
    /// a frame to capture. Turning it off restores the user's wallpaper (also when it was never
    /// on in this run, e.g. it was turned off while mypaper wasn't running).
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        let Some(base) = self.base.clone() else {
            return false;
        };
        if enabled {
            let turned_on = !self.enabled;
            self.enabled = true;
            if turned_on {
                self.due = self.usable;
            }
            return turned_on;
        }
        self.enabled = false;
        self.shown = Arc::default();
        if desktop_wallpaper::pending_in(&base) {
            self.send(Job::Restore);
        }
        false
    }

    /// Is the current frame to be captured? Call after a frame was presented.
    pub fn due(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        let now = Instant::now();
        if self.blank.swap(false, Ordering::Relaxed) && self.retries < MAX_RETRIES {
            self.retries += 1;
            self.not_before = Some(now + RETRY_EVERY);
            self.due = true;
        }
        self.due && self.not_before.is_none_or(|t| now >= t)
    }

    /// Draws `source` once more per target, offscreen, and queues the stills for the worker.
    /// `theme` is the theme's folder.
    pub fn capture(
        &mut self,
        gpu: &Gpu,
        source: &mut dyn Source,
        theme: &Path,
        targets: &[Target],
        clock: Duration,
        frame: u32,
    ) {
        self.due = false;
        let Some(base) = self.base.clone() else {
            return;
        };
        let started = Instant::now();
        let dir = base.join(SNAPSHOT_DIR);
        let fingerprint = fingerprint(theme);

        let mut stills = Vec::new();
        let mut native = false;
        for (index, target) in targets.iter().enumerate() {
            if source.native(index) {
                native = true;
                continue;
            }
            let path = dir.join(file_name(theme, fingerprint, index, target.size));
            let content = if path.exists() {
                Content::Existing
            } else {
                match read(gpu, source, index, target, clock, frame) {
                    Ok(readback) => Content::Pixels(readback),
                    Err(e) => {
                        log::warn!("no snapshot of screen {index}: {e:#}");
                        continue;
                    }
                }
            };
            stills.push(Still {
                rect: target.rect,
                path,
                content,
            });
        }

        if stills.is_empty() {
            if native {
                log::info!(
                    "no snapshot: this theme draws in windows of its own, which can't be read back"
                );
                if desktop_wallpaper::pending_in(&base) {
                    self.send(Job::Restore);
                }
            }
            return;
        }
        let captured = stills
            .iter()
            .filter(|s| matches!(s.content, Content::Pixels(_)))
            .count();
        log::info!(
            "snapshot: {captured} of {} screens drawn in {:.1?} on the main thread",
            stills.len(),
            started.elapsed()
        );
        self.send(Job::Apply(stills, self.shown.clone()));
    }

    /// Queues `job`, starting the worker on first use (and again if it died).
    fn send(&mut self, mut job: Job) {
        let Some(base) = &self.base else { return };
        for _ in 0..2 {
            if self.jobs.is_none() {
                self.jobs = spawn(base.clone(), self.blank.clone());
            }
            let Some(jobs) = &self.jobs else { return };
            match jobs.send(job) {
                Ok(()) => return,
                Err(mpsc::SendError(back)) => {
                    job = back;
                    self.jobs = None;
                }
            }
        }
    }
}

/// What the worker is asked to do. Only the latest queued job matters.
enum Job {
    /// Make these the Windows wallpapers, and delete our stills nobody shows. The flag is set
    /// when every one of them is on its monitor.
    Apply(Vec<Still>, Arc<AtomicBool>),
    /// Put the user's wallpapers back.
    Restore,
}

/// A wallpaper for the monitor at `rect`.
struct Still {
    rect: Rect,
    path: PathBuf,
    content: Content,
}

enum Content {
    /// Drawn, copy to the readback buffer submitted; not yet encoded.
    Pixels(Readback),
    /// The file is there from an earlier run.
    Existing,
}

/// A screen's pixels, on their way back from the GPU.
struct Readback {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    size: (u32, u32),
    /// Bytes per buffer row: the width in bytes, padded to wgpu's alignment.
    row_bytes: usize,
    /// Pixels are B, G, R, A (else R, G, B, A).
    bgra: bool,
}

/// Draws `source` for screen `index` into a new texture of the screen's size and queues its copy
/// to a buffer. Does not wait for the GPU.
fn read(
    gpu: &Gpu,
    source: &mut dyn Source,
    index: usize,
    target: &Target,
    clock: Duration,
    frame: u32,
) -> Result<Readback> {
    let bgra = match gpu.format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
        other => bail!("cannot read back {other:?}"),
    };
    let (width, height) = target.size;
    let limit = gpu.device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width.max(height) > limit {
        bail!("a {width}x{height} screen can't be drawn offscreen");
    }
    let texture = gpu.texture(
        "snapshot",
        target.size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let row_bytes = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot readback"),
        size: u64::from(row_bytes) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let view = texture.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    source.render(
        gpu,
        Frame {
            screen: index,
            size: target.size,
            mouse: target.mouse,
            clock,
            frame,
            view: &view,
            encoder: &mut encoder,
        },
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    Ok(Readback {
        device: gpu.device.clone(),
        buffer,
        size: target.size,
        row_bytes: row_bytes as usize,
        bgra,
    })
}

impl Readback {
    /// Waits for the GPU, then runs `f` on the pixels (`row_bytes` apart).
    fn with_pixels<T>(&self, f: impl FnOnce(&[u8]) -> T) -> Result<T> {
        let (sender, receiver) = mpsc::channel();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let mapped = self.buffer.slice(..).get_mapped_range()?;
        let value = f(&mapped);
        drop(mapped);
        self.buffer.unmap();
        Ok(value)
    }

    /// Is there nothing but black (alpha ignored) in `data`?
    fn is_black(&self, data: &[u8]) -> bool {
        let width = self.size.0 as usize * 4;
        data.chunks_exact(self.row_bytes).all(|row| {
            row[..width]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [0, 0, 0])
        })
    }
}

/// Starts the worker thread. `None` if the thread can't be started.
fn spawn(base: PathBuf, blank: Arc<AtomicBool>) -> Option<mpsc::Sender<Job>> {
    let (sender, receiver) = mpsc::channel();
    // SAFETY: the calling engine thread owns the message loop.
    let thread = unsafe { GetCurrentThreadId() };
    thread::Builder::new()
        .name("snapshot".into())
        .spawn(move || work(&base, &receiver, &blank, thread))
        .inspect_err(|e| log::warn!("no static snapshot: cannot start its thread: {e}"))
        .ok()?;
    Some(sender)
}

/// The worker: encodes, writes and sets, one job at a time. Ends when the engine is dropped.
fn work(base: &Path, jobs: &mpsc::Receiver<Job>, blank: &AtomicBool, thread: u32) {
    // SAFETY: lowers the priority of this very thread.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    let _apartment = match Apartment::enter() {
        Ok(apartment) => apartment,
        Err(e) => return log::warn!("no static snapshot: {e:#}"),
    };
    while let Ok(mut job) = jobs.recv() {
        // Theme switches in a burst: only the last one matters.
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        let started = Instant::now();
        let done = match job {
            Job::Apply(stills, shown) => {
                apply(base, stills, blank).map(|all| shown.store(all, Ordering::Relaxed))
            }
            Job::Restore => desktop_wallpaper::restore_in(base).map(|restored| {
                if restored {
                    log::info!("snapshot: the original wallpaper is back");
                }
            }),
        };
        match done {
            Ok(()) => log::debug!("snapshot job took {:.1?}", started.elapsed()),
            Err(e) => log::warn!("snapshot: {e:#}"),
        }
        // A held engine may be asleep indefinitely. Re-evaluate the game still/release as
        // soon as its snapshot is installed, without adding a readiness timer.
        // SAFETY: a no-op message; a gone engine thread simply rejects it.
        let _ = unsafe { PostThreadMessageW(thread, WM_NULL, WPARAM(0), LPARAM(0)) };
    }
}

/// Writes the stills that are only pixels so far and sets each on its monitor, unless it is
/// already showing it. Then deletes the stills no monitor shows. Returns whether every still is
/// on its monitor now.
fn apply(base: &Path, stills: Vec<Still>, blank: &AtomicBool) -> Result<bool> {
    let mut wallpapers = Wallpapers::native_in(base)?;
    let slots = wallpapers.monitors()?;
    let factory = wic_factory()?;
    let (mut waited, mut encoded, mut set) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut all = true;

    for still in stills {
        if let Content::Pixels(readback) = &still.content {
            let started = Instant::now();
            let encode_time = readback.with_pixels(|data| -> Result<Option<Duration>> {
                if readback.is_black(data) {
                    return Ok(None);
                }
                let started = Instant::now();
                write_jpeg(&factory, readback, data, &still.path)?;
                Ok(Some(started.elapsed()))
            })??;
            let Some(encode_time) = encode_time else {
                log::info!("snapshot: the first frame is black; trying again shortly");
                blank.store(true, Ordering::Relaxed);
                all = false;
                continue;
            };
            encoded += encode_time;
            waited += started.elapsed().saturating_sub(encode_time);
        }

        let Some(slot) = slots.iter().find(|slot| slot.rect == still.rect) else {
            log::warn!(
                "snapshot: Windows has no wallpaper monitor at {:?}",
                still.rect
            );
            all = false;
            continue;
        };
        if slot.shows(&still.path) {
            continue;
        }
        let started = Instant::now();
        wallpapers.apply(&slot.id, &still.path)?;
        set += started.elapsed();
    }
    wallpapers.sweep()?;
    log::info!(
        "snapshot (worker thread): GPU wait {waited:.1?}, encode {encoded:.1?}, set {set:.1?}"
    );
    Ok(all)
}

fn wic_factory() -> Result<IWICImagingFactory> {
    // SAFETY: COM is initialized on this thread; the CLSID names the WIC factory.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .context("creating the WIC imaging factory")
}

/// Encodes `data` (the readback's mapped pixels) as a JPEG at `path`: written next to it first,
/// then renamed over it, so a reader never sees half a file.
fn write_jpeg(
    factory: &IWICImagingFactory,
    readback: &Readback,
    data: &[u8],
    path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("snapshot path has no parent")?)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let written = encode(factory, readback, data, &temp);
    if written.is_ok() {
        std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
    } else {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

fn encode(
    factory: &IWICImagingFactory,
    readback: &Readback,
    data: &[u8],
    path: &Path,
) -> Result<()> {
    let (width, height) = readback.size;
    // SAFETY: COM calls on objects made by this thread's factory. Each `WritePixels` gets a
    // buffer of exactly `rows * width * 3` bytes. The file is closed when the objects drop at the
    // end of this function, before the caller renames it.
    unsafe {
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let (mut frame, mut options) = (None, None);
        encoder.CreateNewFrame(&mut frame, &mut options)?;
        let frame = frame.context("the JPEG encoder made no frame")?;
        let options = options.context("the JPEG encoder has no options")?;
        let quality = PROPBAG2 {
            pstrName: PWSTR(w!("ImageQuality").as_ptr().cast_mut()),
            ..Default::default()
        };
        options.Write(1, &quality, &VARIANT::from(QUALITY))?;
        frame.Initialize(&options)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format)?;
        if format != GUID_WICPixelFormat24bppBGR {
            bail!("the JPEG encoder wants another pixel format");
        }

        let width = width as usize;
        let mut packed = Vec::with_capacity(CHUNK_ROWS * width * 3);
        for rows in data.chunks(CHUNK_ROWS * readback.row_bytes) {
            packed.clear();
            for row in rows.chunks_exact(readback.row_bytes) {
                for px in row[..width * 4].as_chunks::<4>().0 {
                    packed.extend(if readback.bgra {
                        [px[0], px[1], px[2]]
                    } else {
                        [px[2], px[1], px[0]]
                    });
                }
            }
            let lines = packed.len() / (width * 3);
            frame.WritePixels(lines as u32, (width * 3) as u32, &packed)?;
        }
        frame.Commit()?;
        encoder.Commit()?;
    }
    Ok(())
}

/// Names the file for screen `index`: readable (the theme's folder name) and unique per
/// everything that changes the picture (see [`fingerprint`] and the module doc).
fn file_name(theme: &Path, fingerprint: u64, index: usize, (width, height): (u32, u32)) -> String {
    let slug: String = theme
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    let mut key = fnv(FNV_OFFSET, &fingerprint.to_le_bytes());
    for part in [index as u64, width.into(), height.into()] {
        key = fnv(key, &part.to_le_bytes());
    }
    format!("{slug}-{index}-{key:016x}.jpg")
}

/// A hash of the theme folder's path and of the name, size and modification time of every file
/// in it (at most [`MAX_ENTRIES`], [`MAX_DEPTH`] levels down): metadata only, nothing is read.
fn fingerprint(theme: &Path) -> u64 {
    let mut hash = fnv(FNV_OFFSET, theme.as_os_str().as_encoded_bytes());
    let mut seen = 0;
    walk(theme, 0, &mut seen, &mut hash);
    hash
}

fn walk(dir: &Path, depth: usize, seen: &mut usize, hash: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        *seen += 1;
        if *seen > MAX_ENTRIES {
            return;
        }
        *hash = fnv(*hash, entry.file_name().as_encoded_bytes());
        // Not `entry.metadata()`: NTFS can serve a file's size and time stale from its directory.
        let Ok(meta) = std::fs::metadata(entry.path()) else {
            continue;
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        *hash = fnv(*hash, &meta.len().to_le_bytes());
        *hash = fnv(*hash, &modified.to_le_bytes());
        if meta.is_dir() && depth < MAX_DEPTH {
            walk(&entry.path(), depth + 1, seen, hash);
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a, folded over `bytes` from `hash`. Stable across Rust versions, unlike `DefaultHasher`,
/// so a file keeps its name from one mypaper build to the next.
fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::SystemTime;

    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::{
        GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICPalette, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };

    use super::*;

    /// A scratch folder, empty.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mypaper-snapshot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Four colors, one per quadrant: top-left, top-right, bottom-left, bottom-right.
    const QUADRANTS: [[u8; 3]; 4] = [[220, 40, 40], [40, 200, 60], [50, 60, 230], [240, 220, 50]];

    /// Stretches a 2x2 picture of [`QUADRANTS`] over the screen.
    struct Quadrants {
        _texture: wgpu::Texture,
        quad: crate::sources::blit::BlitQuad,
    }

    impl Quadrants {
        fn new(gpu: &Gpu) -> Self {
            let bgra: Vec<u8> = QUADRANTS
                .iter()
                .flat_map(|&[r, g, b]| [b, g, r, 255])
                .collect();
            let texture = gpu.texture(
                "quadrants",
                (2, 2),
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureUsages::empty(),
                Some(&bgra),
            );
            let quad = gpu
                .blit
                .quad(&gpu.device, &texture.create_view(&Default::default()));
            Self {
                _texture: texture,
                quad,
            }
        }
    }

    impl Source for Quadrants {
        fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
            self.quad.set(
                &gpu.queue,
                (2, 2),
                frame.size,
                mypaper_theme::Fit::Stretch,
                1.0,
            );
            let mut pass = crate::sources::begin(
                frame.encoder,
                frame.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            self.quad.draw(&gpu.blit, &mut pass);
        }
        fn redraw(&self, _clock: Duration) -> crate::sources::Redraw {
            crate::sources::Redraw::OnChange
        }
    }

    fn target(width: u32, height: u32) -> Target {
        Target {
            rect: Rect {
                x: 0,
                y: 0,
                width: width as i32,
                height: height as i32,
            },
            size: (width, height),
            mouse: [0.0, 0.0],
        }
    }

    /// Decodes a JPEG to its size and straight BGRA pixels.
    fn decode(path: &Path) -> ((u32, u32), Vec<u8>) {
        let factory = wic_factory().unwrap();
        // SAFETY: COM calls on objects made by this thread's factory; `CopyPixels` fills
        // `bgra`, which has exactly `stride * height` bytes.
        unsafe {
            let decoder = factory
                .CreateDecoderFromFilename(
                    &HSTRING::from(path.as_os_str()),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .unwrap();
            let frame = decoder.GetFrame(0).unwrap();
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&mut width, &mut height).unwrap();
            let converter = factory.CreateFormatConverter().unwrap();
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppBGRA,
                    WICBitmapDitherTypeNone,
                    None::<&IWICPalette>,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .unwrap();
            let source: IWICBitmapSource = windows::core::Interface::cast(&converter).unwrap();
            let mut bgra = vec![0u8; width as usize * height as usize * 4];
            source
                .CopyPixels(std::ptr::null(), width * 4, &mut bgra)
                .unwrap();
            ((width, height), bgra)
        }
    }

    #[test]
    fn a_screen_comes_back_as_a_jpeg_of_its_size_with_its_pixels_in_place() {
        let Some(gpu) = Gpu::headless() else { return };
        let mut source = Quadrants::new(&gpu);
        // 101 px wide: not a multiple of wgpu's 64-pixel row alignment, so rows are padded.
        let (width, height) = (101, 70);
        let readback = read(
            &gpu,
            &mut source,
            0,
            &target(width, height),
            Duration::ZERO,
            0,
        )
        .unwrap();
        assert!(readback.row_bytes > width as usize * 4);

        let dir = temp_dir("jpeg");
        let path = dir.join("still.jpg");
        let _apartment = Apartment::enter().unwrap();
        let factory = wic_factory().unwrap();
        let black = readback
            .with_pixels(|data| {
                write_jpeg(&factory, &readback, data, &path).unwrap();
                readback.is_black(data)
            })
            .unwrap();
        assert!(!black);
        assert!(!path.with_extension("jpg.tmp").exists());

        let (size, bgra) = decode(&path);
        assert_eq!(size, (width, height));
        for (quadrant, want) in QUADRANTS.iter().enumerate() {
            let (x, y) = (
                [width / 4, 3 * width / 4][quadrant % 2],
                [height / 4, 3 * height / 4][quadrant / 2],
            );
            let at = ((y * width + x) * 4) as usize;
            let [b, g, r, _] = bgra[at..at + 4].try_into().unwrap();
            assert!(
                [r, g, b]
                    .iter()
                    .zip(want)
                    .all(|(a, b)| a.abs_diff(*b) <= 12),
                "quadrant {quadrant}: got {:?}, want {want:?}",
                [r, g, b]
            );
        }
    }

    #[test]
    fn an_all_black_frame_is_recognised() {
        let Some(gpu) = Gpu::headless() else { return };
        struct Black;
        impl Source for Black {
            fn render(&mut self, _gpu: &Gpu, frame: Frame<'_>) {
                crate::sources::clear(frame.encoder, frame.view, wgpu::Color::BLACK);
            }
            fn redraw(&self, _clock: Duration) -> crate::sources::Redraw {
                crate::sources::Redraw::OnChange
            }
        }
        let readback = read(&gpu, &mut Black, 0, &target(70, 30), Duration::ZERO, 0).unwrap();
        assert!(
            readback
                .with_pixels(|data| readback.is_black(data))
                .unwrap()
        );
    }

    #[test]
    fn the_name_changes_with_whatever_changes_the_picture() {
        let theme = temp_dir("theme").join("My Theme");
        std::fs::create_dir_all(theme.join("assets")).unwrap();
        std::fs::write(theme.join("theme.json"), "{}").unwrap();
        std::fs::write(theme.join("assets").join("a.png"), "1").unwrap();

        let name = |theme: &Path, index, size| file_name(theme, fingerprint(theme), index, size);
        let first = name(&theme, 0, (1920, 1080));
        assert!(
            first.starts_with("My_Theme-0-") && first.ends_with(".jpg"),
            "{first}"
        );
        assert_eq!(first, name(&theme, 0, (1920, 1080)), "stable");
        assert_ne!(first, name(&theme, 1, (1920, 1080)), "per screen");
        assert_ne!(first, name(&theme, 0, (2560, 1440)), "per size");

        std::fs::write(theme.join("assets").join("a.png"), "22").unwrap();
        let edited = name(&theme, 0, (1920, 1080));
        assert_ne!(first, edited, "a file in a subfolder changed");

        // Same size, new modification time: an editor saving the same bytes still counts.
        File::options()
            .write(true)
            .open(theme.join("theme.json"))
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        assert_ne!(edited, name(&theme, 0, (1920, 1080)));
    }

    /// A dev tool rather than a check: draws a theme through the engine's own source, the way a
    /// screen would show it at `seconds` on the engine clock, and writes a JPEG. For theme
    /// previews and tuning scenes without touching the desktop. Any kind wgpu can read back works
    /// (shader, image, slideshow, video, scene); video is drawn until its first frame is decoded.
    ///
    /// ```text
    /// set MYPAPER_STILL=themes\examples\islands;out.jpg;12;2560x1440
    /// cargo test -p mypaper-wallpaper --release still_of_a_theme -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "dev tool: MYPAPER_STILL=<theme>;<out.jpg>[;<seconds>[;<W>x<H>]]"]
    fn still_of_a_theme() {
        let spec = std::env::var("MYPAPER_STILL").expect("set MYPAPER_STILL");
        let parts: Vec<&str> = spec.split(';').collect();
        // Relative paths are from the repo root (tests run in the crate folder).
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (theme_dir, out) = (root.join(parts[0]), root.join(parts[1]));
        let (theme_dir, out) = (theme_dir.as_path(), out.as_path());
        let seconds: f32 = parts.get(2).map_or(0.0, |s| s.parse().unwrap());
        let (width, height) = parts.get(3).map_or((2560, 1440), |s| {
            let (w, h) = s.split_once('x').unwrap();
            (w.parse().unwrap(), h.parse().unwrap())
        });

        let gpu = Gpu::headless().expect("a DX12 GPU");
        let _apartment = Apartment::enter().unwrap();
        let theme = mypaper_theme::LoadedTheme::load(theme_dir).unwrap();
        let mut source = crate::sources::load(&gpu, &theme, &[]).unwrap();
        let mut target = target(width, height);
        target.mouse = [width as f32 * 0.5, height as f32 * 0.5];
        let clock = Duration::from_secs_f32(seconds);
        let started = Instant::now();
        let (readback, pixels) = loop {
            source.update(&gpu, clock);
            let readback = read(&gpu, source.as_mut(), 0, &target, clock, 1).unwrap();
            let pixels = readback.with_pixels(<[u8]>::to_vec).unwrap();
            if !readback.is_black(&pixels) || started.elapsed() > Duration::from_secs(10) {
                break (readback, pixels);
            }
            thread::sleep(Duration::from_millis(50));
        };
        write_jpeg(&wic_factory().unwrap(), &readback, &pixels, out).unwrap();
        eprintln!("wrote {}", out.display());
    }

    /// The real thing, on the real Windows wallpaper: runs the engine's snapshot code on a
    /// shipped theme (stills in a scratch folder, the journal in the same one), checks that
    /// Windows shows them, turns the feature off and checks that the originals are back. Prints
    /// the wallpapers before and after, and what the capture cost.
    ///
    /// `cargo nextest run -p mypaper-wallpaper --run-ignored only real_wallpaper --no-capture`
    #[test]
    #[ignore = "changes the real Windows wallpaper for a moment"]
    fn real_wallpaper_round_trip() {
        /// Puts the exact originals back when dropped, whatever happened.
        struct Originals(Vec<desktop_wallpaper::Slot>, i32);
        impl Drop for Originals {
            fn drop(&mut self) {
                use desktop_wallpaper::Backend;
                let mut native = desktop_wallpaper::Native::new().unwrap();
                for slot in &self.0 {
                    let _ = native.set_wallpaper(&slot.id, &slot.wallpaper);
                }
                let _ = native.set_position(self.1);
            }
        }
        fn current() -> Vec<desktop_wallpaper::Slot> {
            use desktop_wallpaper::Backend;
            desktop_wallpaper::Native::new()
                .unwrap()
                .monitors()
                .unwrap()
        }
        fn print(label: &str, slots: &[desktop_wallpaper::Slot]) {
            for slot in slots {
                eprintln!(
                    "{label}: {} {:?} -> {:?}",
                    slot.id, slot.rect, slot.wallpaper
                );
            }
        }
        fn wait_for(what: &str, done: impl Fn(&[desktop_wallpaper::Slot]) -> bool) -> Duration {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(30) {
                if done(&current()) {
                    return started.elapsed();
                }
                thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out waiting for {what}");
        }

        let Some(gpu) = Gpu::headless() else { return };
        mypaper_desktop::init_logging();
        let _apartment = Apartment::enter().unwrap();

        let before = current();
        print("before", &before);
        let position = {
            use desktop_wallpaper::Backend;
            desktop_wallpaper::Native::new()
                .unwrap()
                .position()
                .unwrap()
        };
        let _guard = Originals(before.clone(), position);

        let base = temp_dir("real");
        let theme_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/examples/aurora");
        let theme = mypaper_theme::LoadedTheme::load(&theme_dir).unwrap();
        let mut source = crate::sources::load(&gpu, &theme, &[]).unwrap();
        let screens = mypaper_desktop::monitors();
        let targets: Vec<Target> = screens
            .iter()
            .map(|&rect| Target {
                rect,
                size: (rect.width as u32, rect.height as u32),
                mouse: [0.0, 0.0],
            })
            .collect();

        let mut snapshot = Snapshot::in_dir(Some(base.clone()));
        snapshot.theme_loaded(true);
        snapshot.set_enabled(true);
        assert!(snapshot.due());
        let started = Instant::now();
        snapshot.capture(
            &gpu,
            source.as_mut(),
            &theme.dir,
            &targets,
            Duration::ZERO,
            1,
        );
        eprintln!("capture on the main thread: {:.1?}", started.elapsed());

        let dir = base.join(SNAPSHOT_DIR);
        let applied = wait_for("the stills to be set", |slots| {
            slots
                .iter()
                .all(|s| Path::new(&s.wallpaper).parent() == Some(dir.as_path()))
        });
        eprintln!("worker: stills set after {applied:.1?} (including the capture above)");
        print("snapshot", &current());
        assert!(desktop_wallpaper::pending_in(&base));

        // Same theme again: everything is there already, nothing is drawn or set.
        snapshot.theme_loaded(true);
        let started = Instant::now();
        snapshot.capture(
            &gpu,
            source.as_mut(),
            &theme.dir,
            &targets,
            Duration::ZERO,
            2,
        );
        eprintln!("capture of an unchanged theme: {:.1?}", started.elapsed());

        snapshot.set_enabled(false);
        let restored = wait_for("the originals to come back", |slots| {
            let back = slots.iter().zip(&before);
            back.into_iter()
                .all(|(now, was)| now.wallpaper == was.wallpaper)
                && !desktop_wallpaper::pending_in(&base)
        });
        eprintln!("worker: originals back after {restored:.1?}");
        let after = current();
        print("after", &after);
        assert_eq!(after, before);
        assert!(!dir.exists(), "our stills are gone");
        let _ = std::fs::remove_dir_all(&base);
    }
}
