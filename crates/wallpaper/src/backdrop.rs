//! Wallpaper glass: publishes a blurred, low-res snapshot of each monitor's
//! wallpaper for the bars and widget shell to use as a frosted-glass backdrop.
//!
//! Only this process owns the wallpaper pixels. For a screen that is due, the
//! engine shrinks its kept frame (the wallpaper as drawn, never with glass) on the
//! GPU through a bilinear pyramid (1/2 .. 1/64) and back up to 1/8, reads that back (without
//! waiting, for an animated source: a later frame collects it), and
//! writes it to `%LOCALAPPDATA%\mypaper\backdrop\<NAME>.bmp` (`NAME` is the
//! device name without `\\.\`, e.g. `DISPLAY1`; `dev` in `--windowed` mode).
//! The file is an uncompressed top-down 24-bit BMP of `round(w/8) x round(h/8)`
//! pixels with the swapchain format's bytes as-is, written via `.bmp.tmp` + rename.
//! Sampling the kept frame needs no full-screen copy: a full-scale source's kept frame
//! is what the swapchain shows, and a scaled one is the render the swapchain upscales.
//! Screens that share one kept frame (`Source::shared`) share one pyramid and readback per
//! publish: the first screen reads it, the others get the same image.
//!
//! A screen is published once on its first drawn frame after each theme load,
//! and then, while the source is animated, every N seconds, but only while
//! `backdrop\request.json` or `widgets-request.json` (`{ "interval": N }`, clamped to
//! 2..=600, default 10) exists. The shortest request wins. Files are checked once per interval on
//! frames drawn anyway: this module never wakes the engine. Nothing is
//! published periodically while held or paused; a change/request publishes one still instead.
//! Native (web/app) screens use their poster for that one-off refresh;
//! whose stale file is deleted on load. Web sources publish their own CapturePreview snapshots;
//! app sources retain the visible CSS fallback.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_desktop::{Rect, monitor_infos};
use mypaper_theme::Fit;

use crate::gpu::{Gpu, KeptFrame};
use crate::sources::{self, blit::BlitQuad};

/// The published image is the screen shrunk by this much.
const OUTPUT_DIVISOR: u32 = 8;
/// Pyramid texture `k` is the screen shrunk by `DIVISORS[k]`. The kept frame is drawn into 0.
const DIVISORS: [u32; 6] = [2, 4, 8, 16, 32, 64];
/// Which pyramid texture is published (`DIVISORS[RESULT] == OUTPUT_DIVISOR`).
const RESULT: usize = 2;
const _: () = assert!(DIVISORS[RESULT] == OUTPUT_DIVISOR);
/// Blit passes after the first as (sampled texture, target texture): halve down to 1/64, then
/// bilinearly double back up to 1/8.
const PASSES: [(usize, usize); 8] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 5),
    (5, 4),
    (4, 3),
    (3, 2),
];

const DEFAULT_INTERVAL: Duration = Duration::from_secs(10);
const MIN_INTERVAL: Duration = Duration::from_secs(2);
const MAX_INTERVAL: Duration = Duration::from_secs(600);
/// Sits next to the images; the bars create it while a bar uses wallpaper glass.
const REQUEST_FILE: &str = "request.json";

/// BITMAPFILEHEADER (14 bytes) + BITMAPINFOHEADER (40 bytes).
const BMP_HEADERS: usize = 54;

/// Publishes every screen's backdrop; owned by the engine.
pub struct Backdrop {
    /// `%LOCALAPPDATA%\mypaper\backdrop`; `None` if that cannot be resolved.
    dir: Option<PathBuf>,
    /// Indexed like the engine's screens.
    screens: Vec<ScreenState>,
    /// The last kept frame read back: (its texture, the engine frame, the screen that read it).
    latest: Option<(wgpu::Texture, u32, usize)>,
}

struct ScreenState {
    /// `DISPLAY1`; `None` for a screen no monitor matched (never published).
    name: Option<String>,
    cadence: Cadence,
    /// Textures and buffers for the screen's current size. Kept between publishes only while
    /// the source is animated (a static one publishes once per load).
    pyramid: Option<Pyramid>,
    /// The frame being published is from an animated source.
    animated: bool,
    /// An animated source's readback, collected on a later frame.
    reading: Option<Reading>,
    /// Setting up the pyramid failed; stay quiet until the next theme load.
    failed: bool,
}

