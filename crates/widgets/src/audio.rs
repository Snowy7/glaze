//! `audio`: the spectrum of whatever the default output device plays (docs/widgets.md,
//! "Audio spectrum").
//!
//! Three layers:
//!
//! - **Math** (pure, tested): a radix-2 [`Fft`], a Hann-windowed [`Analyzer`] that turns the
//!   mono mix into [`BINS`] log-spaced levels in `0..1` with attack/decay smoothing, sample
//!   format conversion ([`to_mono`]) and [`resample`] to a subscriber's own bin count.
//! - **Lifecycle** ([`Audio`], pure and tested through the [`Engine`] trait): capture runs only
//!   while a *visible* instance subscribes and the host isn't held; otherwise there is no
//!   thread and no audio client at all.
//! - **Capture** ([`Wasapi`] and the thread behind it): WASAPI loopback of the default render
//!   endpoint, polled once per frame (~33 ms). A silent device delivers no packets, so silence
//!   is also simulated by feeding zeros; the bars then decay to zero, one all-zero frame is
//!   sent, and the thread drops to a slow poll until sound returns.

use std::collections::{BTreeSet, HashMap};
use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

use crate::bridge::Out;
use crate::wake::{Msg, Waker};

/// Samples per FFT.
pub const FFT_SIZE: usize = 2048;
/// Bins the analyzer produces; subscribers get a [`resample`]d view of them.
pub const BINS: usize = 128;
/// One spectrum: [`BINS`] levels in `0..1`, lowest frequency first.
pub type Frame = [f32; BINS];

const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;
/// The level that maps to 0; 0 dB (a full-scale sine) maps to 1.
const FLOOR_DB: f32 = -70.0;
/// Time between frames while sound plays: ~30 per second.
const FRAME: Duration = Duration::from_millis(33);
/// Time between polls once the spectrum is silent and has been sent as zeros.
const IDLE_POLL: Duration = Duration::from_millis(100);
/// How often the capture thread looks at which device is the default.
const DEVICE_CHECK: Duration = Duration::from_secs(1);
/// How much of a level's way up to the new value is covered per frame.
const ATTACK: f32 = 0.7;
/// How much of its level is kept per frame when the sound drops.
const DECAY: f32 = 0.9;
/// Levels below this snap to zero, so silence really ends.
const SNAP: f32 = 0.002;
/// The shared-mode buffer, in 100 ns units: room for ~200 ms between polls.
const BUFFER_100NS: i64 = 2_000_000;

// --- FFT -----------------------------------------------------------------------

