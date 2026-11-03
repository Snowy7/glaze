//! The video decode worker: Media Foundation's source reader on a D3D11
//! device (DXVA hardware decode on the minimum-power GPU, the same one
//! wgpu's `LowPower` picks), sending tightly packed NV12 frames to the
//! engine over a small bounded channel. Every COM object lives and dies on
//! the worker thread.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11CreateDevice,
    ID3D11Device, ID3D11Multithread, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_GPU_PREFERENCE_MINIMUM_POWER, IDXGIAdapter1, IDXGIFactory6,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFDXGIBuffer, IMFMediaBuffer, IMFSample, IMFSourceReader, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE,
    MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_FLAG,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MF2DBuffer_LockFlags_Read, MFCreateAttributes,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video,
    MFSTARTUP_LITE, MFShutdown, MFStartup, MFVideoArea, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{GUID, HSTRING, Interface};

use super::Color;

/// Media Foundation's time unit (100 ns) per second.
pub const TICKS: i64 = 10_000_000;
/// Frames decoded ahead of the engine. When the channel is full the worker
/// blocks, so a paused wallpaper stops decoding within this many frames.
const AHEAD: usize = 2;
/// Opening a local file and decoding one frame takes well under this.
const START_TIMEOUT: Duration = Duration::from_secs(10);
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// One decoded frame, tightly packed NV12: `width × height` luma bytes,
/// then rows of interleaved chroma at half resolution.
pub struct Nv12 {
    /// Presentation time on the looped timeline (it never jumps back), in [`TICKS`].
    pub time: i64,
    pub size: (u32, u32),
    pub color: Color,
    pub data: Vec<u8>,
}

/// The engine's end of a running decoder.
pub struct Stream {
    pub frames: Receiver<Nv12>,
    /// Takes spent frame buffers back, so steady playback allocates nothing.
    pub recycle: SyncSender<Vec<u8>>,
    /// The video's own frame rate.
    pub fps: f64,
    pub thread: Thread,
}

/// Joins the worker on drop. Drop it after [`Stream::frames`]: the closed
/// channel is what stops the worker.
pub struct Thread(Option<JoinHandle<()>>);

impl Drop for Thread {
    fn drop(&mut self) {
        if let Some(thread) = self.0.take() {
            let _ = thread.join();
        }
    }
}

/// Starts decoding `path` on a worker thread. Returns once the first frame
/// is decoded, so a missing or undecodable file fails here, not later.
pub fn spawn(path: PathBuf, fps_cap: u32, speed: f32) -> Result<Stream> {
    let (frames_tx, frames) = mpsc::sync_channel(AHEAD);
    let (recycle, spent) = mpsc::sync_channel(AHEAD + 2);
    let (ready_tx, ready) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("video decode".into())
        .spawn(move || {
            let mut ready = Some(ready_tx);
            if let Err(e) = decode(&path, fps_cap, speed, &mut ready, &frames_tx, &spent) {
                match ready.take() {
                    Some(ready) => _ = ready.send(Err(e)),
                    None => log::warn!("video decoding stopped, keeping the last frame: {e:#}"),
                }
            }
        })?;
    // On failure `thread` is detached rather than joined: it may be stuck.
    let fps = ready
        .recv_timeout(START_TIMEOUT)
        .context("the video decoder did not start")??;
    Ok(Stream {
        frames,
        recycle,
        fps,
        thread: Thread(Some(thread)),
    })
}

/// The worker loop. Returns `Ok` when the engine hangs up.
fn decode(
    path: &Path,
    fps_cap: u32,
    speed: f32,
    ready: &mut Option<SyncSender<Result<f64>>>,
    frames: &SyncSender<Nv12>,
    spent: &Receiver<Vec<u8>>,
) -> Result<()> {
    let mut decoder = Decoder::open(path)?;
    let mut pacing = Pacing::new(fps_cap, speed, decoder.format.frame_ticks());
    loop {
        let (time, sample) = decoder.next()?;
        if !pacing.keep(time) {
            continue; // above the fps cap: never copied off the GPU
        }
        let frame = decoder.copy(time, sample, spent.try_recv().unwrap_or_default())?;
        let (width, height) = frame.size;
        if frames.send(frame).is_err() {
            return Ok(());
        }
        if let Some(ready) = ready.take() {
            log::info!(
                "video: {width}x{height} at {:.3} fps, {} decode on {}",
                decoder.format.fps,
                if decoder.hardware {
                    "hardware"
                } else {
                    "software"
                },
                decoder.adapter,
            );
            let _ = ready.send(Ok(decoder.format.fps));
        }
    }
}

