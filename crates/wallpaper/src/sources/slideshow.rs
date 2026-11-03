//! `slideshow` wallpapers: images from a folder, cross-faded on a timer.
//!
//! Timing runs on the engine clock (it stops while hidden), so the source is
//! idle between slides: [`Redraw::At`] the next switch, and
//! [`Redraw::Every`] only while fading. The next image is decoded on a worker
//! thread while the current one is shown and uploaded on the main thread, so
//! at most two textures exist and the frame loop never waits on a decode.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use mypaper_theme::{Fit, LoadedTheme, SlideshowWallpaper};

use super::wic::{Budget, Decoded, Decoder, Picture};
use super::{Frame, Redraw, Source, begin};
use crate::gpu::Gpu;

/// Frame interval while cross-fading.
const FADE_FRAME: Duration = Duration::from_micros(16_667);
/// How often to check for a decode that is late for its slide.
const LATE_POLL: Duration = Duration::from_millis(100);
/// File extensions WIC can decode (compared case-insensitively).
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "bmp", "gif", "tif", "tiff", "webp", "heic", "heif", "jxr", "ico",
];

/// A folder of images shown one after another with a cross-fade.
pub struct SlideshowSource {
    fit: Fit,
    timeline: Timeline,
    current: Picture,
    /// Uploaded and waiting for its turn, or being faded in.
    next: Option<Picture>,
    /// A frame showing the finished fade has been drawn, so `current` may be replaced.
    fade_drawn: bool,
    /// `None` for a single image: nothing to switch to, so nothing to schedule.
    prefetch: Option<Prefetch>,
}

impl SlideshowSource {
    /// Lists the folder, decodes the first readable image right away and
    /// starts decoding the second in the background. Errors if the folder
    /// has no images or none of them decode.
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &SlideshowWallpaper) -> Result<Self> {
        let folder = theme.asset(&config.folder);
        let paths = list_images(&folder)?;
        if paths.is_empty() {
            bail!("no images in {}", folder.display());
        }
        let interval = Duration::try_from_secs_f32(config.interval)
            .context("wallpaper.interval is out of range")?;
        let transition = Duration::try_from_secs_f32(config.transition)
            .context("wallpaper.transition is out of range")?;

        let budget = Budget::new(gpu, config.fit);
        let mut playlist = Playlist::new(paths, config.shuffle);
        let decoder = Decoder::new()?;
        let current = (0..playlist.len())
            .find_map(|_| {
                let path = playlist.take_next();
                decoder
                    .decode(&path, budget)
                    .and_then(|image| Picture::upload(gpu, "slide", &image))
                    .map_err(|e| log::warn!("skipping {}: {e:#}", path.display()))
                    .ok()
            })
            .with_context(|| {
                format!(
                    "none of the images in {} could be decoded",
                    folder.display()
                )
            })?;

        let prefetch = if playlist.len() > 1 {
            let mut prefetch = Prefetch::spawn(playlist, budget)?;
            prefetch.request();
            Some(prefetch)
        } else {
            None
        };
        Ok(Self {
            fit: config.fit,
            timeline: Timeline::new(interval, transition),
            current,
            next: None,
            fade_drawn: false,
            prefetch,
        })
    }

    /// Uploads the next image once the worker has decoded it; skips
    /// files that fail.
    fn receive(&mut self, gpu: &Gpu) {
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        let Some((path, decoded)) = prefetch.poll() else {
            return;
        };
        match decoded.and_then(|image| Picture::upload(gpu, "slide", &image)) {
            Ok(picture) => {
                self.next = Some(picture);
                prefetch.failures = 0;
            }
            Err(e) => {
                log::warn!("skipping {}: {e:#}", path.display());
                prefetch.failures += 1;
                prefetch.request();
            }
        }
    }
}

impl Source for SlideshowSource {
    fn update(&mut self, gpu: &Gpu, clock: Duration) {
        self.receive(gpu);
        if self
            .timeline
            .advance(clock, self.next.is_some(), self.fade_drawn)
        {
            if let Some(next) = self.next.take() {
                self.current = next;
            }
            self.fade_drawn = false;
            if let Some(prefetch) = &mut self.prefetch {
                prefetch.request();
            }
        }
    }

    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        self.current.draw(gpu, &mut pass, frame.size, self.fit, 1.0);
        if let (Some(opacity), Some(next)) = (self.timeline.fade_progress(frame.clock), &self.next)
        {
            next.draw(gpu, &mut pass, frame.size, self.fit, opacity);
            self.fade_drawn |= opacity >= 1.0;
        }
    }

    fn redraw(&self, clock: Duration) -> Redraw {
        match &self.prefetch {
            // Nothing will ever replace the current image (single image, or
            // every other file failed): stay idle.
            Some(prefetch) if prefetch.active() || self.next.is_some() => {
                self.timeline.wake(clock, self.next.is_some())
            }
            _ => Redraw::OnChange,
        }
    }
}