/// What [`Backdrop::encode`] needs to know about a screen's frame.
pub struct Draw<'a> {
    pub screen: usize,
    /// The engine frame, so screens sharing one kept frame read it back once.
    pub frame: u32,
    pub size: (u32, u32),
    /// The wallpaper the source just drew, at its render scale.
    pub kept: &'a KeptFrame,
    /// The engine is held: publish nothing.
    pub held: bool,
    /// The source redraws on its own (`Redraw::Every` / `Redraw::At`).
    pub animated: bool,
}

impl Backdrop {
    /// Names the screens by the monitors they cover (`None`: the dev window),
    /// then [`reset`](Self::reset)s for a freshly started source.
    pub fn new(monitors: Option<&[Rect]>, native: impl Fn(usize) -> bool) -> Self {
        let names = match monitors {
            None => vec![Some("dev".to_owned())],
            Some(rects) => {
                let infos = monitor_infos();
                rects
                    .iter()
                    .map(|rect| {
                        let name = infos
                            .iter()
                            .find(|m| m.rect == *rect)
                            .map(|m| device_name(&m.device).to_owned());
                        if name.is_none() {
                            log::warn!("backdrop: no device name for monitor {rect:?}");
                        }
                        name
                    })
                    .collect()
            }
        };
        let dir = match mypaper_tweaks::palette::local_dir() {
            Ok(local) => Some(local.join("backdrop")),
            Err(e) => {
                log::warn!("no backdrop: {e:#}");
                None
            }
        };
        Self::with(dir, names, native)
    }

    fn with(
        dir: Option<PathBuf>,
        names: Vec<Option<String>>,
        native: impl Fn(usize) -> bool,
    ) -> Self {
        let screens = names
            .into_iter()
            .map(|name| ScreenState {
                name,
                cadence: Cadence::new(),
                pyramid: None,
                animated: false,
                reading: None,
                failed: false,
            })
            .collect();
        let mut backdrop = Self {
            dir,
            screens,
            latest: None,
        };
        backdrop.reset(native);
        backdrop
    }

    /// A new source was loaded: every screen is due on its next drawn frame,
    /// and a native screen's stale image is deleted.
    pub fn reset(&mut self, native: impl Fn(usize) -> bool) {
        for (index, state) in self.screens.iter_mut().enumerate() {
            state.cadence = Cadence::new();
            state.failed = false;
            if let (true, Some(dir), Some(name)) = (native(index), &self.dir, &state.name) {
                remove_stale(&image_path(dir, name));
            }
        }
    }

    /// Drops every pyramid, readback and kept-frame reference (the engine released its drawing
    /// while every screen is covered). The cadence carries on from where it was.
    pub fn release(&mut self) {
        self.latest = None;
        for state in &mut self.screens {
            state.pyramid = None;
            state.reading = None;
        }
    }

    /// A covered change publishes immediately, independent of the animated cadence. Each
    /// screen owns its one-shot readback, so no later frame is needed to collect or share it.
    pub fn refresh_screen(&mut self, screen: usize) {
        self.latest = None;
        if let Some(state) = self.screens.get_mut(screen) {
            state.cadence = Cadence::new();
            state.reading = None;
            state.pyramid = None;
        }
    }

