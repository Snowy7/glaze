//! One shell publishes complete snapshots in physical virtual-desktop pixels. Motion belongs
//! to the shell. The engine waits on `Reader::wake_event` and the writer's process handle.
//! A seqlock protects the snapshot; payload words are atomic too, so concurrent reads never
//! create Rust data races. The session-local mapping is 5.7 KiB, with no files or polling.
//!
//! Each plate's finer knobs ([`Style`]) travel in a second section (`<channel>.style`, 1.9 KiB)
//! stamped with the snapshot's sequence, written inside the same seqlock window. A writer or
//! reader built before it simply doesn't touch it, and a stamp that doesn't match the snapshot
//! reads as default styles, so either end can be older than the other.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

use anyhow::{Result, bail, ensure};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenProcess, PROCESS_SYNCHRONIZE, ReleaseMutex, SetEvent,
    WaitForSingleObject,
};
use windows::core::HSTRING;

pub const MAX_PLATES: usize = 96;
/// The largest blur a plate may ask for, in physical px: 64 DIP (the bars' largest) at 400 %.
/// The engine's frost tops out at 64 pt whatever the screen's scale (`material::liquid`).
pub const MAX_BLUR: f32 = 256.0;
const WORDS: usize = 15;
const CHANNEL: &str = "Local\\mypaper.glass.v1";

/// A plate's Liquid Glass material (plan, "Material controls"). The engine draws it as
/// liquid-rust's `Glass.regular` with these knobs (`crates/wallpaper/src/material.rs`,
/// `docs/glass.md`). All values except blur are in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Clear (1) to tinted (0): iOS 27's Liquid Glass slider, reversed.
    pub clarity: f32,
    /// Frost, physical pixels (points once divided by the screen's scale), up to [`MAX_BLUR`].
    pub blur: f32,
    /// How far the bevel bends what is behind it (Figma *Refraction*).
    pub refraction: f32,
    /// The specular rim light (0.75 is the kit's 30 % *Light*).
    pub highlight: f32,
    /// Straight sRGB RGBA. The colour sets the scheme (a dark tint takes the dark face); alpha
    /// is a stained-glass tint's strength.
    pub tint: [f32; 4],
}

impl Default for Material {
    /// Apple's kit `Glass.regular` on a 150 % screen: the slider's middle, 10 pt of frost,
    /// refraction 70 %, light 30 %, dark scheme.
    fn default() -> Self {
        Self {
            clarity: 0.5,
            blur: 15.0,
            refraction: 0.7,
            highlight: 0.75,
            tint: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// A plate's finer glass knobs beyond [`Material`] (parity C07, C10, C12), published with
/// [`Writer::publish_styled`]. `Style::default()` draws exactly as a plate without one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Brightness of what's behind the glass, a multiplier, `0..=2` (1: as is).
    pub brightness: f32,
    /// Saturation of what's behind the glass, `0..=3` (1: as is, 0: grey).
    pub saturation: f32,
    /// Corner smoothing, `0..=1`: 0 quarter circles (the default), 0.6 iOS's continuous
    /// (squircle) corners. The surface must draw the same corners.
    pub smoothing: f32,
    /// Sheen: the soft light just inside the lit (top) edge, `0..=1`, 0.5 the kit's;
    /// `None` follows the material's highlight (0.75 is the kit's).
    pub sheen: Option<f32>,
    /// Depth: the broad soft shade inside the glass under its lit edge, `0..=1`, 0.5 the kit's;
    /// `None` follows the material's highlight.
    pub depth: Option<f32>,
    /// Independent bottom inner shadow, `0..=1`; zero (the default) is off.
    pub inner_shadow: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            saturation: 1.0,
            smoothing: 0.0,
            sheen: None,
            depth: None,
            inner_shadow: 0.0,
        }
    }
}

const STYLE_WORDS: usize = 5;
/// `None` on the wire.
const UNSET: f32 = -1.0;

impl Style {
    fn words(self) -> [u32; STYLE_WORDS] {
        [
            self.brightness,
            self.saturation,
            self.smoothing,
            self.sheen.unwrap_or(UNSET),
            self.depth.unwrap_or(UNSET),
        ]
        .map(f32::to_bits)
    }