/// A source reader that outputs NV12 for the first video stream.
struct Decoder {
    reader: IMFSourceReader,
    format: Format,
    timeline: Timeline,
    adapter: String,
    /// The last frame came from GPU memory (DXVA).
    hardware: bool,
    // Last, so Media Foundation shuts down after the reader is released.
    _mf: MediaFoundation,
}

impl Decoder {
    fn open(path: &Path) -> Result<Self> {
        let mf = MediaFoundation::start()?;
        let (device, adapter) = decode_device()?;
        let (mut token, mut manager, mut attributes) = (0, None, None);
        // SAFETY: COM calls on live interfaces; the out-pointers are valid locals.
        let reader = unsafe {
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.context("no DXGI device manager")?;
            manager.ResetDevice(&device, token)?;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.context("no attributes")?;
            attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            MFCreateSourceReaderFromURL(&HSTRING::from(path), &attributes)
        }
        .with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: COM calls on a live reader with valid arguments.
        unsafe {
            // Video only: audio is never demuxed or decoded.
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader
                .SetStreamSelection(VIDEO, true)
                .context("the file has no video stream")?;
            let nv12 = MFCreateMediaType()?;
            nv12.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            nv12.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            reader
                .SetCurrentMediaType(VIDEO, None, &nv12)
                .context("no decoder for this video format")?;
        }
        Ok(Self {
            format: Format::read(&reader)?,
            reader,
            timeline: Timeline::default(),
            adapter,
            hardware: false,
            _mf: mf,
        })
    }

    /// Decodes the next frame, looping at the end of the file. Returns it
    /// with its time on the continuous timeline.
    fn next(&mut self) -> Result<(i64, IMFSample)> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            // SAFETY: the out-pointers are valid locals.
            unsafe {
                self.reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut time),
                    Some(&mut sample),
                )
            }?;
            let has = |flag: MF_SOURCE_READER_FLAG| flags & flag.0 as u32 != 0;
            ensure!(!has(MF_SOURCE_READERF_ERROR), "the decoder failed");
            if has(MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED) {
                self.format = Format::read(&self.reader)?;
            }
            let placed = sample.map(|sample| {
                // SAFETY: a COM call on a live sample.
                let duration = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
                let duration = if duration > 0 {
                    duration
                } else {
                    self.format.frame_ticks()
                };
                (self.timeline.place(time, duration), sample)
            });
            if has(MF_SOURCE_READERF_ENDOFSTREAM) {
                if !self.timeline.wrap() {
                    bail!("the video has no frames");
                }
                // SAFETY: a COM call on a live reader; both pointers outlive it.
                unsafe {
                    self.reader
                        .SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(0i64))
                }
                .context("seeking back to the start")?;
            }
            if let Some(placed) = placed {
                return Ok(placed);
            }
        }
    }

    /// Copies a frame off the GPU (or out of system memory) into `data`.
    fn copy(&mut self, time: i64, sample: IMFSample, mut data: Vec<u8>) -> Result<Nv12> {
        // SAFETY: a COM call on a live sample.
        let buffer = unsafe { sample.GetBufferByIndex(0) }?;
        let gpu_rows = match buffer.cast::<IMFDXGIBuffer>() {
            Ok(gpu) => Some(texture_rows(&gpu)?),
            Err(_) => None,
        };
        self.hardware = gpu_rows.is_some();
        let format = &self.format;
        let locked = Locked::new(&buffer, format.stride)?;
        let rows = gpu_rows.unwrap_or(format.rows);
        pack_nv12(locked.bytes(), locked.pitch, rows, format.area, &mut data)?;
        Ok(Nv12 {
            time,
            size: (format.area.width as u32, format.area.height as u32),
            color: format.color,
            data,
        })
    }
}