    /// If `draw`'s screen is due, records the copy, the blur and the readback
    /// into `encoder` (after the source drew, before submit) and returns true:
    /// call [`finish`](Self::finish) once the encoder is submitted.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        draw: Draw<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let Some(dir) = &self.dir else { return false };
        let Some(state) = self.screens.get_mut(draw.screen) else {
            return false;
        };
        let Some(name) = state.name.clone() else {
            return false;
        };
        if draw.held || state.failed {
            return false;
        }
        // An animated screen's last readback may still be in flight: pick it up first, and don't
        // record into a buffer that's still mapped.
        if state.reading.is_some() && !state.collect(gpu, dir) {
            return false;
        }
        let request = dir.join(REQUEST_FILE);
        let due = state
            .cadence
            .due(Instant::now(), draw.animated, || read_request(&request));
        if !due {
            return false;
        }
        // Another screen read this very kept frame back in this engine frame: same image.
        if let Some((_, _, reader)) = self.latest.as_ref().filter(|(texture, frame, reader)| {
            *texture == draw.kept.texture && *frame == draw.frame && *reader != draw.screen
        }) {
            let reader = &mut self.screens[*reader];
            match (&mut reader.reading, &reader.name) {
                (Some(reading), _) => reading.also.push(name),
                (None, Some(from)) => copy_image(dir, from, &name),
                (None, None) => {}
            }
            return false;
        }
        self.latest = Some((draw.kept.texture.clone(), draw.frame, draw.screen));
        let state = &mut self.screens[draw.screen];
        if state.pyramid.as_ref().is_none_or(|p| p.size != draw.size) {
            match Pyramid::new(gpu, draw.size) {
                Ok(pyramid) => state.pyramid = Some(pyramid),
                Err(e) => {
                    log::warn!("no backdrop: {e:#}");
                    state.failed = true;
                    return false;
                }
            }
        }
        let Some(pyramid) = &state.pyramid else {
            return false;
        };
        let kept = draw.kept;
        let source = (kept.texture.width(), kept.texture.height());
        kept.quad
            .set(&gpu.queue, source, pyramid.size, Fit::Stretch, 1.0);
        pyramid.blur(gpu, &kept.quad, encoder);
        state.animated = draw.animated;
        true
    }

    /// Reads back what [`encode`](Self::encode) recorded and writes the image. For an animated
    /// source this only starts the readback: the next drawn frame collects it (the engine never
    /// waits on the GPU for it). A static source's one publish waits. Failures are warnings.
    pub fn finish(&mut self, gpu: &Gpu, screen: usize) {
        let Some(dir) = &self.dir else { return };
        let Some(state) = self.screens.get_mut(screen) else {
            return;
        };
        let (Some(name), Some(pyramid)) = (&state.name, &state.pyramid) else {
            return;
        };
        if state.animated {
            let (sender, receiver) = mpsc::channel();
            pyramid
                .readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
            state.reading = Some(Reading {
                receiver,
                asked: Instant::now(),
                also: Vec::new(),
            });
            return;
        }
        match publish(gpu, pyramid, &image_path(dir, name)) {
            Ok(timing) => log::info!(
                "backdrop {name} {}x{}: gpu+readback {:.2} ms, write {:.2} ms",
                pyramid.output.0,
                pyramid.output.1,
                timing.gpu.as_secs_f64() * 1e3,
                timing.write.as_secs_f64() * 1e3,
            ),
            Err(e) => {
                log::warn!("backdrop {name}: {e:#}");
                // A failed readback can leave the buffer mapped: start over.
                state.pyramid = None;
            }
        }
        // A static wallpaper won't publish again until the next load: free its textures.
        state.pyramid = None;
    }
}

/// A readback in flight: the map's result arrives on the channel.
struct Reading {
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    /// When it was asked for.
    asked: Instant,
    /// Screens sharing the kept frame it reads, written with the same image.
    also: Vec<String>,
}

/// Gives screen `to` the image screen `from` just published.
fn copy_image(dir: &Path, from: &str, to: &str) {
    let copied = std::fs::read(image_path(dir, from))
        .map_err(anyhow::Error::from)
        .and_then(|bmp| write_atomic(&image_path(dir, to), &bmp));
    match copied {
        Ok(()) => log::info!("backdrop {to}: the same image as {from}"),
        Err(e) => log::warn!("backdrop {to}: {e:#}"),
    }
}

