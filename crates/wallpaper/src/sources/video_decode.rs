//! The video decode worker: Media Foundation's source reader on a D3D11
//! device (DXVA hardware decode on wgpu's GPU), sending frames to the
//! engine over a small bounded channel. Frames are NV12, or P010 for
//! 10-bit video ([`Layout`]). They stay on the GPU when wgpu can share
//! textures with this device ([`super::share`]); otherwise they are copied
//! to system memory, tightly packed. Every COM object lives and dies on the
//! worker thread.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use windows::Win32::Foundation::{HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11CreateDevice,
    ID3D11Device, ID3D11Multithread, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_GPU_PREFERENCE_MINIMUM_POWER, IDXGIAdapter1, IDXGIFactory6,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFDXGIBuffer, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_FLAG,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MF2DBuffer_LockFlags_Read, MFCreateAttributes,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video,
    MFSTARTUP_LITE, MFShutdown, MFStartup, MFVideoArea, MFVideoFormat_AV1, MFVideoFormat_H265,
    MFVideoFormat_HEVC, MFVideoFormat_VP90, MFVideoTransFunc_2084, MFVideoTransFunc_HLG,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{GUID, HSTRING, Interface};

use super::share::{Handle, SLOTS, Slots, Writer};
use super::{Color, Layout, Tags};

/// Media Foundation's time unit (100 ns) per second.
pub const TICKS: i64 = 10_000_000;
/// Frames decoded ahead of the engine. When the channel is full the worker
/// blocks, so a paused wallpaper stops decoding within this many frames.
const AHEAD: usize = 2;
/// Opening a local file and decoding one frame takes well under this.
const START_TIMEOUT: Duration = Duration::from_secs(10);
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// One decoded 4:2:0 frame (`color.layout` says NV12 or P010).
pub struct Yuv {
    /// Presentation time on the looped timeline (it never jumps back), in [`TICKS`].
    pub time: i64,
    pub size: (u32, u32),
    pub color: Color,
    pub pixels: Pixels,
}

pub enum Pixels {
    /// Tightly packed in system memory: `width × height` luma samples, then
    /// rows of interleaved chroma at half resolution.
    Cpu(Vec<u8>),
    /// In shared slot `slot`, once its `written` fence reaches `ready`.
    Gpu { slot: usize, ready: u64 },
}

impl Pixels {
    /// Hands the pixels back unshown (a slot that wgpu never sampled).
    pub fn spent(self) -> Spent {
        match self {
            Self::Cpu(data) => Spent::Buffer(data),
            Self::Gpu { slot, .. } => Spent::Slot { slot, after: 0 },
        }
    }
}

/// Storage the engine is done with, going back to the worker for reuse.
pub enum Spent {
    Buffer(Vec<u8>),
    /// Free once wgpu's `released` fence reaches `after` (0: already free).
    Slot {
        slot: usize,
        after: u64,
    },
}

/// Where to decode: on wgpu's adapter (`None`: the minimum-power one) and,
/// for zero-copy, with the fence wgpu signals when it releases a slot.
pub struct Target {
    pub adapter: Option<LUID>,
    pub released: Option<Handle>,
}

/// The worker's startup report.
struct Ready {
    fps: f64,
    layout: Layout,
    slots: Option<Slots>,
}