/// Where in its cycle the slideshow is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// No update has run yet, so there is no clock reading to anchor on.
    Unstarted,
    /// The current slide has been fully visible since this engine-clock time.
    Holding { since: Duration },
    /// The next slide has been fading in since this engine-clock time.
    Fading { since: Duration },
}

/// Pure slide timing on the engine clock: hold for `interval`, then
/// cross-fade over `transition`.
#[derive(Debug)]
struct Timeline {
    interval: Duration,
    transition: Duration,
    phase: Phase,
}

impl Timeline {
    fn new(interval: Duration, transition: Duration) -> Self {
        Self {
            interval,
            transition,
            phase: Phase::Unstarted,
        }
    }

    /// Steps the phase to `clock`. A fade starts once the interval is up *and*
    /// the next image is ready (a late decode delays it rather than cutting).
    /// Returns `true` when the fade is complete and has been drawn at full
    /// opacity, i.e. the next image should become the current one.
    fn advance(&mut self, clock: Duration, next_ready: bool, fade_drawn: bool) -> bool {
        if self.phase == Phase::Unstarted {
            self.phase = Phase::Holding { since: clock };
        }
        if let Phase::Holding { since } = self.phase
            && next_ready
            && clock >= since + self.interval
        {
            self.phase = Phase::Fading { since: clock };
        }
        if let Phase::Fading { since } = self.phase
            && fade_drawn
            && clock >= since + self.transition
        {
            self.phase = Phase::Holding { since: clock };
            return true;
        }
        false
    }

    /// The incoming image's opacity (0..=1) while fading.
    fn fade_progress(&self, clock: Duration) -> Option<f32> {
        let Phase::Fading { since } = self.phase else {
            return None;
        };
        if self.transition.is_zero() {
            return Some(1.0);
        }
        let elapsed = clock.saturating_sub(since);
        Some(elapsed.div_duration_f32(self.transition).clamp(0.0, 1.0))
    }

    /// When the engine should next draw: every frame while fading, otherwise
    /// at the switch time (or shortly after, while the next image is late).
    fn wake(&self, clock: Duration, next_ready: bool) -> Redraw {
        match self.phase {
            Phase::Unstarted => Redraw::OnChange,
            Phase::Fading { .. } => Redraw::Every(FADE_FRAME),
            Phase::Holding { since } => {
                let switch_at = since + self.interval;
                if clock >= switch_at && !next_ready {
                    Redraw::At(clock + LATE_POLL)
                } else {
                    Redraw::At(switch_at)
                }
            }
        }
    }
}

/// The order images are shown in: sorted, or reshuffled each pass.
struct Playlist {
    paths: Vec<PathBuf>,
    cursor: usize,
    rng: Option<XorShift>,
}

impl Playlist {
    fn new(mut paths: Vec<PathBuf>, shuffle: bool) -> Self {
        let mut rng = shuffle.then(XorShift::from_clock);
        if let Some(rng) = &mut rng {
            rng.shuffle(&mut paths);
        }
        Self {
            paths,
            cursor: 0,
            rng,
        }
    }

    fn len(&self) -> usize {
        self.paths.len()
    }

    /// The next path, wrapping (and reshuffling) at the end. The playlist is never empty.
    fn take_next(&mut self) -> PathBuf {
        if self.cursor >= self.paths.len() {
            self.cursor = 0;
            if let Some(rng) = &mut self.rng {
                rng.shuffle(&mut self.paths);
            }
        }
        let path = self.paths[self.cursor].clone();
        self.cursor += 1;
        path
    }
}

/// A decode result from the worker, tagged with the file it came from.
type Fetched = (PathBuf, Result<Decoded>);

/// The decode worker thread plus the playlist it is fed from. At most one
/// request is in flight, so at most one decoded image is held in memory.
struct Prefetch {
    playlist: Playlist,
    requests: Sender<PathBuf>,
    results: Receiver<Fetched>,
    pending: bool,
    /// Consecutive failures; once every file has failed, stop asking.
    failures: usize,
}

impl Prefetch {
    /// Starts the worker. It exits when this struct is dropped.
    fn spawn(playlist: Playlist, budget: Budget) -> Result<Self> {
        let (requests, wanted) = mpsc::channel::<PathBuf>();
        let (done, results) = mpsc::channel::<Fetched>();
        thread::Builder::new()
            .name("slideshow-decode".into())
            .spawn(move || {
                let decoder = match Decoder::new() {
                    Ok(decoder) => decoder,
                    Err(e) => return log::error!("slideshow decoder unavailable: {e:#}"),
                };
                for path in wanted {
                    let decoded = decoder
                        .decode(&path, budget)
                        .with_context(|| format!("decoding {}", path.display()));
                    if done.send((path, decoded)).is_err() {
                        break;
                    }
                }
            })
            .context("starting the decode thread")?;
        Ok(Self {
            playlist,
            requests,
            results,
            pending: false,
            failures: 0,
        })
    }