impl ScreenState {
    /// Picks up an animated screen's readback if it's done: copies the pixels out, unmaps, and
    /// encodes and writes the image on a short-lived thread. Returns whether the readback buffer
    /// is free again.
    fn collect(&mut self, gpu: &Gpu, dir: &Path) -> bool {
        let Some(reading) = &self.reading else {
            return true;
        };
        // Runs finished map callbacks; never waits.
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        let mapped = match reading.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(wgpu::BufferAsyncError),
        };
        let waited = reading.asked.elapsed();
        let also = self.reading.take().map(|r| r.also).unwrap_or_default();
        let (Some(name), Some(pyramid)) = (&self.name, &self.pyramid) else {
            return true;
        };
        if let Err(e) = mapped {
            log::warn!("backdrop {name}: {e}");
            self.pyramid = None;
            return true;
        }
        let copied = Instant::now();
        let bytes = match pyramid.readback.slice(..).get_mapped_range() {
            Ok(range) => range.to_vec(),
            Err(e) => {
                log::warn!("backdrop {name}: {e}");
                self.pyramid = None;
                return true;
            }
        };
        pyramid.readback.unmap();
        let copy = copied.elapsed();
        let (name, (w, h), stride, bgra) = (
            name.clone(),
            pyramid.output,
            pyramid.stride as usize,
            pyramid.bgra,
        );
        // The next publish is seconds away: its ~5 MiB (2560x1440) are made again then.
        self.pyramid = None;
        let paths: Vec<PathBuf> = std::iter::once(&name)
            .chain(&also)
            .map(|name| image_path(dir, name))
            .collect();
        let spawned = std::thread::Builder::new()
            .name("backdrop".into())
            .spawn(move || {
                let started = Instant::now();
                let bmp = encode_bmp(w, h, stride, &bytes, bgra);
                for path in &paths {
                    if let Err(e) = write_atomic(path, &bmp) {
                        log::warn!("backdrop {}: {e:#}", path.display());
                    }
                }
                log::info!(
                    "backdrop {name} {w}x{h} ({} screens): ready after {:.1} ms (not waited on), \
                     copy {:.2} ms on the frame, write {:.2} ms off it",
                    paths.len(),
                    waited.as_secs_f64() * 1e3,
                    copy.as_secs_f64() * 1e3,
                    started.elapsed().as_secs_f64() * 1e3,
                );
            });
        if let Err(e) = spawned {
            log::warn!("backdrop: {e}");
        }
        true
    }
}

/// When one screen's backdrop is due. Pure bookkeeping: the caller supplies
/// the clock and the request-file read.
#[derive(Debug)]
struct Cadence {
    /// A theme was just loaded: publish on the next drawn frame.
    pending: bool,
    /// From the last request file seen.
    interval: Duration,
    /// When the request file was last consulted.
    checked: Option<Instant>,
}

impl Cadence {
    fn new() -> Self {
        Self {
            pending: true,
            interval: DEFAULT_INTERVAL,
            checked: None,
        }
    }

    /// Whether a frame drawn at `now` should be published. `request` reads the
    /// request file (its interval, or `None` if absent); it runs at most once
    /// per interval, and only for animated sources.
    fn due(
        &mut self,
        now: Instant,
        animated: bool,
        request: impl FnOnce() -> Option<Duration>,
    ) -> bool {
        if std::mem::take(&mut self.pending) {
            self.checked = Some(now);
            return true;
        }
        if !animated
            || self
                .checked
                .is_some_and(|t| now.duration_since(t) < self.interval)
        {
            return false;
        }
        self.checked = Some(now);
        match request() {
            Some(interval) => {
                self.interval = interval;
                true
            }
            None => false,
        }
    }
}

/// Everything GPU-side for one screen size, cached between publishes.
struct Pyramid {
    /// Screen size.
    size: (u32, u32),
    /// Published image size.
    output: (u32, u32),
    /// Indexed like [`DIVISORS`].
    textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
    /// `quads[k]` samples `textures[k]`.
    quads: Vec<sources::blit::BlitQuad>,
    readback: wgpu::Buffer,
    /// Bytes per readback row, padded to wgpu's copy alignment.
    stride: u32,
    /// The swapchain stores blue first.
    bgra: bool,
}