/// A radix-2 FFT of a fixed power-of-two size, with its tables precomputed.
pub struct Fft {
    n: usize,
    reverse: Vec<u32>,
    /// `exp(-2πik/n)` for `k < n/2`, as `(cos, -sin)`.
    twiddle: Vec<(f32, f32)>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(
            n.is_power_of_two() && n >= 2,
            "FFT size must be a power of two"
        );
        let bits = n.trailing_zeros();
        Self {
            n,
            reverse: (0..n as u32)
                .map(|i| i.reverse_bits() >> (32 - bits))
                .collect(),
            twiddle: (0..n / 2)
                .map(|k| {
                    let angle = TAU * k as f32 / n as f32;
                    (angle.cos(), -angle.sin())
                })
                .collect(),
        }
    }

    /// Transforms `re + i·im` in place.
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        assert!(re.len() == self.n && im.len() == self.n);
        for (i, &j) in self.reverse.iter().enumerate() {
            let j = j as usize;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= self.n {
            let (half, step) = (len / 2, self.n / len);
            for start in (0..self.n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = self.twiddle[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

// --- Bins ------------------------------------------------------------------------

/// The `BINS + 1` edges (Hz) of the log-spaced bins at `rate`: 40 Hz up to 16 kHz, or a bit
/// under Nyquist when the device's rate is low.
pub fn bin_edges(rate: f32) -> [f32; BINS + 1] {
    let high = HIGH_HZ.min(rate * 0.45);
    let ratio = (high / LOW_HZ).ln();
    std::array::from_fn(|i| LOW_HZ * (ratio * i as f32 / BINS as f32).exp())
}

/// How one bin reads the FFT magnitudes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Span {
    /// The max over these FFT bins (inclusive).
    Max(usize, usize),
    /// Narrower than one FFT bin: interpolate between `k` and `k + 1`.
    Lerp(usize, f32),
}

fn spans(rate: f32) -> Vec<Span> {
    let edges = bin_edges(rate);
    let width = rate / FFT_SIZE as f32;
    let last = FFT_SIZE / 2;
    (0..BINS)
        .map(|i| {
            let (a, b) = (edges[i] / width, edges[i + 1] / width);
            if b - a >= 1.0 {
                Span::Max(
                    (a.ceil() as usize).min(last),
                    (b.floor() as usize).min(last),
                )
            } else {
                let centre = (a + b) / 2.0;
                let k = (centre.floor() as usize).min(last - 1);
                Span::Lerp(k, centre - k as f32)
            }
        })
        .collect()
}

/// Magnitude (1.0 for a full-scale sine) to a level in `0..1`.
fn level(magnitude: f32) -> f32 {
    let db = 20.0 * magnitude.max(1e-9).log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Attack-fast, decay-slow smoothing of the levels, per frame.
#[derive(Debug, Clone, PartialEq)]
struct Smoother {
    levels: Frame,
}

impl Smoother {
    fn new() -> Self {
        Self {
            levels: [0.0; BINS],
        }
    }

    /// Moves towards `target`; true while any level is above zero.
    fn step(&mut self, target: &Frame) -> bool {
        let mut audible = false;
        for (level, &target) in self.levels.iter_mut().zip(target) {
            *level = if target > *level {
                *level + (target - *level) * ATTACK
            } else {
                *level * DECAY + target * (1.0 - DECAY)
            };
            if *level < SNAP {
                *level = 0.0;
            }
            audible |= *level > 0.0;
        }
        audible
    }
}

/// A [`Frame`] seen at `bins` bins (the subscriber's own resolution): the max of the source
/// bins each output bin covers.
pub fn resample(frame: &Frame, bins: usize) -> Vec<f32> {
    (0..bins)
        .map(|j| {
            let (from, to) = (j * BINS / bins, ((j + 1) * BINS).div_ceil(bins).min(BINS));
            frame[from..to.max(from + 1)]
                .iter()
                .copied()
                .fold(0.0, f32::max)
        })
        .collect()
}

/// Mono samples in, spectrum frames out. One per capture session, for one sample rate.
pub struct Analyzer {
    fft: Fft,
    window: Vec<f32>,
    spans: Vec<Span>,
    /// The last [`FFT_SIZE`] mono samples, circular.
    ring: Vec<f32>,
    write: usize,
    /// Samples since the last non-silent chunk, capped at [`FFT_SIZE`].
    silent_run: usize,
    smoother: Smoother,
    sent_zero: bool,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Analyzer {
    pub fn new(rate: u32) -> Self {
        Self {
            fft: Fft::new(FFT_SIZE),
            window: (0..FFT_SIZE)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / FFT_SIZE as f32).cos())
                .collect(),
            spans: spans(rate as f32),
            ring: vec![0.0; FFT_SIZE],
            write: 0,
            silent_run: FFT_SIZE,
            smoother: Smoother::new(),
            sent_zero: false,
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
        }
    }

    pub fn push(&mut self, mono: &[f32]) {
        for &sample in mono {
            self.ring[self.write] = sample;
            self.write = (self.write + 1) % FFT_SIZE;
        }
        if mono.iter().any(|&s| s != 0.0) {
            self.silent_run = 0;
        } else {
            self.silent_run = (self.silent_run + mono.len()).min(FFT_SIZE);
        }
    }

    /// `count` samples of silence (a device that delivers nothing, or flagged packets).
    pub fn push_silence(&mut self, count: usize) {
        for _ in 0..count.min(FFT_SIZE) {
            self.ring[self.write] = 0.0;
            self.write = (self.write + 1) % FFT_SIZE;
        }
        self.silent_run = (self.silent_run + count).min(FFT_SIZE);
    }

    /// The spectrum of the ring, as levels.
    fn spectrum(&mut self) -> Frame {
        for i in 0..FFT_SIZE {
            self.re[i] = self.ring[(self.write + i) % FFT_SIZE] * self.window[i];
        }
        self.im.fill(0.0);
        self.fft.forward(&mut self.re, &mut self.im);
        // A full-scale sine under a Hann window peaks at N/4.
        let scale = 4.0 / FFT_SIZE as f32;
        let magnitude = |k: usize| self.re[k].hypot(self.im[k]) * scale;
        std::array::from_fn(|i| match self.spans[i] {
            Span::Max(from, to) => level((from..=to).map(magnitude).fold(0.0, f32::max)),
            Span::Lerp(k, t) => level(magnitude(k) * (1.0 - t) + magnitude(k + 1) * t),
        })
    }

    /// The next frame to send, or `None` when there is nothing new to say: once the levels
    /// have decayed to zero, one all-zero frame goes out and then nothing until sound returns.
    pub fn frame(&mut self) -> Option<Frame> {
        let target = if self.silent_run >= FFT_SIZE {
            [0.0; BINS] // no FFT of a silent ring
        } else {
            self.spectrum()
        };
        if self.smoother.step(&target) {
            self.sent_zero = false;
            Some(self.smoother.levels)
        } else if !self.sent_zero {
            self.sent_zero = true;
            Some([0.0; BINS])
        } else {
            None
        }
    }

    /// Silent, and said so: the capture loop can poll slowly.
    pub fn idle(&self) -> bool {
        self.sent_zero && self.silent_run >= FFT_SIZE
    }
}

// --- Sample formats ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    F32,
    I16,
    I24,
    I32,
}

impl Sample {
    fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 | Self::I32 => 4,
        }
    }

    fn decode(self, b: &[u8]) -> f32 {
        match self {
            Self::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Self::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
            Self::I24 => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
            Self::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        }
    }
}

/// An interleaved little-endian PCM or float stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub rate: u32,
    pub channels: usize,
    pub sample: Sample,
}

const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