    /// Asks the worker for the next image, unless one is already coming or
    /// every file has failed.
    fn request(&mut self) {
        if self.pending {
            return;
        }
        if self.failures >= self.playlist.len() {
            log::error!("no other slideshow image could be decoded; staying on the current one");
            return;
        }
        self.pending = self.requests.send(self.playlist.take_next()).is_ok();
    }

    /// The finished decode, if any.
    fn poll(&mut self) -> Option<Fetched> {
        if !self.pending {
            return None;
        }
        match self.results.try_recv() {
            Ok(fetched) => {
                self.pending = false;
                Some(fetched)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                log::error!("slideshow decode thread stopped");
                self.pending = false;
                self.failures = self.playlist.len();
                None
            }
        }
    }

    /// Whether another image may still arrive.
    fn active(&self) -> bool {
        self.pending
    }
}

/// Minimal xorshift64 generator for shuffling (no `rand` dependency).
struct XorShift(u64);

impl XorShift {
    /// Seeded from the system time.
    fn from_clock() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // Spread nearby seeds apart; the state must never be zero.
        Self(nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Fisher-Yates.
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

/// Whether `path` has an extension WIC can decode.
fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// Images directly inside `folder` (not recursive), sorted by name.
fn list_images(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let entries =
        std::fs::read_dir(folder).with_context(|| format!("reading {}", folder.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", folder.display()))?
            .path();
        if path.is_file() && is_image(&path) {
            paths.push(path);
        }
    }
    paths.sort_by_cached_key(|p| p.file_name().map(OsStr::to_ascii_lowercase));
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn extension_filter_is_case_insensitive_and_exact() {
        for name in ["a.jpg", "b.JPEG", "c.Png", "d.heic", "e.tiff", "f.ICO"] {
            assert!(is_image(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "b.jpg.bak", "jpg", "c.mp4", ".png"] {
            assert!(!is_image(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn lists_only_images_in_the_folder_sorted_by_name() {
        let dir = std::env::temp_dir().join(format!("mypaper-slides-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested.png")).unwrap();
        for name in ["b.png", "A.jpg", "c.txt", "d.WEBP"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let names: Vec<_> = list_images(&dir)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        // A directory named like an image, and a non-image, are skipped.
        assert_eq!(names, ["A.jpg", "b.png", "d.WEBP"]);
    }

    #[test]
    fn holds_for_the_interval_then_fades_and_hands_over() {
        let mut t = Timeline::new(secs(10), secs(2));
        // The first update anchors the cycle at its clock reading.
        assert!(!t.advance(secs(5), true, false));
        assert_eq!(t.wake(secs(5), true), Redraw::At(secs(15)));
        assert!(!t.advance(secs(14), true, false));
        assert_eq!(t.fade_progress(secs(14)), None);

        assert!(!t.advance(secs(15), true, false));
        assert_eq!(t.fade_progress(secs(15)), Some(0.0));
        assert_eq!(t.fade_progress(secs(16)), Some(0.5));
        assert!(matches!(t.wake(secs(16), true), Redraw::Every(_)));

        // Not handed over until a frame at full opacity has been drawn.
        assert!(!t.advance(secs(17), true, false));
        assert_eq!(t.fade_progress(secs(30)), Some(1.0));
        assert!(t.advance(secs(17), true, true));
        assert_eq!(t.fade_progress(secs(17)), None);
        assert_eq!(t.wake(secs(17), false), Redraw::At(secs(27)));
    }

    #[test]
    fn a_late_decode_delays_the_fade_instead_of_cutting() {
        let mut t = Timeline::new(secs(10), secs(2));
        t.advance(secs(0), false, false);
        assert!(!t.advance(secs(10), false, false));
        assert_eq!(t.fade_progress(secs(10)), None);
        assert_eq!(t.wake(secs(10), false), Redraw::At(secs(10) + LATE_POLL));

        // The fade starts when the image finally arrives, not at the missed deadline.
        t.advance(secs(12), true, false);
        assert_eq!(t.fade_progress(secs(12)), Some(0.0));
    }

    #[test]
    fn zero_transition_is_a_cut_after_one_frame() {
        let mut t = Timeline::new(secs(10), Duration::ZERO);
        t.advance(secs(0), true, false);
        assert!(!t.advance(secs(10), true, false));
        assert_eq!(t.fade_progress(secs(10)), Some(1.0));
        assert!(t.advance(secs(10), true, true));
    }

    #[test]
    fn shuffle_keeps_every_path_and_playlist_cycles() {
        let paths: Vec<PathBuf> = (0..5).map(|i| PathBuf::from(format!("{i}.png"))).collect();
        let mut playlist = Playlist::new(paths.clone(), true);
        for _ in 0..3 {
            let mut pass: Vec<_> = (0..5).map(|_| playlist.take_next()).collect();
            pass.sort();
            assert_eq!(pass, paths);
        }
    }
}