/// The engine's end of a running decoder.
pub struct Stream {
    pub frames: Receiver<Yuv>,
    /// Takes spent buffers and slots back, so steady playback allocates nothing.
    pub recycle: SyncSender<Spent>,
    /// The video's own frame rate.
    pub fps: f64,
    /// How the frames are stored: 8-bit NV12 or 10-bit P010.
    pub layout: Layout,
    /// Shared slots, when the frames stay on the GPU.
    pub slots: Option<Slots>,
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
pub fn spawn(path: PathBuf, fps_cap: u32, speed: f32, target: Target) -> Result<Stream> {
    let (frames_tx, frames) = mpsc::sync_channel(AHEAD);
    // Room for every slot and buffer in flight, so handing one back never fails.
    let (recycle, returned) = mpsc::sync_channel(SLOTS + AHEAD);
    let (ready_tx, ready) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("video decode".into())
        .spawn(move || {
            let mut ready = Some(ready_tx);
            let spares = Spares {
                returned,
                buffers: Vec::new(),
                slots: (0..SLOTS).map(|slot| (slot, 0)).collect(),
            };
            let result = decode(
                &path, fps_cap, speed, target, &mut ready, &frames_tx, spares,
            );
            if let Err(e) = result {
                match ready.take() {
                    Some(ready) => _ = ready.send(Err(e)),
                    None => log::warn!("video decoding stopped, keeping the last frame: {e:#}"),
                }
            }
        })?;
    // On failure `thread` is detached rather than joined: it may be stuck.
    let Ready { fps, layout, slots } = ready
        .recv_timeout(START_TIMEOUT)
        .context("the video decoder did not start")??;
    Ok(Stream {
        frames,
        recycle,
        fps,
        layout,
        slots,
        thread: Thread(Some(thread)),
    })
}

/// The worker loop. Returns `Ok` when the engine hangs up.
fn decode(
    path: &Path,
    fps_cap: u32,
    speed: f32,
    target: Target,
    ready: &mut Option<SyncSender<Result<Ready>>>,
    frames: &SyncSender<Yuv>,
    mut spares: Spares,
) -> Result<()> {
    let mut decoder = Decoder::open(path, target.adapter)?;
    let mut pacing = Pacing::new(fps_cap, speed, decoder.format.frame_ticks());
    let mut released = target.released;
    let (mut writer, mut slots) = (None, None);
    loop {
        let (time, sample) = decoder.next()?;
        if !pacing.keep(time) {
            continue; // above the fps cap: never copied off the GPU
        }
        let decoded = Decoded::new(&sample)?;
        let area = decoder.format.area;
        let (origin, size) = (
            (area.x as u32, area.y as u32),
            (area.width as u32, area.height as u32),
        );
        // Decide on the first frame: zero-copy if it is on the GPU and wgpu takes shared slots.
        if let (Some(released), Some(_)) = (released.take(), &decoded.texture) {
            match Writer::new(
                &decoder.device,
                released,
                decoder.format.color.layout,
                origin,
                size,
            ) {
                Ok((created, shared)) => (writer, slots) = (Some(created), Some(shared)),
                Err(e) => log::warn!("zero-copy video unavailable, copying frames: {e:#}"),
            }
        }
        let (pixels, size) = match (&mut writer, &decoded.texture) {
            (Some(writer), Some((texture, index)))
                if writer.fits(decoder.format.color.layout, origin, size) =>
            {
                let Some((slot, after)) = spares.slot() else {
                    return Ok(());
                };
                let ready = writer.write(texture, *index, slot, after)?;
                (Pixels::Gpu { slot, ready }, writer.size)
            }
            _ => {
                let data = decoder.copy(&decoded, spares.buffer())?;
                (Pixels::Cpu(data), size)
            }
        };
        let mode = match (&pixels, &decoded.texture) {
            (Pixels::Gpu { .. }, _) => "hardware decode, zero-copy",
            (Pixels::Cpu(_), Some(_)) => "hardware decode, copied through memory",
            (Pixels::Cpu(_), None) => "software decode",
        };
        // Back to the decoder's pool before we (maybe) block on a full channel.
        drop((decoded, sample));
        let frame = Yuv {
            time,
            size,
            color: decoder.format.color,
            pixels,
        };
        if frames.send(frame).is_err() {
            return Ok(());
        }
        if let Some(ready) = ready.take() {
            let (fps, adapter) = (decoder.format.fps, &decoder.adapter);
            let color = decoder.format.color;
            log::info!(
                "video: {}x{} at {fps:.3} fps, {:?} ({:?}, {:?}), {mode} on {adapter}",
                size.0,
                size.1,
                color.layout,
                color.transfer,
                color.gamut,
            );
            let _ = ready.send(Ok(Ready {
                fps,
                layout: color.layout,
                slots: slots.take(),
            }));
        }
    }
}

/// Buffers and slots the engine handed back, ready for reuse.
struct Spares {
    returned: Receiver<Spent>,
    buffers: Vec<Vec<u8>>,
    /// Free slots, with the `released` value to wait for before writing.
    slots: Vec<(usize, u64)>,
}

impl Spares {
    fn keep(&mut self, spent: Spent) {
        match spent {
            Spent::Buffer(buffer) => self.buffers.push(buffer),
            Spent::Slot { slot, after } => self.slots.push((slot, after)),
        }
    }

    fn collect(&mut self) {
        while let Ok(spent) = self.returned.try_recv() {
            self.keep(spent);
        }
    }

    fn buffer(&mut self) -> Vec<u8> {
        self.collect();
        self.buffers.pop().unwrap_or_default()
    }

