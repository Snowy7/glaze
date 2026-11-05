//! One shell publishes complete snapshots in physical virtual-desktop pixels. Motion belongs
//! to the shell. The engine waits on `Reader::wake_event` and the writer's process handle.
//! A seqlock protects the snapshot; payload words are atomic too, so concurrent reads never
//! create Rust data races. The session-local mapping is 5.7 KiB, with no files or polling.

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
const WORDS: usize = 15;
const CHANNEL: &str = "Local\\mypaper.glass.v1";

/// A plate's Liquid Glass material (plan, "Material controls"). The engine draws it as
/// liquid-rust's `Glass.regular` with these knobs (`crates/wallpaper/src/material.rs`,
/// `docs/glass.md`). All values except blur are in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Clear (1) to tinted (0): iOS 27's Liquid Glass slider, reversed.
    pub clarity: f32,
    /// Frost, physical pixels (points once divided by the screen's scale).
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
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
            && (0.0..=32.0).contains(&m.blur)
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

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Mapping {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    _mapping: Handle,
    wake: Handle,
}
impl Mapping {
    fn open(name: &str) -> Result<Self> {
        // SAFETY: a page-file mapping starts zeroed (valid atomic representations), is aligned
        // to an allocation boundary, and stays mapped until Drop. Named v1 fixes the ABI.
        unsafe {
            let mapping = Handle(CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size_of::<Shared>() as u32,
                &HSTRING::from(name),
            )?);
            let view = MapViewOfFile(mapping.0, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<Shared>());
            ensure!(
                !view.Value.is_null(),
                "MapViewOfFile: {}",
                windows::core::Error::from_thread()
            );
            let wake =
                match CreateEventW(None, false, false, &HSTRING::from(format!("{name}.wake"))) {
                    Ok(handle) => Handle(handle),
                    Err(error) => {
                        let _ = UnmapViewOfFile(view);
                        return Err(error.into());
                    }
                };
            Ok(Self {
                view,
                _mapping: mapping,
                wake,
            })
        }
    }
    fn shared(&self) -> &Shared {
        // SAFETY: the mapping contains a correctly aligned Shared for our whole lifetime.
        unsafe { &*self.view.Value.cast::<Shared>() }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this view belongs to us and is no longer referenced.
        let _ = unsafe { UnmapViewOfFile(self.view) };
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
        self.publish_as(plates, std::process::id())
    }
    fn publish_as(&mut self, plates: &[Plate], pid: u32) -> Result<u64> {
        ensure!(
            plates.len() <= MAX_PLATES,
            "at most {MAX_PLATES} glass plates"
        );
        for (i, plate) in plates.iter().enumerate() {
            ensure!(plate.valid(), "invalid glass plate {}", plate.id);
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
        let _ = self.publish_as(&[], 0);
        // SAFETY: the creating thread owns the lease. Writer is intentionally not Send.
        let _ = unsafe { ReleaseMutex(self.lease.0) };
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub sequence: u64,
    pub published_qpc: u64,
    pub plates: Vec<Plate>,
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
            ensure!(plates.iter().all(|p| p.valid()), "invalid glass snapshot");
            return Ok(Some(Snapshot {
                sequence,
                published_qpc,
                plates: if self.process.is_some() {
                    plates
                } else {
                    Vec::new()
                },
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