    fn from_words(words: [u32; STYLE_WORDS]) -> Self {
        let [brightness, saturation, smoothing, sheen, depth] = words.map(f32::from_bits);
        let set = |v: f32| (v != UNSET).then_some(v);
        Self {
            brightness,
            saturation,
            smoothing,
            sheen: set(sheen),
            depth: set(depth),
            inner_shadow: 0.0,
        }
    }

    fn valid(self) -> bool {
        let within = |v: f32, max: f32| (0.0..=max).contains(&v);
        within(self.brightness, 2.0)
            && within(self.saturation, 3.0)
            && within(self.smoothing, 1.0)
            && within(self.inner_shadow, 1.0)
            && [self.sheen, self.depth]
                .into_iter()
                .all(|v| v.is_none_or(|v| within(v, 1.0)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    /// Stable per plate. Its low 32 bits are the window of the surface the plate belongs to
    /// (window handles fit in 32 bits), which the engine needs to draw the plate as an overlay
    /// under that window when the wallpaper is a web page or an app.
    pub id: u64,
    /// [x, y, width, height], physical virtual-desktop pixels, with subpixel precision.
    pub rect: [f32; 4],
    pub radius: f32,
    pub material: Material,
}

impl Plate {
    fn words(self) -> [u32; WORDS] {
        let m = self.material;
        let [x, y, w, h] = self.rect;
        let [r, g, b, a] = m.tint;
        let mut out = [0; WORDS];
        out[0] = self.id as u32;
        out[1] = (self.id >> 32) as u32;
        for (word, value) in out[2..].iter_mut().zip([
            x,
            y,
            w,
            h,
            self.radius,
            m.clarity,
            m.blur,
            m.refraction,
            m.highlight,
            r,
            g,
            b,
            a,
        ]) {
            *word = value.to_bits();
        }
        out
    }

    fn from_words(words: [u32; WORDS]) -> Self {
        let v = words.map(f32::from_bits);
        Self {
            id: u64::from(words[0]) | (u64::from(words[1]) << 32),
            rect: [v[2], v[3], v[4], v[5]],
            radius: v[6],
            material: Material {
                clarity: v[7],
                blur: v[8],
                refraction: v[9],
                highlight: v[10],
                tint: [v[11], v[12], v[13], v[14]],
            },
        }
    }

    fn valid(self) -> bool {
        let m = self.material;
        self.rect.into_iter().all(f32::is_finite)
            && self.rect[2] > 0.0
            && self.rect[3] > 0.0
            && self.radius.is_finite()
            && self.radius >= 0.0
            && [m.clarity, m.refraction, m.highlight]
                .into_iter()
                .chain(m.tint)
                .all(|v| (0.0..=1.0).contains(&v))
            && (0.0..=MAX_BLUR).contains(&m.blur)
    }
}

#[repr(C)]
struct Shared {
    sequence: AtomicU64,
    published_qpc: AtomicU64,
    pid: AtomicU32,
    count: AtomicU32,
    plates: [AtomicU32; MAX_PLATES * WORDS],
}

/// The plates' [`Style`]s, valid for the snapshot whose sequence `sequence` holds.
#[repr(C)]
struct Styles {
    sequence: AtomicU64,
    words: [AtomicU32; MAX_PLATES * STYLE_WORDS],
}

/// Additive sidecar: the v1 style mapping keeps its size/stride for old readers and writers.
#[repr(C)]
struct InnerShadows {
    sequence: AtomicU64,
    words: [AtomicU32; MAX_PLATES],
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// A named page-file section of `T`, created or opened, mapped whole until drop.
struct Section {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    _mapping: Handle,
}
impl Section {
    fn open<T>(name: &str) -> Result<Self> {
        // SAFETY: a page-file mapping starts zeroed (valid atomic representations), is aligned
        // to an allocation boundary, and stays mapped until Drop. Named v1 fixes the ABI.
        unsafe {
            let mapping = Handle(CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size_of::<T>() as u32,
                &HSTRING::from(name),
            )?);
            let view = MapViewOfFile(mapping.0, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<T>());
            ensure!(
                !view.Value.is_null(),
                "MapViewOfFile: {}",
                windows::core::Error::from_thread()
            );
            Ok(Self {
                view,
                _mapping: mapping,
            })
        }
    }
}
impl Drop for Section {
    fn drop(&mut self) {
        // SAFETY: this view belongs to us and is no longer referenced.
        let _ = unsafe { UnmapViewOfFile(self.view) };
    }
}

struct Mapping {
    plates: Section,
    styles: Section,
    inner_shadows: Section,
    wake: Handle,
}
impl Mapping {
    fn open(name: &str) -> Result<Self> {
        let plates = Section::open::<Shared>(name)?;
        let styles = Section::open::<Styles>(&format!("{name}.style"))?;
        let inner_shadows = Section::open::<InnerShadows>(&format!("{name}.inner-shadow"))?;
        // SAFETY: a named auto-reset event, owned until drop.
        let wake = Handle(unsafe {
            CreateEventW(None, false, false, &HSTRING::from(format!("{name}.wake")))?
        });
        Ok(Self {
            plates,
            styles,
            inner_shadows,
            wake,
        })
    }
    fn shared(&self) -> &Shared {
        // SAFETY: the section holds a correctly aligned Shared for our whole lifetime.
        unsafe { &*self.plates.view.Value.cast::<Shared>() }
    }
    fn styles(&self) -> &Styles {
        // SAFETY: the section holds a correctly aligned Styles for our whole lifetime.
        unsafe { &*self.styles.view.Value.cast::<Styles>() }
    }
    fn inner_shadows(&self) -> &InnerShadows {
        // SAFETY: the section holds a correctly aligned InnerShadows for our whole lifetime.
        unsafe { &*self.inner_shadows.view.Value.cast::<InnerShadows>() }
    }
}

/// Exactly one writer per session. A named mutex held for this object's lifetime rejects a
/// second shell. After a crash its abandoned mutex is recoverable. Drop clears the plates.
pub struct Writer {
    mapping: Mapping,
    lease: Handle,
}
impl Writer {
    pub fn open() -> Result<Self> {
        Self::named(CHANNEL)
    }
    fn named(name: &str) -> Result<Self> {
        // SAFETY: create/open our named lifetime mutex, acquired without waiting.
        let lease =
            Handle(unsafe { CreateMutexW(None, false, &HSTRING::from(format!("{name}.writer")))? });
        // SAFETY: lease is valid.
        let status = unsafe { WaitForSingleObject(lease.0, 0) };
        ensure!(
            status == WAIT_OBJECT_0 || status == WAIT_ABANDONED,
            "another glass writer is active"
        );
        let mapping = match Mapping::open(name) {
            Ok(mapping) => mapping,
            Err(error) => {
                // SAFETY: this thread owns the mutex.
                let _ = unsafe { ReleaseMutex(lease.0) };
                return Err(error);
            }
        };
        if mapping.shared().pid.load(Ordering::Acquire) == std::process::id() {
            // A Win32 mutex is recursive on its owning thread; reject that second writer too.
            // SAFETY: release only this recursive acquisition, leaving the first writer's lease.
            let _ = unsafe { ReleaseMutex(lease.0) };
            bail!("another glass writer is active on this thread");
        }
        let mut writer = Self { mapping, lease };
        writer.publish(&[])?;
        Ok(writer)
    }

    /// Replaces all plates, signals the wake event, and returns the even snapshot sequence.
    /// Invalid geometry/materials or duplicate ids fail before changing shared memory.
    pub fn publish(&mut self, plates: &[Plate]) -> Result<u64> {
        self.publish_as(plates, &[], std::process::id())
    }

    /// [`publish`](Self::publish) with each plate's [`Style`] (`styles[i]` for `plates[i]`);
    /// empty `styles` is every plate at `Style::default()`.
    pub fn publish_styled(&mut self, plates: &[Plate], styles: &[Style]) -> Result<u64> {
        self.publish_as(plates, styles, std::process::id())
    }

    fn publish_as(&mut self, plates: &[Plate], styles: &[Style], pid: u32) -> Result<u64> {
        ensure!(
            plates.len() <= MAX_PLATES,
            "at most {MAX_PLATES} glass plates"
        );
        ensure!(
            styles.is_empty() || styles.len() == plates.len(),
            "one glass style per plate"
        );
        for (i, plate) in plates.iter().enumerate() {
            ensure!(plate.valid(), "invalid glass plate {}", plate.id);
            ensure!(
                styles.get(i).is_none_or(|s| s.valid()),
                "invalid glass style for plate {}",
                plate.id
            );
            ensure!(
                !plates[..i].iter().any(|p| p.id == plate.id),
                "duplicate glass id {}",
                plate.id
            );
        }
        let shared = self.mapping.shared();
        // Recover an odd sequence left by a crashed writer; the mutex guarantees exclusivity.
        let base = shared.sequence.load(Ordering::Relaxed) | 1;
        shared.sequence.store(base, Ordering::SeqCst);
        shared.pid.store(pid, Ordering::Relaxed);
        shared.count.store(plates.len() as u32, Ordering::Relaxed);
        shared.published_qpc.store(qpc(), Ordering::Relaxed);
        for (slot, plate) in shared.plates.as_chunks::<WORDS>().0.iter().zip(plates) {
            for (word, value) in slot.iter().zip(plate.words()) {
                word.store(value, Ordering::Relaxed);
            }
        }
        let sequence = base.wrapping_add(1);
        // The styles, inside the same window: stamped with this snapshot, or with none.
        let style = self.mapping.styles();
        for (slot, s) in style.words.as_chunks::<STYLE_WORDS>().0.iter().zip(styles) {
            for (word, value) in slot.iter().zip(s.words()) {
                word.store(value, Ordering::Relaxed);
            }
        }
        let stamp = if styles.is_empty() { 0 } else { sequence };
        style.sequence.store(stamp, Ordering::Relaxed);
        let inner = self.mapping.inner_shadows();
        for (word, s) in inner.words.iter().zip(styles) {
            word.store(s.inner_shadow.to_bits(), Ordering::Relaxed);
        }
        inner.sequence.store(stamp, Ordering::Relaxed);
        shared.sequence.store(sequence, Ordering::Release);
        // SAFETY: valid event; auto-reset coalesces bursts without losing the latest snapshot.
        unsafe {
            SetEvent(self.mapping.wake.0)?;
        }
        Ok(sequence)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.publish_as(&[], &[], 0);
        // SAFETY: the creating thread owns the lease. Writer is intentionally not Send.
        let _ = unsafe { ReleaseMutex(self.lease.0) };
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub sequence: u64,
    pub published_qpc: u64,
    pub plates: Vec<Plate>,
    /// `styles[i]` is `plates[i]`'s; defaults when the writer published none.
    pub styles: Vec<Style>,
}

pub struct Reader {
    mapping: Mapping,
    sequence: u64,
    pid: u32,
    process: Option<Handle>,
}
impl Reader {
    pub fn open() -> Result<Self> {
        Self::named(CHANNEL)
    }
    fn named(name: &str) -> Result<Self> {
        Ok(Self {
            mapping: Mapping::open(name)?,
            sequence: u64::MAX,
            pid: 0,
            process: None,
        })
    }
    pub fn wake_event(&self) -> HANDLE {
        self.mapping.wake.0
    }
    /// Include this in the engine wait set so a crashed shell clears its plates while idle.
    pub fn writer_process(&self) -> Option<HANDLE> {
        self.process.as_ref().map(|h| h.0)
    }

    /// The process id of the writer of the last snapshot read (0 before one), which owns the
    /// windows named in its plates' ids.
    pub fn writer_pid(&self) -> u32 {
        self.pid
    }

    /// Latest coherent snapshot, once per sequence; never waits for a writer in flight.
    /// A dead writer produces one empty snapshot. A wake during a read stays signalled.
    pub fn read_changed(&mut self) -> Result<Option<Snapshot>> {
        let shared = self.mapping.shared();
        for _ in 0..8 {
            let sequence = shared.sequence.load(Ordering::Acquire);
            if sequence & 1 != 0 {
                continue;
            }
            if sequence == self.sequence {
                break;
            }
            let pid = shared.pid.load(Ordering::Relaxed);
            let count = shared.count.load(Ordering::Relaxed) as usize;
            if count > MAX_PLATES {
                bail!("invalid glass snapshot count");
            }
            let published_qpc = shared.published_qpc.load(Ordering::Relaxed);
            let plates: Vec<_> = shared
                .plates
                .as_chunks::<WORDS>()
                .0
                .iter()
                .take(count)
                .map(|slot| {
                    Plate::from_words(std::array::from_fn(|i| slot[i].load(Ordering::Relaxed)))
                })
                .collect();
            let style = self.mapping.styles();
            let mut styles: Vec<_> = if style.sequence.load(Ordering::Relaxed) == sequence {
                style
                    .words
                    .as_chunks::<STYLE_WORDS>()
                    .0
                    .iter()
                    .take(count)
                    .map(|slot| {
                        Style::from_words(std::array::from_fn(|i| slot[i].load(Ordering::Relaxed)))
                    })
                    .collect()
            } else {
                vec![Style::default(); count]
            };
            let inner = self.mapping.inner_shadows();
            if inner.sequence.load(Ordering::Relaxed) == sequence {
                for (s, word) in styles.iter_mut().zip(&inner.words) {
                    s.inner_shadow = f32::from_bits(word.load(Ordering::Relaxed));
                }
            }
            fence(Ordering::Acquire);
            if sequence != shared.sequence.load(Ordering::Acquire) {
                continue;
            }
            self.sequence = sequence;
            if pid != self.pid {
                // SAFETY: read-only process wait handle, closed on replacement or drop.
                self.process = (pid != 0)
                    .then(|| {
                        // SAFETY: query-only process wait handle, closed on replacement/drop.
                        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
                            .ok()
                            .map(Handle)
                    })
                    .flatten();
                self.pid = pid;
            }
            ensure!(
                plates.iter().all(|p| p.valid()) && styles.iter().all(|s| s.valid()),
                "invalid glass snapshot"
            );
            let live = self.process.is_some();
            return Ok(Some(Snapshot {
                sequence,
                published_qpc,
                plates: if live { plates } else { Vec::new() },
                styles: if live { styles } else { Vec::new() },
            }));
        }
        if self.process.as_ref().is_some_and(|p| {
            // SAFETY: a valid wait handle; zero timeout is nonblocking.
            (unsafe { WaitForSingleObject(p.0, 0) }) == WAIT_OBJECT_0
        }) {
            self.process = None;
            return Ok(Some(Snapshot {
                sequence: self.sequence,
                published_qpc: qpc(),
                plates: Vec::new(),
                styles: Vec::new(),
            }));
        }
        Ok(None)
    }
}

pub fn qpc() -> u64 {
    let mut ticks = 0;
    // SAFETY: valid output pointer; QPC is available on every supported Windows version.
    unsafe { QueryPerformanceCounter(&mut ticks) }.expect("QueryPerformanceCounter");
    ticks as u64
}
pub fn qpc_frequency() -> u64 {
    let mut hz = 0;
    // SAFETY: valid output pointer.
    unsafe { QueryPerformanceFrequency(&mut hz) }.expect("QueryPerformanceFrequency");
    hz as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_publication_never_mixes_plate_generations() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Barrier};
        let name = format!("{CHANNEL}.concurrent.{}", std::process::id());
        let mut reader = Reader::named(&name).unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(2));
        let done = finished.clone();
        let hold = barrier.clone();
        let worker = std::thread::spawn(move || {
            let mut writer = Writer::named(&name).unwrap();
            for generation in 1..=2000 {
                let plates: Vec<_> = (0..MAX_PLATES)
                    .map(|id| Plate {
                        id: id as u64,
                        rect: [generation as f32, id as f32, 100.0, 80.0],
                        radius: 16.0,
                        material: Material::default(),
                    })
                    .collect();
                writer.publish(&plates).unwrap();
            }
            done.store(true, Ordering::Release);
            hold.wait(); // retain the last nonempty snapshot until the reader has checked it
        });
        let mut seen = 0;
        loop {
            if let Some(snapshot) = reader.read_changed().unwrap()
                && !snapshot.plates.is_empty()
            {
                let generation = snapshot.plates[0].rect[0];
                assert_eq!(snapshot.plates.len(), MAX_PLATES);
                for (id, plate) in snapshot.plates.iter().enumerate() {
                    assert_eq!(plate.rect, [generation, id as f32, 100.0, 80.0]);
                    assert_eq!(plate.id, id as u64);
                }
                seen += 1;
            }
            if finished.load(Ordering::Acquire) {
                break;
            }
        }
        // If the writer won the scheduler, its retained final generation is still readable.
        if seen == 0 {
            assert_eq!(
                reader.read_changed().unwrap().unwrap().plates.len(),
                MAX_PLATES
            );
        }
        barrier.wait();
        worker.join().unwrap();
        assert!(reader.read_changed().unwrap().unwrap().plates.is_empty());
    }
    #[test]
    fn styles_travel_with_their_snapshot_and_default_without_one() {
        let name = format!("{CHANNEL}.styles.{}", std::process::id());
        let mut reader = Reader::named(&name).unwrap();
        let mut writer = Writer::named(&name).unwrap();
        let plates: Vec<_> = (0..2)
            .map(|id| Plate {
                id,
                rect: [0.0, 40.0 * id as f32, 100.0, 40.0],
                radius: 8.0,
                material: Material::default(),
            })
            .collect();
        let styled = Style {
            brightness: 0.8,
            saturation: 1.6,
            smoothing: 0.6,
            sheen: Some(0.3),
            depth: None,
            inner_shadow: 0.8,
        };
        writer
            .publish_styled(&plates, &[styled, Style::default()])
            .unwrap();
        let snapshot = reader.read_changed().unwrap().unwrap();
        assert_eq!(snapshot.styles, [styled, Style::default()]);
        assert_eq!(
            size_of::<Styles>(),
            1928,
            "v1's five-word stride never changes"
        );
        // A pre-extension writer touches only the original section. An unmatched shadow
        // stamp defaults only the new field, retaining all of its existing style controls.
        let sequence = writer.publish_styled(&plates, &[styled; 2]).unwrap();
        writer
            .mapping
            .inner_shadows()
            .sequence
            .store(sequence - 2, Ordering::Relaxed);
        assert_eq!(
            reader.read_changed().unwrap().unwrap().styles,
            [Style {
                inner_shadow: 0.0,
                ..styled
            }; 2]
        );
        // A plain publish (or a writer that predates styles) leaves the old stamp: defaults.
        writer.publish(&plates).unwrap();
        let snapshot = reader.read_changed().unwrap().unwrap();
        assert_eq!(snapshot.styles, [Style::default(); 2]);
        let bad = Style {
            saturation: 4.0,
            ..styled
        };
        assert!(writer.publish_styled(&plates, &[bad, styled]).is_err());
        assert!(
            writer
                .publish_styled(
                    &plates,
                    &[Style {
                        inner_shadow: f32::NAN,
                        ..styled
                    }; 2]
                )
                .is_err()
        );
        assert!(
            writer
                .publish_styled(
                    &plates,
                    &[Style {
                        inner_shadow: 1.01,
                        ..styled
                    }; 2]
                )
                .is_err()
        );
        assert!(
            writer.publish_styled(&plates, &[styled]).is_err(),
            "one per plate"
        );
    }

    #[test]
    fn blur_takes_the_bars_full_range_at_high_dpi() {
        let plate = |blur| Plate {
            id: 1,
            rect: [0.0, 0.0, 100.0, 40.0],
            radius: 8.0,
            material: Material {
                blur,
                ..Material::default()
            },
        };
        // 64 DIP (the bars' largest blur) at 150 % and at 400 %.
        assert!(plate(96.0).valid() && plate(MAX_BLUR).valid());
        assert!(!plate(MAX_BLUR + 1.0).valid() && !plate(-1.0).valid());
    }

    #[test]
    fn snapshot_replaces_geometry_without_interpolation_and_drop_clears_it() {
        let name = format!("{CHANNEL}.test.{}", std::process::id());
        let mut reader = Reader::named(&name).unwrap();
        let mut writer = Writer::named(&name).unwrap();
        assert!(Writer::named(&name).is_err());
        let mut plate = Plate {
            id: u64::MAX,
            rect: [-2560.5, 9.0, 300.0, 200.0],
            radius: 22.0,
            material: Material::default(),
        };
        writer.publish(&[plate]).unwrap();
        assert_eq!(reader.read_changed().unwrap().unwrap().plates, [plate]);
        assert!(reader.read_changed().unwrap().is_none());
        plate.rect[0] = 107.25;
        writer.publish(&[plate]).unwrap();
        assert_eq!(reader.read_changed().unwrap().unwrap().plates, [plate]);
        assert!(writer.publish(&[plate, plate]).is_err());
        plate.rect[0] = f32::NAN;
        assert!(writer.publish(&[plate]).is_err());
        drop(writer);
        assert!(reader.read_changed().unwrap().unwrap().plates.is_empty());
    }
}