/// The reader's current output type.
struct Format {
    /// What to show: the display aperture inside the (padded) coded frame.
    area: Area,
    /// Luma rows of a system-memory frame (GPU frames report their own).
    rows: usize,
    /// Row pitch of a system-memory buffer without 2D locking.
    stride: usize,
    color: Color,
    fps: f64,
}

impl Format {
    fn read(reader: &IMFSourceReader) -> Result<Self> {
        let mut aperture = MFVideoArea::default();
        // SAFETY: COM calls on a live reader. The blob read writes at most
        // `size_of::<MFVideoArea>()` bytes, into `aperture`.
        unsafe {
            let current = reader.GetCurrentMediaType(VIDEO)?;
            // Container tags such as the YUV matrix often live only on the native type.
            let native = reader.GetNativeMediaType(VIDEO, 0).ok();
            let tag = |key| {
                (current.GetUINT32(key).ok())
                    .or_else(|| native.as_ref().and_then(|t| t.GetUINT32(key).ok()))
            };
            let (width, height) = split(current.GetUINT64(&MF_MT_FRAME_SIZE)?);
            let (width, height) = (width as usize, height as usize);
            let full = Area {
                x: 0,
                y: 0,
                width,
                height,
            };
            let bytes = std::slice::from_raw_parts_mut(
                (&raw mut aperture).cast::<u8>(),
                size_of::<MFVideoArea>(),
            );
            let area = match current.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None) {
                Ok(()) => Area {
                    x: aperture.OffsetX.value.max(0) as usize,
                    y: aperture.OffsetY.value.max(0) as usize,
                    width: aperture.Area.cx.max(0) as usize,
                    height: aperture.Area.cy.max(0) as usize,
                },
                Err(_) => full,
            };
            let area = if area.width > 0
                && area.height > 0
                && area.x + area.width <= width
                && area.y + area.height <= height
            {
                area
            } else {
                full
            };
            // MF_MT_DEFAULT_STRIDE is a signed value stored as a UINT32.
            let stride = (current.GetUINT32(&MF_MT_DEFAULT_STRIDE).ok())
                .map(|s| s as i32)
                .filter(|&s| s > 0)
                .map_or(width, |s| s as usize);
            let fps = match current.GetUINT64(&MF_MT_FRAME_RATE).map(split) {
                Ok((num, den)) if num > 0 && den > 0 => f64::from(num) / f64::from(den),
                _ => 30.0,
            };
            Ok(Self {
                area,
                rows: height,
                stride,
                color: Color::from_tags(
                    tag(&MF_MT_YUV_MATRIX),
                    tag(&MF_MT_VIDEO_NOMINAL_RANGE),
                    area.height,
                ),
                fps,
            })
        }
    }

    /// The nominal frame duration, in [`TICKS`].
    fn frame_ticks(&self) -> i64 {
        (TICKS as f64 / self.fps) as i64
    }
}

/// Media Foundation packs sizes and ratios as `high << 32 | low`.
fn split(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// A video-capable D3D11 device on the minimum-power GPU, safe for Media
/// Foundation's own threads to use. Returns it with the adapter's name.
fn decode_device() -> Result<(ID3D11Device, String)> {
    let mut device = None;
    // SAFETY: DXGI/D3D11 calls with valid arguments; `device` is a valid out-pointer.
    unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory1()?;
        let adapter: IDXGIAdapter1 =
            factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER)?;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let device: ID3D11Device = device.context("no D3D11 device")?;
        // Returns the previous setting, which does not matter.
        let _ = (device.GetImmediateContext()?)
            .cast::<ID3D11Multithread>()?
            .SetMultithreadProtected(true);
        let name = adapter.GetDesc1()?.Description;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Ok((device, String::from_utf16_lossy(&name[..len])))
    }
}

/// The allocated height of the texture behind a GPU buffer: the chroma
/// plane starts after this many rows, which may be more than the frame has.
fn texture_rows(buffer: &IMFDXGIBuffer) -> Result<usize> {
    let mut texture = null_mut();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: on success GetResource hands over one reference to an
    // ID3D11Texture2D, which `from_raw` adopts (and releases on drop).
    unsafe {
        buffer.GetResource(&ID3D11Texture2D::IID, &mut texture)?;
        ID3D11Texture2D::from_raw(texture).GetDesc(&mut desc);
    }
    Ok(desc.Height as usize)
}