    /// A free slot, waiting for one to come back if need be. `None` once
    /// the engine has hung up.
    fn slot(&mut self) -> Option<(usize, u64)> {
        loop {
            self.collect();
            if let Some(slot) = self.slots.pop() {
                return Some(slot);
            }
            let spent = self.returned.recv().ok()?;
            self.keep(spent);
        }
    }
}

/// A source reader that outputs NV12 (or P010, for 10-bit video) for the
/// first video stream.
struct Decoder {
    reader: IMFSourceReader,
    device: ID3D11Device,
    format: Format,
    timeline: Timeline,
    adapter: String,
    // Last, so Media Foundation shuts down after the reader is released.
    _mf: MediaFoundation,
}

impl Decoder {
    fn open(path: &Path, adapter: Option<LUID>) -> Result<Self> {
        let mf = MediaFoundation::start()?;
        let (device, adapter) = decode_device(adapter)?;
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
            // 8-bit video decodes to NV12, the common path. 10-bit video asks
            // for P010 first: some decoders accept NV12 for it too and
            // silently drop the extra bits.
            let output = |layout: Layout| {
                let output = MFCreateMediaType()?;
                output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                output.SetGUID(&MF_MT_SUBTYPE, &layout.subtype())?;
                reader.SetCurrentMediaType(VIDEO, None, &output)
            };
            let ten_bit = reader
                .GetNativeMediaType(VIDEO, 0)
                .is_ok_and(|native| is_ten_bit(&native));
            let order = if ten_bit {
                [Layout::P010, Layout::Nv12]
            } else {
                [Layout::Nv12, Layout::P010]
            };
            output(order[0])
                .or_else(|_| output(order[1]))
                .context("no decoder for this video format")?;
        }
        Ok(Self {
            format: Format::read(&reader)?,
            reader,
            device,
            timeline: Timeline::default(),
            adapter,
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

    /// Copies a frame off the GPU (or out of system memory) into `data`,
    /// tightly packed.
    fn copy(&self, frame: &Decoded, mut data: Vec<u8>) -> Result<Vec<u8>> {
        let format = &self.format;
        // The chroma plane starts after the texture's allocated rows, which
        // may be more than the frame has.
        let rows = match &frame.texture {
            Some((texture, _)) => {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                // SAFETY: a D3D11 call on a live texture; `desc` is a valid out-pointer.
                unsafe { texture.GetDesc(&mut desc) };
                desc.Height as usize
            }
            None => format.rows,
        };
        let locked = Locked::new(&frame.buffer, format.stride)?;
        let bytes = format.color.layout.bytes_per_sample();
        pack_yuv420(
            locked.bytes(),
            locked.pitch,
            rows,
            format.area,
            bytes,
            &mut data,
        )?;
        Ok(data)
    }
}

/// Whether a stream's native type is 10-bit video: a 4:2:0 10-bit HEVC, VP9
/// or AV1 profile, or any stream tagged with an HDR transfer function.
fn is_ten_bit(native: &IMFMediaType) -> bool {
    // The profile numbering of all three codecs puts 4:2:0 10-bit at 2.
    const MAIN_420_10: u32 = 2;
    // SAFETY: COM calls on a live media type.
    unsafe {
        let subtype = native.GetGUID(&MF_MT_SUBTYPE).ok();
        let tens = [
            MFVideoFormat_HEVC,
            MFVideoFormat_H265,
            MFVideoFormat_VP90,
            MFVideoFormat_AV1,
        ];
        let profile = native.GetUINT32(&MF_MT_MPEG2_PROFILE).ok();
        let transfer = native.GetUINT32(&MF_MT_TRANSFER_FUNCTION).ok();
        (subtype.is_some_and(|s| tens.contains(&s)) && profile == Some(MAIN_420_10))
            || [MFVideoTransFunc_2084, MFVideoTransFunc_HLG]
                .iter()
                .any(|hdr| transfer == Some(hdr.0 as u32))
    }
}

/// A decoded sample's buffer and, for a hardware frame, the D3D11 texture
/// (and array slice) it lives in.
struct Decoded {
    buffer: IMFMediaBuffer,
    texture: Option<(ID3D11Texture2D, u32)>,
}

impl Decoded {
    fn new(sample: &IMFSample) -> Result<Self> {
        // SAFETY: a COM call on a live sample.
        let buffer = unsafe { sample.GetBufferByIndex(0) }?;
        let texture = match buffer.cast::<IMFDXGIBuffer>() {
            Ok(gpu) => {
                let mut texture = null_mut();
                // SAFETY: on success GetResource hands over one reference to
                // an ID3D11Texture2D, which `from_raw` adopts.
                unsafe {
                    gpu.GetResource(&ID3D11Texture2D::IID, &mut texture)?;
                    Some((
                        ID3D11Texture2D::from_raw(texture),
                        gpu.GetSubresourceIndex()?,
                    ))
                }
            }
            Err(_) => None,
        };
        Ok(Self { buffer, texture })
    }
}

/// The reader's current output type.
struct Format {
    /// What to show: the display aperture inside the (padded) coded frame.
    area: Area,
    /// Luma rows of a system-memory frame (GPU frames report their own).
    rows: usize,
    /// Row pitch, in bytes, of a system-memory buffer without 2D locking.
    stride: usize,
    /// How its samples map to RGB, including the storage layout.
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
            let layout = current
                .GetGUID(&MF_MT_SUBTYPE)
                .ok()
                .and_then(|subtype| Layout::from_subtype(&subtype))
                .context("the decoder output is neither NV12 nor P010")?;
            // MF_MT_DEFAULT_STRIDE is a signed value stored as a UINT32.
            let stride = (current.GetUINT32(&MF_MT_DEFAULT_STRIDE).ok())
                .map(|s| s as i32)
                .filter(|&s| s > 0)
                .map_or(width * layout.bytes_per_sample(), |s| s as usize);
            let fps = match current.GetUINT64(&MF_MT_FRAME_RATE).map(split) {
                Ok((num, den)) if num > 0 && den > 0 => f64::from(num) / f64::from(den),
                _ => 30.0,
            };
            Ok(Self {
                area,
                rows: height,
                stride,
                color: Color::from_tags(
                    layout,
                    Tags {
                        matrix: tag(&MF_MT_YUV_MATRIX),
                        range: tag(&MF_MT_VIDEO_NOMINAL_RANGE),
                        transfer: tag(&MF_MT_TRANSFER_FUNCTION),
                        primaries: tag(&MF_MT_VIDEO_PRIMARIES),
                    },
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

/// A video-capable D3D11 device on `adapter` (wgpu's GPU) or else the
/// minimum-power GPU (the one wgpu's `LowPower` picks), safe for Media
/// Foundation's own threads to use. Returns it with the adapter's name.
fn decode_device(adapter: Option<LUID>) -> Result<(ID3D11Device, String)> {
    let mut device = None;
    // SAFETY: DXGI/D3D11 calls with valid arguments; `device` is a valid out-pointer.
    unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory1()?;
        let adapter: IDXGIAdapter1 = match adapter {
            Some(luid) => factory.EnumAdapterByLuid(luid)?,
            None => factory.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_MINIMUM_POWER)?,
        };
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
            ensure!(pitch > 0, "bottom-up video frames are not supported");
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

/// Packs the visible `area` of an NV12 or P010 image into `out` without
/// padding: luma rows, then chroma rows. In `src` rows are `pitch` bytes
/// apart and the chroma plane starts after `rows` luma rows. `bytes` is the
/// size of one sample (1 for NV12, 2 for P010).
fn pack_yuv420(
    src: &[u8],
    pitch: usize,
    rows: usize,
    area: Area,
    bytes: usize,
    out: &mut Vec<u8>,
) -> Result<()> {
    let row = |start: usize, len: usize| {
        (src.get(start..start + len)).context("the video buffer is smaller than its frame")
    };
    let (chroma_width, chroma_rows) = (area.width.div_ceil(2) * 2, area.height.div_ceil(2));
    out.clear();
    out.reserve((area.width * area.height + chroma_width * chroma_rows) * bytes);
    for y in area.y..area.y + area.height {
        out.extend_from_slice(row(y * pitch + area.x * bytes, area.width * bytes)?);
    }
    let chroma = rows * pitch + (area.x & !1) * bytes;
    for y in area.y / 2..area.y / 2 + chroma_rows {
        out.extend_from_slice(row(chroma + y * pitch, chroma_width * bytes)?);
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
        pack_yuv420(&src, pitch, rows, area, 1, &mut out).unwrap();
        #[rustfmt::skip]
        assert_eq!(out, [
            22, 23, 24, 25,
            32, 33, 34, 35,
            112, 113, 114, 115, // chroma row 1: the luma rows 2-3
        ]);
        assert!(pack_yuv420(&src[..pitch * rows], pitch, rows, area, 1, &mut out).is_err());
    }

    #[test]
    fn packs_two_byte_samples_for_p010() {
        // A 2x2 area at (2, 0) of a 4x2 frame with a 12-byte pitch (6 samples
        // per row): luma samples are 10*row + column, chroma 100 + column.
        let (pitch, rows) = (12, 2);
        let samples = |row: &dyn Fn(usize) -> u16, count: usize| {
            (0..count)
                .flat_map(|x| row(x).to_le_bytes())
                .collect::<Vec<u8>>()
        };
        let mut src = Vec::new();
        for y in 0..rows {
            src.extend(samples(&|x| (10 * y + x) as u16, pitch / 2));
        }
        src.extend(samples(&|x| (100 + x) as u16, pitch / 2));
        let area = Area {
            x: 2,
            y: 0,
            width: 2,
            height: 2,
        };
        let mut out = Vec::new();
        pack_yuv420(&src, pitch, rows, area, 2, &mut out).unwrap();
        let words: Vec<u16> = (out.as_chunks::<2>().0.iter())
            .map(|&b| u16::from_le_bytes(b))
            .collect();
        // Luma rows 0 and 1 at columns 2-3, then the chroma pair (U, V) at samples 2-3.
        assert_eq!(words, [2, 3, 12, 13, 102, 103]);
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