impl Pyramid {
    fn new(gpu: &Gpu, size: (u32, u32)) -> Result<Self> {
        let bgra = match gpu.format {
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
            other => bail!("cannot read back {other:?}"),
        };
        let sizes = DIVISORS.map(|d| scaled(size, d));
        let textures: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(k, &size)| {
                let usage = if k == RESULT {
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                };
                gpu.texture("backdrop", size, gpu.format, usage, None)
            })
            .collect();
        let views: Vec<_> = textures
            .iter()
            .map(|t| t.create_view(&Default::default()))
            .collect();
        let quads: Vec<_> = views
            .iter()
            .zip(sizes)
            .map(|(view, size)| {
                let quad = gpu.blit.quad(&gpu.device, view);
                // Stretch is an identity mapping whatever the target size, so
                // each quad is set once.
                quad.set(&gpu.queue, size, size, Fit::Stretch, 1.0);
                quad
            })
            .collect();

        let output = sizes[RESULT];
        let stride = (output.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("backdrop readback"),
            size: u64::from(stride) * u64::from(output.1),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            size,
            output,
            textures,
            views,
            quads,
            readback,
            stride,
            bgra,
        })
    }

    /// Records the pyramid passes from `source` (a stretch quad of the kept frame) and the
    /// copy of the result into the readback buffer.
    fn blur(&self, gpu: &Gpu, source: &BlitQuad, encoder: &mut wgpu::CommandEncoder) {
        let rest = PASSES.map(|(from, to)| (&self.quads[from], to));
        for (quad, to) in std::iter::once((source, 0)).chain(rest) {
            let mut pass = sources::begin(
                encoder,
                &self.views[to],
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            quad.draw(&gpu.blit, &mut pass);
        }
        encoder.copy_texture_to_buffer(
            self.textures[RESULT].as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.stride),
                    rows_per_image: Some(self.output.1),
                },
            },
            extent(self.output),
        );
    }

    /// Waits for the GPU, then runs `read` on the mapped readback rows
    /// (`stride` bytes apart).
    fn with_pixels<T>(&self, gpu: &Gpu, read: impl FnOnce(&[u8]) -> T) -> Result<T> {
        let (sender, receiver) = mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let mapped = self.readback.slice(..).get_mapped_range()?;
        let value = read(&mapped);
        drop(mapped);
        self.readback.unmap();
        Ok(value)
    }
}

/// Where the time of one publish went.
struct Timing {
    /// Waiting for the GPU (the frame's own work included) and mapping.
    gpu: Duration,
    /// Encoding the BMP and writing it.
    write: Duration,
}

/// Reads back the pyramid's result and writes it to `path`.
fn publish(gpu: &Gpu, pyramid: &Pyramid, path: &Path) -> Result<Timing> {
    let waited = Instant::now();
    let (bmp, gpu_time, encode_time) = pyramid.with_pixels(gpu, |bytes| {
        let gpu_time = waited.elapsed();
        let encoding = Instant::now();
        let (w, h) = pyramid.output;
        let bmp = encode_bmp(w, h, pyramid.stride as usize, bytes, pyramid.bgra);
        (bmp, gpu_time, encoding.elapsed())
    })?;
    let writing = Instant::now();
    write_atomic(path, &bmp)?;
    Ok(Timing {
        gpu: gpu_time,
        write: encode_time + writing.elapsed(),
    })
}

/// `size` shrunk by `divisor`, rounded, at least one pixel each way.
fn scaled((w, h): (u32, u32), divisor: u32) -> (u32, u32) {
    let shrink = |v: u32| ((v + divisor / 2) / divisor).max(1);
    (shrink(w), shrink(h))
}