/// COM (multithreaded) and Media Foundation on this thread; shut down on drop.
struct MediaFoundation;

impl MediaFoundation {
    fn start() -> Result<Self> {
        // SAFETY: balanced by CoUninitialize, in `drop` or below.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        // SAFETY: balanced by MFShutdown in `drop`.
        if let Err(e) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) } {
            // SAFETY: balances the CoInitializeEx above.
            unsafe { CoUninitialize() };
            return Err(e.into());
        }
        Ok(Self)
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // SAFETY: balances `start`; this thread's MF objects are released by now.
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// A sample buffer locked for reading, unlocked on drop.
struct Locked {
    buffer: Lockable,
    data: *const u8,
    len: usize,
    pitch: usize,
}

enum Lockable {
    Planar(IMF2DBuffer2),
    Linear(IMFMediaBuffer),
}

impl Locked {
    /// Prefers a read-only 2D lock: it reports the real pitch, and a GPU
    /// buffer then skips copying the data back to the texture on unlock.
    fn new(buffer: &IMFMediaBuffer, linear_pitch: usize) -> Result<Self> {
        if let Ok(planar) = buffer.cast::<IMF2DBuffer2>() {
            let (mut scan0, mut pitch, mut start, mut len) = (null_mut(), 0, null_mut(), 0);
            // SAFETY: the out-pointers are valid locals.
            unsafe {
                planar.Lock2DSize(
                    MF2DBuffer_LockFlags_Read,
                    &mut scan0,
                    &mut pitch,
                    &mut start,
                    &mut len,
                )
            }?;
            // Built before any check, so the buffer is unlocked whatever happens.
            let locked = Self {
                buffer: Lockable::Planar(planar),
                data: scan0,
                len: (start.addr() + len as usize).saturating_sub(scan0.addr()),
                pitch: pitch.max(0) as usize,
            };
            ensure!(pitch > 0, "bottom-up NV12 is not supported");
            return Ok(locked);
        }
        let (mut data, mut len) = (null_mut(), 0);
        // SAFETY: the out-pointers are valid locals.
        unsafe { buffer.Lock(&mut data, None, Some(&mut len)) }?;
        Ok(Self {
            buffer: Lockable::Linear(buffer.clone()),
            data,
            len: len as usize,
            pitch: linear_pitch,
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: while locked (until `self` drops) the buffer's `len` bytes
        // at `data` are valid and unchanged.
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: balances the successful lock in `new`.
        let _ = unsafe {
            match &self.buffer {
                Lockable::Planar(buffer) => buffer.Unlock2D(),
                Lockable::Linear(buffer) => buffer.Unlock(),
            }
        };
    }
}

/// A rectangle of a frame, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Area {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

/// Packs the visible `area` of an NV12 image into `out` without padding:
/// luma rows, then chroma rows. In `src` rows are `pitch` bytes apart and
/// the chroma plane starts after `rows` luma rows.
fn pack_nv12(src: &[u8], pitch: usize, rows: usize, area: Area, out: &mut Vec<u8>) -> Result<()> {
    let row = |start: usize, len: usize| {
        (src.get(start..start + len)).context("the NV12 buffer is smaller than its frame")
    };
    let (chroma_width, chroma_rows) = (area.width.div_ceil(2) * 2, area.height.div_ceil(2));
    out.clear();
    out.reserve(area.width * area.height + chroma_width * chroma_rows);
    for y in area.y..area.y + area.height {
        out.extend_from_slice(row(y * pitch + area.x, area.width)?);
    }
    let chroma = rows * pitch + (area.x & !1);
    for y in area.y / 2..area.y / 2 + chroma_rows {
        out.extend_from_slice(row(chroma + y * pitch, chroma_width)?);
    }
    Ok(())
}

/// Maps each pass's sample times onto one continuous timeline: a pass
/// starts exactly where the previous one ended.
#[derive(Debug, Default)]
struct Timeline {
    /// Where the current pass starts on the timeline.
    base: i64,
    /// The current pass's first sample time and its latest end time.
    pass: Option<(i64, i64)>,
}

impl Timeline {
    fn place(&mut self, time: i64, duration: i64) -> i64 {
        let (start, end) = self.pass.get_or_insert((time, time));
        *end = (*end).max(time + duration);
        self.base + time - *start
    }

    /// Starts the next pass. False if this one had no frames to loop.
    fn wrap(&mut self) -> bool {
        let Some((start, end)) = self.pass.take() else {
            return false;
        };
        self.base += end - start;
        true
    }
}

/// Picks the frames to show under the fps cap; the rest are never copied
/// off the GPU. Kept frames follow a fixed grid, so a 50 fps video capped
/// at 30 shows 30 frames a second, not 25.
struct Pacing {
    interval: i64,
    slack: i64,
    next: Option<i64>,
}

impl Pacing {
    /// `frame` is the video's frame duration. All times are video time, in [`TICKS`].
    fn new(fps_cap: u32, speed: f32, frame: i64) -> Self {
        Self {
            interval: (TICKS as f64 * f64::from(speed) / f64::from(fps_cap)) as i64,
            slack: frame / 2,
            next: None,
        }
    }

    fn keep(&mut self, time: i64) -> bool {
        if self.next.is_some_and(|next| time < next - self.slack) {
            return false;
        }
        let next = self.next.unwrap_or(time) + self.interval;
        // Fell behind the grid (slow video or a gap): restart it here.
        self.next = Some(if next - self.slack <= time {
            time + self.interval
        } else {
            next
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_the_visible_area_and_skips_pitch_padding() {
        // A 4x2 area at (2, 2) of a frame with 4 luma rows and an 8-byte
        // pitch: luma bytes hold 10*row + column, chroma 100 + 10*row + byte.
        let (pitch, rows) = (8, 4);
        let mut src = vec![0u8; pitch * (rows + rows / 2)];
        for y in 0..rows {
            for x in 0..pitch {
                src[y * pitch + x] = (10 * y + x) as u8;
            }
        }
        for y in 0..rows / 2 {
            for x in 0..pitch {
                src[(rows + y) * pitch + x] = (100 + 10 * y + x) as u8;
            }
        }
        let area = Area {
            x: 2,
            y: 2,
            width: 4,
            height: 2,
        };
        let mut out = Vec::new();
        pack_nv12(&src, pitch, rows, area, &mut out).unwrap();
        #[rustfmt::skip]
        assert_eq!(out, [
            22, 23, 24, 25,
            32, 33, 34, 35,
            112, 113, 114, 115, // chroma row 1: the luma rows 2-3
        ]);
        assert!(pack_nv12(&src[..pitch * rows], pitch, rows, area, &mut out).is_err());
    }

    #[test]
    fn loops_onto_a_continuous_timeline() {
        let mut timeline = Timeline::default();
        // The first pass starts at 500 (a container offset): it maps to 0.
        assert_eq!(timeline.place(500, 100), 0);
        assert_eq!(timeline.place(600, 100), 100);
        assert!(timeline.wrap());
        // The next pass continues right after the last frame.
        assert_eq!(timeline.place(500, 100), 200);
        assert!(timeline.wrap());
        assert!(!timeline.wrap(), "a pass without frames cannot loop");
    }

    #[test]
    fn keeps_frames_up_to_the_fps_cap() {
        let kept = |fps: i64, cap: u32, speed: f32| {
            let mut pacing = Pacing::new(cap, speed, TICKS / fps);
            (0..fps).filter(|i| pacing.keep(i * TICKS / fps)).count()
        };
        // One second of video each.
        assert_eq!(kept(60, 30, 1.0), 30);
        assert_eq!(kept(50, 30, 1.0), 30);
        assert_eq!(kept(30, 30, 1.0), 30);
        assert_eq!(kept(24, 30, 1.0), 24);
        // At 2x, a second of video plays in half a second: 15 frames at 30 fps.
        assert_eq!(kept(30, 30, 2.0), 15);
    }
}