impl Format {
    /// From a `WAVEFORMATEX` tag (and, for `WAVE_FORMAT_EXTENSIBLE`, the first word of the
    /// sub-format GUID, which is the tag it stands for).
    fn from_parts(tag: u16, sub_format: u32, bits: u16, channels: u16, rate: u32) -> Option<Self> {
        let tag = if tag == TAG_EXTENSIBLE {
            u16::try_from(sub_format).ok()?
        } else {
            tag
        };
        let sample = match (tag, bits) {
            (TAG_FLOAT, 32) => Sample::F32,
            (TAG_PCM, 16) => Sample::I16,
            (TAG_PCM, 24) => Sample::I24,
            (TAG_PCM, 32) => Sample::I32,
            _ => return None,
        };
        (channels >= 1 && rate > 0).then_some(Self {
            rate,
            channels: usize::from(channels),
            sample,
        })
    }

    fn frame_bytes(self) -> usize {
        self.channels * self.sample.bytes()
    }

    /// # Safety
    ///
    /// `wave` points at a valid `WAVEFORMATEX` (and a `WAVEFORMATEXTENSIBLE` when its tag
    /// says so), as returned by `IAudioClient::GetMixFormat`.
    unsafe fn from_wave(wave: *const WAVEFORMATEX) -> Option<Self> {
        // SAFETY: per the contract; the structs are packed, so fields are copied, not borrowed.
        unsafe {
            let (tag, bits, channels, rate) = (
                (*wave).wFormatTag,
                (*wave).wBitsPerSample,
                (*wave).nChannels,
                (*wave).nSamplesPerSec,
            );
            let sub_format = if tag == TAG_EXTENSIBLE {
                (*wave.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat.data1
            } else {
                0
            };
            Self::from_parts(tag, sub_format, bits, channels, rate)
        }
    }
}

/// Appends the mono mix (the mean of the channels) of interleaved `bytes` to `out`.
pub fn to_mono(bytes: &[u8], format: Format, out: &mut Vec<f32>) {
    let width = format.sample.bytes();
    let channels = format.channels as f32;
    out.extend(bytes.chunks_exact(format.frame_bytes()).map(|frame| {
        frame
            .chunks_exact(width)
            .map(|sample| format.sample.decode(sample))
            .sum::<f32>()
            / channels
    }));
}

// --- Lifecycle ---------------------------------------------------------------------

/// What capture is: started with the generation its frames will carry, and stopped.
pub trait Engine {
    fn start(&mut self, generation: u64);
    fn stop(&mut self);
}

struct Sub {
    instance: String,
    /// The widget's own id for the subscription.
    sub: String,
    bins: usize,
}

/// Every `audio` subscription and when capture runs for them: only while one belongs to a
/// visible instance and the host isn't held.
pub struct Audio<E> {
    engine: E,
    subs: Vec<Sub>,
    visible: BTreeSet<String>,
    held: bool,
    running: bool,
    /// Bumped on every start, so a frame from a capture that was stopped is ignored.
    generation: u64,
}

impl<E: Engine> Audio<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            subs: Vec::new(),
            visible: BTreeSet::new(),
            held: false,
            running: false,
            generation: 0,
        }
    }

    /// Starts (or replaces) subscription `sub`.
    pub fn subscribe(&mut self, instance: &str, sub: &str, bins: usize) {
        self.subs
            .retain(|s| !(s.instance == instance && s.sub == sub));
        self.subs.push(Sub {
            instance: instance.to_owned(),
            sub: sub.to_owned(),
            bins,
        });
        self.sync();
    }

    pub fn unsubscribe(&mut self, instance: &str, sub: &str) {
        self.subs
            .retain(|s| !(s.instance == instance && s.sub == sub));
        self.sync();
    }

    /// Drops everything an instance subscribed (it reloaded or went away).
    pub fn forget(&mut self, instance: &str) {
        self.subs.retain(|s| s.instance != instance);
        self.sync();
    }

    /// Which instances are on screen and running right now.
    pub fn set_visible(&mut self, visible: BTreeSet<String>) {
        if visible != self.visible {
            self.visible = visible;
            self.sync();
        }
    }

    pub fn set_held(&mut self, held: bool) {
        self.held = held;
        self.sync();
    }

    fn wanted(&self) -> bool {
        !self.held && self.subs.iter().any(|s| self.visible.contains(&s.instance))
    }

    fn sync(&mut self) {
        match (self.wanted(), self.running) {
            (true, false) => {
                self.generation += 1;
                self.engine.start(self.generation);
                self.running = true;
            }
            (false, true) => {
                self.engine.stop();
                self.running = false;
            }
            _ => {}
        }
    }

    /// Whether capture is running (tests, logging).
    #[cfg(test)]
    fn running(&self) -> bool {
        self.running
    }

    /// The events for one frame from capture `generation`: each visible subscriber gets it at
    /// its own bin count, rounded to three decimals.
    pub fn on_frame(&self, generation: u64, frame: &Frame) -> Vec<Out> {
        if !self.running || generation != self.generation {
            return Vec::new();
        }
        let mut by_bins: HashMap<usize, Value> = HashMap::new();
        self.subs
            .iter()
            .filter(|s| self.visible.contains(&s.instance))
            .map(|s| {
                let value = by_bins.entry(s.bins).or_insert_with(|| {
                    Value::Array(resample(frame, s.bins).into_iter().map(round3).collect())
                });
                Out::Event {
                    instance: s.instance.clone(),
                    event: "sub",
                    data: json!({ "sub": s.sub, "value": value }),
                }
            })
            .collect()
    }
}