fn extent((width, height): (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

/// `\\.\DISPLAY1` -> `DISPLAY1`.
fn device_name(device: &str) -> &str {
    device.trim_start_matches(r"\\.\")
}

fn image_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.bmp"))
}

/// Deletes an image left by a previous theme, if there is one.
fn remove_stale(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => log::info!("backdrop: removed stale {}", path.display()),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => log::warn!("backdrop: cannot remove {}: {e}", path.display()),
    }
}

/// Writes `bytes` to `path` via `<path>.tmp` + rename, so the bars' folder
/// watch never sees a half-written image.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("backdrop path has no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// The request file's interval, or `None` if the file is absent. A file that
/// exists but cannot be read still counts as a request.
fn read_request(path: &Path) -> Option<Duration> {
    [path.to_owned(), path.with_file_name("widgets-request.json")]
        .into_iter()
        .filter_map(|path| match std::fs::read_to_string(path) {
            Ok(json) => Some(parse_interval(&json)),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(_) => Some(DEFAULT_INTERVAL),
        })
        .min()
}

/// The `"interval"` (seconds) of a request file's JSON, clamped to 2..=600;
/// 10 when missing or not a number. Scans for the one field instead of
/// pulling a JSON parser into this crate.
fn parse_interval(json: &str) -> Duration {
    let seconds = json
        .split_once("\"interval\"")
        .and_then(|(_, rest)| rest.trim_start().strip_prefix(':'))
        .and_then(|rest| {
            let rest = rest.trim_start();
            let number = rest
                .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
                .map_or(rest, |end| &rest[..end]);
            number.parse::<f64>().ok()
        })
        .filter(|s| s.is_finite())
        .unwrap_or(DEFAULT_INTERVAL.as_secs_f64());
    // Clamp before converting: `from_secs_f64` panics on negative values.
    Duration::from_secs_f64(seconds.clamp(MIN_INTERVAL.as_secs_f64(), MAX_INTERVAL.as_secs_f64()))
}

/// An uncompressed 24-bit top-down BMP of `width x height` pixels, from rows
/// of 4-byte pixels `stride` bytes apart (`bgra`: blue first, else red first).
pub(crate) fn encode_bmp(
    width: u32,
    height: u32,
    stride: usize,
    pixels: &[u8],
    bgra: bool,
) -> Vec<u8> {
    let row = (width as usize * 3).next_multiple_of(4);
    let image = row * height as usize;
    let mut out = Vec::with_capacity(BMP_HEADERS + image);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((BMP_HEADERS + image) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]); // reserved
    out.extend_from_slice(&(BMP_HEADERS as u32).to_le_bytes()); // pixel data offset
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER size
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes()); // negative: top-down
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(image as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 dpi, both ways
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // palette colors: none

    for line in pixels.chunks(stride).take(height as usize) {
        for px in line[..width as usize * 4].as_chunks::<4>().0 {
            let (b, g, r) = if bgra {
                (px[0], px[1], px[2])
            } else {
                (px[2], px[1], px[0])
            };
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(out.len() + row - width as usize * 3, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn bmp_has_top_down_24_bit_headers_and_padded_rows() {
        // 3x2 pixels, source rows padded to 16 bytes; red-first bytes.
        let mut pixels = vec![0xEE; 32];
        pixels[..12].copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255]);
        pixels[16..28].copy_from_slice(&[10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255]);
        let bmp = encode_bmp(3, 2, 16, &pixels, false);

        // 3 pixels * 3 bytes = 9, padded to 12 per row.
        assert_eq!(bmp.len(), 54 + 24);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32_at(&bmp, 2), 78);
        assert_eq!(u32_at(&bmp, 10), 54);
        assert_eq!(u32_at(&bmp, 14), 40);
        assert_eq!(u32_at(&bmp, 18), 3);
        assert_eq!(u32_at(&bmp, 22) as i32, -2);
        assert_eq!(bmp[26..28], 1u16.to_le_bytes());
        assert_eq!(bmp[28..30], 24u16.to_le_bytes());
        assert_eq!(u32_at(&bmp, 30), 0, "BI_RGB");
        assert_eq!(u32_at(&bmp, 34), 24);

        // BMP stores blue first, so red-first input is swapped; rows pad with zeros.
        assert_eq!(&bmp[54..66], &[3, 2, 1, 6, 5, 4, 9, 8, 7, 0, 0, 0]);
        assert_eq!(&bmp[66..78], &[12, 11, 10, 15, 14, 13, 18, 17, 16, 0, 0, 0]);
    }

    #[test]
    fn bgra_pixels_are_copied_in_order_and_aligned_rows_get_no_padding() {
        let bmp = encode_bmp(4, 1, 16, &[1, 2, 3, 255].repeat(4), true);
        assert_eq!(bmp.len(), 54 + 12);
        assert_eq!(&bmp[54..60], &[1, 2, 3, 1, 2, 3]);
    }

    #[test]
    fn interval_is_read_clamped_and_defaults_when_unusable() {
        let secs = |json| parse_interval(json).as_secs_f64();
        assert_eq!(secs(r#"{ "interval": 30 }"#), 30.0);
        assert_eq!(secs(r#"{"other":1,"interval":2.5}"#), 2.5);
        assert_eq!(secs(r#"{ "interval": 0 }"#), 2.0);
        assert_eq!(secs(r#"{ "interval": 100000 }"#), 600.0);
        assert_eq!(secs(r#"{ "interval": -4 }"#), 2.0);
        assert_eq!(secs("{}"), 10.0);
        assert_eq!(secs(""), 10.0);
        assert_eq!(secs(r#"{ "interval": "fast" }"#), 10.0);
        assert_eq!(secs(r#"{ "interval": null }"#), 10.0);
        assert_eq!(secs("not json at all"), 10.0);
    }

    #[test]
    fn widget_snapshots_work_without_bars_and_keep_the_shortest_request() {
        let dir =
            std::env::temp_dir().join(format!("mypaper-backdrop-requests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bars = dir.join(REQUEST_FILE);
        let widgets = dir.join("widgets-request.json");
        assert_eq!(read_request(&bars), None);
        std::fs::write(&widgets, r#"{"interval":10}"#).unwrap();
        assert_eq!(read_request(&bars), Some(Duration::from_secs(10)));
        std::fs::write(&bars, r#"{"interval":60}"#).unwrap();
        assert_eq!(read_request(&bars), Some(Duration::from_secs(10)));
        std::fs::write(&bars, r#"{"interval":2}"#).unwrap();
        assert_eq!(read_request(&bars), Some(Duration::from_secs(2)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn first_frame_after_load_is_always_due_once() {
        let t0 = Instant::now();
        let mut cadence = Cadence::new();
        // Even a static source, and without reading the request file.
        assert!(cadence.due(t0, false, || unreachable!()));
        assert!(!cadence.due(t0, false, || unreachable!()));
        assert!(!cadence.due(t0 + Duration::from_secs(60), false, || unreachable!()));
    }

    #[test]
    fn covered_refresh_bypasses_cadence_without_starting_a_timer() {
        let mut backdrop = Backdrop::with(None, vec![Some("DISPLAY1".into())], |_| false);
        let now = Instant::now();
        assert!(
            backdrop.screens[0]
                .cadence
                .due(now, false, || unreachable!())
        );
        assert!(
            !backdrop.screens[0]
                .cadence
                .due(now, false, || unreachable!())
        );
        backdrop.refresh_screen(0);
        assert!(
            backdrop.screens[0]
                .cadence
                .due(now, false, || unreachable!())
        );
        assert!(
            !backdrop.screens[0]
                .cadence
                .due(now, false, || unreachable!())
        );
    }

    #[test]
    fn animated_sources_republish_per_interval_and_only_with_a_request() {
        let t0 = Instant::now();
        let after = |s| t0 + Duration::from_secs(s);
        let mut cadence = Cadence::new();
        assert!(cadence.due(t0, true, || None));

        // Within the default interval the request is not even looked at.
        assert!(!cadence.due(after(9), true, || unreachable!()));
        // Due for a check, but nobody asked: no publish, next check an interval later.
        assert!(!cadence.due(after(10), true, || None));
        assert!(!cadence.due(after(19), true, || unreachable!()));

        // A request arrives with its own interval.
        assert!(cadence.due(after(20), true, || Some(Duration::from_secs(3))));
        assert!(!cadence.due(after(22), true, || unreachable!()));
        assert!(cadence.due(after(23), true, || Some(Duration::from_secs(3))));
        // The request file is deleted again.
        assert!(!cadence.due(after(26), true, || None));
    }

    #[test]
    fn reset_deletes_only_native_screens_stale_images() {
        let dir =
            std::env::temp_dir().join(format!("mypaper-backdrop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["DISPLAY1", "DISPLAY2"] {
            std::fs::write(image_path(&dir, name), b"old").unwrap();
        }
        let names = vec![Some("DISPLAY1".to_owned()), Some("DISPLAY2".to_owned())];
        let _ = Backdrop::with(Some(dir.clone()), names, |screen| screen == 1);

        assert!(image_path(&dir, "DISPLAY1").exists());
        assert!(!image_path(&dir, "DISPLAY2").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writing_replaces_the_image_and_leaves_no_temp_file() {
        let dir =
            std::env::temp_dir().join(format!("mypaper-backdrop-write-{}", std::process::id()));
        let path = image_path(&dir, "DISPLAY1");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn output_is_an_eighth_of_the_screen() {
        assert_eq!(scaled((2560, 1440), OUTPUT_DIVISOR), (320, 180));
        assert_eq!(scaled((1366, 768), OUTPUT_DIVISOR), (171, 96));
        assert_eq!(scaled((3, 3), 64), (1, 1));
        assert_eq!(device_name(r"\\.\DISPLAY2"), "DISPLAY2");
    }

    /// Screens sharing one kept frame read it back once and publish the same image.
    #[test]
    fn screens_sharing_a_kept_frame_share_one_readback() {
        let Some(gpu) = Gpu::headless() else {
            eprintln!("no GPU: skipping");
            return;
        };
        let dir =
            std::env::temp_dir().join(format!("mypaper-backdrop-share-{}", std::process::id()));
        let names = vec![Some("DISPLAY1".to_owned()), Some("DISPLAY2".to_owned())];
        let mut backdrop = Backdrop::with(Some(dir.clone()), names, |_| false);
        let kept = KeptFrame::new(&gpu, (1664, 936));
        gpu.write_texture(&kept.texture, &[50, 100, 200, 255].repeat(1664 * 936));
        let draw = |screen| Draw {
            screen,
            frame: 1,
            size: (2560, 1440),
            kept: &kept,
            held: false,
            animated: false,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        assert!(backdrop.encode(&gpu, draw(0), &mut encoder));
        gpu.queue.submit([encoder.finish()]);
        backdrop.finish(&gpu, 0);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        assert!(
            !backdrop.encode(&gpu, draw(1), &mut encoder),
            "no second readback"
        );
        assert!(backdrop.screens[1].pyramid.is_none());
        let image = |name| std::fs::read(image_path(&dir, name)).unwrap();
        assert_eq!(image("DISPLAY1"), image("DISPLAY2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A solid-colored screen must come out solid after the whole pyramid.
    #[test]
    fn pyramid_keeps_a_solid_color() {
        let Some(gpu) = Gpu::headless() else {
            eprintln!("no GPU: skipping");
            return;
        };
        let size = (2560, 1440);
        let pyramid = Pyramid::new(&gpu, size).unwrap();
        // A 0.65-scale kept frame, as rain-city keeps. Bgra8Unorm: blue, green, red, alpha.
        let kept = KeptFrame::new(&gpu, (1664, 936));
        let (b, g, r) = (50u8, 100u8, 200u8);
        gpu.write_texture(&kept.texture, &[b, g, r, 255].repeat(1664 * 936));
        kept.quad
            .set(&gpu.queue, (1664, 936), size, Fit::Stretch, 1.0);

        // The first run pays for the texture upload and cold pipelines; time the second.
        let run = || {
            let started = Instant::now();
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            pyramid.blur(&gpu, &kept.quad, &mut encoder);
            gpu.queue.submit([encoder.finish()]);
            let (bmp, waited) = pyramid
                .with_pixels(&gpu, |bytes| {
                    let waited = started.elapsed();
                    let (w, h) = pyramid.output;
                    (
                        encode_bmp(w, h, pyramid.stride as usize, bytes, true),
                        waited,
                    )
                })
                .unwrap();
            (bmp, waited, started.elapsed())
        };
        run();
        let (bmp, waited, total) = run();
        eprintln!("pyramid {size:?} warm: gpu+readback {waited:?}, with bmp {total:?}");

        assert_eq!(pyramid.output, (320, 180));
        let row = 320 * 3;
        assert_eq!(bmp.len(), 54 + row * 180);
        for px in bmp[54..].as_chunks::<3>().0 {
            assert!(
                px[0].abs_diff(b) <= 1 && px[1].abs_diff(g) <= 1 && px[2].abs_diff(r) <= 1,
                "{px:?}"
            );
        }
    }
}