/// Three decimals keep the JSON small; rounded in f64 so it prints as `0.123`.
fn round3(level: f32) -> Value {
    json!((f64::from(level) * 1000.0).round() / 1000.0)
}

// --- Capture -------------------------------------------------------------------------

/// How long to wait before opening the device again after `failures` failures in a row:
/// 1 s, doubling, up to 10 s.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(4)).min(10))
}

/// What the capture thread used.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunStats {
    /// Kernel plus user time of the thread, in 100 ns units.
    pub cpu_100ns: u64,
    pub frames: u64,
}

fn thread_cpu_100ns() -> u64 {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // SAFETY: the out-pointers are valid FILETIMEs for the call.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok.is_ok() {
        ticks(kernel) + ticks(user)
    } else {
        0
    }
}

/// The capture thread. Dropping this stops it; the thread then releases the audio client
/// by itself.
pub struct Capture {
    /// Set to tell the thread to stop; it notices within one frame.
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<RunStats>>,
}

impl Capture {
    /// Starts capturing the default output device; `sink` gets every frame, on the capture
    /// thread.
    pub fn spawn(sink: impl FnMut(Box<Frame>) + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(&flag, sink))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stops, waits for the thread, and reports what it used (for the cost measurement).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finish(mut self) -> Option<RunStats> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take()?.join().ok()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sleeps until `until` (precisely: `thread::sleep` uses a high-resolution timer, where a
/// channel timeout would round up to the 15.6 ms system tick), in slices short enough to
/// notice `stop`. True if told to stop.
fn nap_until(stop: &AtomicBool, until: Instant) -> bool {
    const SLICE: Duration = Duration::from_millis(100);
    loop {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(left.min(SLICE));
    }
}

/// The real [`Engine`]: frames go to the main loop as [`Msg::Audio`].
pub struct Wasapi {
    waker: Waker,
    capture: Option<Capture>,
}

impl Wasapi {
    pub fn new(waker: Waker) -> Self {
        Self {
            waker,
            capture: None,
        }
    }
}

impl Engine for Wasapi {
    fn start(&mut self, generation: u64) {
        let waker = self.waker.clone();
        let sink = move |frame| waker.send(Msg::Audio { generation, frame });
        match Capture::spawn(sink) {
            Ok(capture) => self.capture = Some(capture),
            Err(e) => log::warn!("couldn't start audio capture: {e}"),
        }
    }

    fn stop(&mut self) {
        self.capture = None;
    }
}

enum Exit {
    Stopped,
    /// The default device is another one now.
    DeviceChanged,
    Failed(windows::core::Error),
}

fn run(stop: &AtomicBool, mut sink: impl FnMut(Box<Frame>)) -> RunStats {
    // SAFETY: COM for this thread; balanced by CoUninitialize below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut stats = RunStats::default();
    let mut failures = 0u32;
    loop {
        let exit = match Session::open() {
            Ok(mut session) => {
                log::debug!("audio: capturing {:?}", session.format);
                session.pump(stop, &mut sink, &mut stats)
            }
            Err(e) => Exit::Failed(e),
        };
        let wait = match exit {
            Exit::Stopped => break,
            Exit::DeviceChanged => {
                log::info!("audio: the default output device changed");
                failures = 0;
                Duration::ZERO
            }
            Exit::Failed(e) => {
                failures += 1;
                let wait = backoff(failures);
                log::info!("audio: capture failed ({e}); retrying in {wait:?}");
                wait
            }
        };
        if nap_until(stop, Instant::now() + wait) {
            break;
        }
    }
    stats.cpu_100ns = thread_cpu_100ns();
    if com {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
    stats
}

/// An open loopback stream on the default render device. Dropping it stops the stream and
/// releases the audio client.
struct Session {
    enumerator: IMMDeviceEnumerator,
    device_id: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: Format,
    analyzer: Analyzer,
    /// Mono samples of one packet.
    mono: Vec<f32>,
}

/// The id of the default render device.
fn default_device(enumerator: &IMMDeviceEnumerator) -> windows::core::Result<String> {
    // SAFETY: COM calls on a live enumerator; the id string is freed after copying.
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let id = device.GetId()?;
        let text = id.to_string();
        CoTaskMemFree(Some(id.as_ptr().cast()));
        Ok(text?)
    }
}

impl Session {
    fn open() -> windows::core::Result<Self> {
        // SAFETY: COM calls on objects created here; the mix format is freed on every path.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let id = device.GetId()?;
            let device_id = id.to_string();
            CoTaskMemFree(Some(id.as_ptr().cast()));
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let mix = client.GetMixFormat()?;
            let format = Format::from_wave(mix).ok_or_else(|| {
                windows::core::Error::new(
                    windows::Win32::Foundation::E_NOTIMPL,
                    "unsupported mix format",
                )
            });
            let opened = format.and_then(|format| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    BUFFER_100NS,
                    0,
                    mix,
                    None,
                )?;
                Ok(format)
            });
            CoTaskMemFree(Some(mix.cast()));
            let format = opened?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok(Self {
                enumerator,
                device_id: device_id?,
                client,
                capture,
                format,
                analyzer: Analyzer::new(format.rate),
                mono: Vec::new(),
            })
        }
    }

    /// Moves everything the device has buffered into the analyzer; false if there was none.
    fn drain(&mut self) -> windows::core::Result<bool> {
        let mut any = false;
        // SAFETY: COM calls on the live capture client; the buffer is copied out before
        // `ReleaseBuffer` hands it back.
        unsafe {
            while self.capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let count = frames as usize;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    self.analyzer.push_silence(count);
                } else {
                    let bytes = std::slice::from_raw_parts(data, count * self.format.frame_bytes());
                    self.mono.clear();
                    to_mono(bytes, self.format, &mut self.mono);
                    self.analyzer.push(&self.mono);
                }
                self.capture.ReleaseBuffer(frames)?;
                any = true;
            }
        }
        Ok(any)
    }

    /// One frame per [`FRAME`] (or per [`IDLE_POLL`] when silent) until told to stop, the
    /// device goes away, or another one becomes the default.
    fn pump(
        &mut self,
        stop: &AtomicBool,
        sink: &mut impl FnMut(Box<Frame>),
        stats: &mut RunStats,
    ) -> Exit {
        let mut last = Instant::now();
        let mut checked = last;
        let mut next = last;
        loop {
            let period = if self.analyzer.idle() {
                IDLE_POLL
            } else {
                FRAME
            };
            // A fixed rate, not "period after the work": no drift. A late wake-up (the PC was
            // asleep) doesn't try to catch up.
            next = (next + period).max(Instant::now());
            if nap_until(stop, next) {
                return Exit::Stopped;
            }
            let now = Instant::now();
            match self.drain() {
                Ok(true) => {}
                // A silent device delivers no packets: feed the time that passed as silence.
                Ok(false) => {
                    let samples = (now - last).as_secs_f32() * self.format.rate as f32;
                    self.analyzer.push_silence(samples as usize);
                }
                Err(e) => return Exit::Failed(e),
            }
            last = now;
            if let Some(frame) = self.analyzer.frame() {
                sink(Box::new(frame));
                stats.frames += 1;
            }
            if now - checked >= DEVICE_CHECK {
                checked = now;
                match default_device(&self.enumerator) {
                    Ok(id) if id != self.device_id => return Exit::DeviceChanged,
                    Ok(_) => {}
                    Err(e) => return Exit::Failed(e),
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the client we started; the COM objects release themselves after this.
        let _ = unsafe { self.client.Stop() };
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A deterministic pseudo-random sequence in -1..1.
    fn noise(count: usize) -> Vec<f32> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..count)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 40) as f32 / (1u64 << 23) as f32 - 1.0
            })
            .collect()
    }

    #[test]
    fn the_fft_matches_a_naive_dft() {
        let n = 64;
        let values = noise(2 * n);
        let (mut re, mut im) = (values[..n].to_vec(), values[n..].to_vec());
        let (input_re, input_im) = (re.clone(), im.clone());
        Fft::new(n).forward(&mut re, &mut im);
        for k in 0..n {
            let (mut sum_re, mut sum_im) = (0.0f64, 0.0f64);
            for t in 0..n {
                let angle = -std::f64::consts::TAU * (k * t) as f64 / n as f64;
                let (c, s) = (angle.cos(), angle.sin());
                let (x, y) = (f64::from(input_re[t]), f64::from(input_im[t]));
                sum_re += x * c - y * s;
                sum_im += x * s + y * c;
            }
            assert!((f64::from(re[k]) - sum_re).abs() < 1e-3, "re[{k}]");
            assert!((f64::from(im[k]) - sum_im).abs() < 1e-3, "im[{k}]");
        }
    }

    fn sine(rate: u32, hz: f32, amplitude: f32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| amplitude * (TAU * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    /// The bin a frequency falls in.
    fn bin_of(rate: f32, hz: f32) -> usize {
        let edges = bin_edges(rate);
        edges.iter().rposition(|&edge| edge <= hz).unwrap()
    }

    fn settled(analyzer: &mut Analyzer) -> Frame {
        let mut last = [0.0; BINS];
        for _ in 0..20 {
            last = analyzer.frame().unwrap_or(last);
        }
        last
    }

    #[test]
    fn a_pure_sine_lands_in_its_log_bin() {
        for (rate, hz) in [
            (48_000, 1_000.0),
            (44_100, 1_000.0),
            (48_000, 100.0),
            (48_000, 8_000.0),
            (96_000, 440.0),
        ] {
            let mut analyzer = Analyzer::new(rate);
            analyzer.push(&sine(rate, hz, 0.5, FFT_SIZE));
            let frame = settled(&mut analyzer);
            let peak = (0..BINS)
                .max_by(|&a, &b| frame[a].total_cmp(&frame[b]))
                .unwrap();
            let expected = bin_of(rate as f32, hz);
            assert!(
                peak.abs_diff(expected) <= 1,
                "{hz} Hz at {rate}: peak {peak}, expected {expected}"
            );
            assert!(
                frame[peak] > 0.7,
                "{hz} Hz at {rate}: level {}",
                frame[peak]
            );
            // Two octaves away from the tone there is (next to) nothing.
            let edges = bin_edges(rate as f32);
            for (i, &v) in frame.iter().enumerate() {
                if edges[i] > hz * 4.0 || edges[i + 1] < hz / 4.0 {
                    assert!(v < 0.35, "{hz} Hz at {rate}: bin {i} is {v}");
                }
            }
        }
    }

    #[test]
    fn bin_edges_are_monotonic_and_cover_the_range() {
        for rate in [22_050.0, 32_000.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let edges = bin_edges(rate);
            assert!((edges[0] - LOW_HZ).abs() < 0.01);
            assert!(edges.windows(2).all(|w| w[1] > w[0]), "{rate}");
            let high = edges[BINS];
            assert!(high <= HIGH_HZ + 1.0 && high < rate / 2.0, "{rate}: {high}");
            if rate >= 44_100.0 {
                assert!((high - HIGH_HZ).abs() < 1.0, "{rate}: {high}");
            }
            // Every bin reads valid FFT bins, wide ones a range, narrow ones an interpolation.
            for span in spans(rate) {
                match span {
                    Span::Max(from, to) => assert!(from <= to && to <= FFT_SIZE / 2),
                    Span::Lerp(k, t) => assert!(k < FFT_SIZE / 2 && (0.0..=1.0).contains(&t)),
                }
            }
        }
        // Low bins are narrower than an FFT bin (interpolated), high ones are wide.
        let at_48k = spans(48_000.0);
        assert!(matches!(at_48k[0], Span::Lerp(..)));
        assert!(matches!(at_48k[BINS - 1], Span::Max(from, to) if to > from));
    }

    #[test]
    fn levels_map_minus_70_to_0_db_onto_0_to_1() {
        assert_eq!(level(1.0), 1.0);
        assert_eq!(level(2.0), 1.0);
        assert!((level(0.1) - (50.0 / 70.0)).abs() < 1e-4, "-20 dB");
        assert_eq!(level(10f32.powf(-70.0 / 20.0) * 0.99), 0.0);
        assert_eq!(level(0.0), 0.0);
    }

    #[test]
    fn levels_attack_fast_and_decay_slowly_to_zero() {
        let mut smoother = Smoother::new();
        assert!(smoother.step(&[1.0; BINS]));
        assert!((smoother.levels[0] - ATTACK).abs() < 1e-6, "one frame up");
        let up = smoother.levels[0];
        smoother.step(&[1.0; BINS]);
        assert!(smoother.levels[0] > up);
        smoother.levels = [1.0; BINS];
        smoother.step(&[0.0; BINS]);
        assert!((smoother.levels[0] - DECAY).abs() < 1e-6, "one frame down");
        let frames = (1..200).find(|_| !smoother.step(&[0.0; BINS])).unwrap();
        assert!(frames < 100, "silence takes {frames} frames to end");
        assert_eq!(smoother.levels, [0.0; BINS]);
        // A level that only just falls below the target doesn't oscillate.
        smoother.step(&[0.5; BINS]);
        let before = smoother.levels[3];
        smoother.step(&[0.5; BINS]);
        assert!(smoother.levels[3] > before && smoother.levels[3] < 0.5);
    }

    #[test]
    fn silence_decays_then_sends_one_zero_frame_and_goes_quiet() {
        let mut analyzer = Analyzer::new(48_000);
        // Fresh and silent: one zero frame says so.
        assert_eq!(analyzer.frame(), Some([0.0; BINS]));
        assert_eq!(analyzer.frame(), None);
        assert!(analyzer.idle());

        analyzer.push(&sine(48_000, 1_000.0, 0.5, FFT_SIZE));
        assert!(!analyzer.idle());
        assert!(analyzer.frame().is_some());
        // The device falls silent and delivers nothing; the caller feeds silence.
        let mut sent = 0;
        let mut zero_frames = 0;
        for _ in 0..200 {
            analyzer.push_silence(1_600);
            match analyzer.frame() {
                Some(frame) if frame == [0.0; BINS] => zero_frames += 1,
                Some(_) => sent += 1,
                None => {}
            }
        }
        assert!(sent > 5 && sent < 100, "decaying frames: {sent}");
        assert_eq!(zero_frames, 1, "exactly one all-zero frame");
        assert!(analyzer.idle());
        // Sound returns.
        analyzer.push(&sine(48_000, 1_000.0, 0.5, 1_600));
        assert!(analyzer.frame().is_some_and(|f| f.iter().any(|&v| v > 0.0)));
        assert!(!analyzer.idle());
    }

    #[test]
    fn frames_resample_to_the_subscribers_bins() {
        let mut frame = [0.0; BINS];
        frame[5] = 0.8;
        frame[6] = 0.3;
        frame[127] = 1.0;
        assert_eq!(resample(&frame, 128), frame);
        let half = resample(&frame, 64);
        assert_eq!(half.len(), 64);
        assert_eq!(
            (half[2], half[3], half[63]),
            (0.8, 0.3, 1.0),
            "pairs take the max"
        );
        assert_eq!(half[2].max(half[3]), 0.8);
        let mut pair = [0.0; BINS];
        (pair[10], pair[11]) = (0.2, 0.9);
        assert_eq!(resample(&pair, 64)[5], 0.9);
        for bins in [64, 77, 100, 127, 128] {
            let out = resample(&frame, bins);
            assert_eq!(out.len(), bins);
            assert_eq!(*out.last().unwrap(), 1.0, "the top bin survives at {bins}");
            // A lone peak keeps (about) its place on the frequency axis.
            let mut lone = [0.0; BINS];
            lone[64] = 1.0;
            let at = resample(&lone, bins)
                .iter()
                .position(|&v| v == 1.0)
                .unwrap();
            assert!(at.abs_diff(64 * bins / BINS) <= 1, "{bins}: {at}");
            assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn interleaved_samples_become_mono() {
        let f = |channels, sample| Format {
            rate: 48_000,
            channels,
            sample,
        };
        let mut out = Vec::new();
        let floats: Vec<u8> = [0.5f32, -0.5, 1.0, 1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        to_mono(&floats, f(2, Sample::F32), &mut out);
        assert_eq!(out, [0.0, 1.0]);

        out.clear();
        let i16s: Vec<u8> = [16_384i16, 16_384, -32_768, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        to_mono(&i16s, f(2, Sample::I16), &mut out);
        assert_eq!(out, [0.5, -0.5]);

        out.clear();
        // 24-bit: 0x400000 is half scale, 0x800000 is -1.
        to_mono(
            &[0x00, 0x00, 0x40, 0x00, 0x00, 0x80],
            f(1, Sample::I24),
            &mut out,
        );
        assert_eq!(out, [0.5, -1.0]);

        out.clear();
        let i32s: Vec<u8> = [i32::MIN, 0x4000_0000]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        to_mono(&i32s, f(1, Sample::I32), &mut out);
        assert_eq!(out, [-1.0, 0.5]);

        // A trailing partial frame is ignored.
        out.clear();
        to_mono(&[0, 0, 0, 0, 0, 0], f(2, Sample::I16), &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn mix_formats_are_recognised() {
        let float = Format::from_parts(TAG_FLOAT, 0, 32, 2, 48_000).unwrap();
        assert_eq!(
            (float.sample, float.channels, float.rate),
            (Sample::F32, 2, 48_000)
        );
        // WAVE_FORMAT_EXTENSIBLE names the real format in its sub-format GUID.
        assert_eq!(
            Format::from_parts(TAG_EXTENSIBLE, 3, 32, 8, 44_100)
                .unwrap()
                .sample,
            Sample::F32
        );
        assert_eq!(
            Format::from_parts(TAG_EXTENSIBLE, 1, 24, 2, 96_000)
                .unwrap()
                .sample,
            Sample::I24
        );
        assert_eq!(
            Format::from_parts(TAG_PCM, 0, 16, 1, 8_000).unwrap().sample,
            Sample::I16
        );
        for bad in [
            Format::from_parts(TAG_PCM, 0, 8, 2, 48_000),
            Format::from_parts(TAG_FLOAT, 0, 64, 2, 48_000),
            Format::from_parts(TAG_EXTENSIBLE, 0x1_0000, 32, 2, 48_000),
            Format::from_parts(0x55, 0, 16, 2, 48_000),
            Format::from_parts(TAG_PCM, 0, 16, 0, 48_000),
        ] {
            assert_eq!(bad, None);
        }
    }

    #[test]
    fn the_retry_backs_off_from_one_to_ten_seconds() {
        let secs: Vec<u64> = (1..=8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 10, 10, 10, 10]);
        assert_eq!(backoff(1000).as_secs(), 10);
    }

    // --- The lifecycle, without any audio device ---

    #[derive(Clone, Default)]
    struct Log(Rc<RefCell<Vec<String>>>);

    impl Engine for Log {
        fn start(&mut self, generation: u64) {
            self.0.borrow_mut().push(format!("start {generation}"));
        }

        fn stop(&mut self) {
            self.0.borrow_mut().push("stop".into());
        }
    }

    fn audio() -> (Audio<Log>, Log) {
        let log = Log::default();
        (Audio::new(log.clone()), log)
    }

    fn visible(instances: &[&str]) -> BTreeSet<String> {
        instances.iter().map(|s| (*s).to_owned()).collect()
    }

    fn events(log: &Log) -> Vec<String> {
        log.0.borrow().clone()
    }

    #[test]
    fn capture_starts_with_the_first_visible_subscriber_and_stops_with_the_last() {
        let (mut audio, log) = audio();
        audio.set_visible(visible(&["viz-1", "viz-2"]));
        assert!(events(&log).is_empty(), "nobody listens yet");
        audio.subscribe("viz-1", "a", 64);
        audio.subscribe("viz-2", "b", 100);
        audio.subscribe("viz-2", "c", 128);
        assert_eq!(events(&log), ["start 1"], "one capture for everyone");
        audio.unsubscribe("viz-1", "a");
        audio.unsubscribe("viz-2", "b");
        assert!(audio.running(), "viz-2 still listens");
        audio.unsubscribe("viz-2", "c");
        assert_eq!(events(&log), ["start 1", "stop"]);
        audio.subscribe("viz-1", "a", 64);
        assert_eq!(events(&log).last().unwrap(), "start 2", "a new generation");
    }

    #[test]
    fn subscribers_of_hidden_instances_do_not_count() {
        let (mut audio, log) = audio();
        audio.subscribe("viz-1", "a", 64);
        assert!(events(&log).is_empty(), "not visible: no capture");
        audio.set_visible(visible(&["viz-1"]));
        assert_eq!(events(&log), ["start 1"]);
        // Covered, paused or on a hidden layer: gone from the visible set.
        audio.set_visible(visible(&[]));
        assert_eq!(events(&log), ["start 1", "stop"]);
        // Another instance being visible doesn't start it.
        audio.set_visible(visible(&["clock-1"]));
        assert_eq!(events(&log).len(), 2);
        audio.set_visible(visible(&["viz-1", "clock-1"]));
        assert_eq!(events(&log).len(), 3);
    }

    #[test]
    fn a_held_host_does_not_capture_and_resumes_with_its_subscribers() {
        let (mut audio, log) = audio();
        audio.set_held(true);
        audio.set_visible(visible(&["viz-1"]));
        audio.subscribe("viz-1", "a", 64);
        assert!(events(&log).is_empty(), "held: no capture");
        audio.set_held(false);
        assert_eq!(events(&log), ["start 1"]);
        audio.set_held(true);
        assert_eq!(events(&log), ["start 1", "stop"]);
        audio.set_held(true);
        audio.set_held(false);
        assert_eq!(events(&log).len(), 3, "the subscription survived the hold");
    }

    #[test]
    fn a_reloaded_frame_or_a_removed_instance_drops_its_subscriptions() {
        let (mut audio, log) = audio();
        audio.set_visible(visible(&["viz-1", "viz-2"]));
        audio.subscribe("viz-1", "a", 64);
        audio.subscribe("viz-1", "b", 64);
        audio.subscribe("viz-2", "a", 64);
        audio.forget("viz-1"); // its `hello`
        assert!(audio.running(), "viz-2 remains");
        audio.forget("viz-2");
        assert_eq!(events(&log), ["start 1", "stop"]);
        // Subscribing again with the same id replaces, it doesn't pile up.
        audio.subscribe("viz-1", "a", 64);
        audio.subscribe("viz-1", "a", 128);
        audio.unsubscribe("viz-1", "a");
        assert_eq!(events(&log).last().unwrap(), "stop");
    }

    #[test]
    fn frames_go_to_visible_subscribers_at_their_resolution() {
        let (mut audio, _) = audio();
        audio.set_visible(visible(&["viz-1", "viz-2"]));
        audio.subscribe("viz-1", "a", 64);
        audio.subscribe("viz-2", "b", 100);
        audio.subscribe("hidden-1", "c", 128);
        let mut frame = [0.0; BINS];
        frame[0] = 0.12345;
        let out = audio.on_frame(1, &frame);
        assert_eq!(out.len(), 2, "the hidden instance gets nothing");
        let Out::Event {
            instance,
            event,
            data,
        } = &out[0]
        else {
            panic!()
        };
        assert_eq!((instance.as_str(), *event), ("viz-1", "sub"));
        assert_eq!(data["sub"], "a");
        let value = data["value"].as_array().unwrap();
        assert_eq!(value.len(), 64);
        assert_eq!(value[0], json!(0.123), "three decimals");
        let Out::Event { data, .. } = &out[1] else {
            panic!()
        };
        assert_eq!(data["value"].as_array().unwrap().len(), 100);
        // Frames from a capture that was stopped are ignored.
        assert!(audio.on_frame(7, &frame).is_empty());
        audio.set_held(true);
        assert!(audio.on_frame(1, &frame).is_empty());
    }

    /// Real capture for `MYPAPER_AUDIO_SECS` (default 15) seconds: reports the capture
    /// thread's CPU as a share of one core, and the frames it produced. Run it while audio
    /// plays, and again in silence:
    /// `cargo test --release -p mypaper-widgets measure_capture_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "uses the real audio device and takes seconds"]
    fn measure_capture_cost() {
        let secs: u64 = std::env::var("MYPAPER_AUDIO_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        let peak = std::sync::Arc::new(std::sync::Mutex::new(0.0f32));
        let seen = peak.clone();
        let capture = Capture::spawn(move |frame| {
            let top = frame.iter().copied().fold(0.0, f32::max);
            if let Ok(mut peak) = seen.lock() {
                *peak = peak.max(top);
            }
        })
        .unwrap();
        let started = Instant::now();
        std::thread::sleep(Duration::from_secs(secs));
        let wall = started.elapsed();
        let stats = capture.finish().unwrap();
        let cpu = Duration::from_nanos(stats.cpu_100ns * 100);
        println!(
            "audio capture: {} frames in {:.1}s ({:.1}/s), thread CPU {:.1} ms = {:.3}% of one core, loudest level {:.2}",
            stats.frames,
            wall.as_secs_f32(),
            stats.frames as f32 / wall.as_secs_f32(),
            cpu.as_secs_f64() * 1000.0,
            100.0 * cpu.as_secs_f64() / wall.as_secs_f64(),
            peak.lock().map(|p| *p).unwrap_or(0.0),
        );
    }
}
