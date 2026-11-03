//! Writes a short synthetic 10-bit HEVC (Main10) clip through Media Foundation's
//! sink writer, to test the video source's P010 and HDR paths without keeping
//! large binary files in the repo.
//!
//! ```text
//! cargo run -p mypaper-wallpaper --example hdr_clip -- OUT.mp4 [pq|hlg|sdr]
//! ```
//!
//! `pq` (default) is HDR10: SMPTE ST 2084 transfer, BT.2020 primaries. `hlg` is
//! the same with the hybrid log-gamma curve. `sdr` is plain 10-bit BT.709.
//! The picture is grey bars from 0.2 to 10000 nits, colour bars at 203 and at
//! 1000 nits, and a moving 4000-nit disc. Needs a hardware HEVC encoder that
//! takes P010 (most GPU drivers ship one); there is no software encoder in Windows.

use std::env;

use anyhow::{Context, Result, bail};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_DISABLE_THROTTLING,
    MF_VERSION, MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Video, MFNominalRange_16_235, MFSTARTUP_LITE,
    MFShutdown, MFStartup, MFVideoFormat_HEVC, MFVideoFormat_P010, MFVideoInterlace_Progressive,
    MFVideoPrimaries_BT709, MFVideoPrimaries_BT2020, MFVideoTransFunc_709, MFVideoTransFunc_2084,
    MFVideoTransFunc_HLG, MFVideoTransferMatrix_BT709, MFVideoTransferMatrix_BT2020_10,
    eAVEncH265VProfile_Main_420_10,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::HSTRING;

const SIZE: (u32, u32) = (1280, 720);
const FPS: u32 = 30;
const FRAMES: i64 = 60;
const TICKS: i64 = 10_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Signal {
    Pq,
    Hlg,
    Sdr,
}

impl Signal {
    /// Encodes display light in nits (SDR: relative to 203) as a 0..1 code value.
    fn encode(self, nits: f64) -> f64 {
        match self {
            Self::Pq => {
                let y = (nits / 10_000.0).clamp(0.0, 1.0).powf(2610.0 / 16384.0);
                let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
                ((c1 + c2 * y) / (1.0 + c3 * y)).powf(2523.0 / 32.0)
            }
            Self::Hlg => {
                // Nits as scene light of a 1000-nit display; no OOTF.
                let e = (nits / 1000.0).clamp(0.0, 1.0);
                let (a, b, c) = (0.178_832_77, 0.284_668_92, 0.559_910_73);
                if e <= 1.0 / 12.0 {
                    (3.0 * e).sqrt()
                } else {
                    a * (12.0 * e - b).ln() + c
                }
            }
            Self::Sdr => (nits / 203.0).clamp(0.0, 1.0).powf(1.0 / 2.4),
        }
    }

    /// Luma weights `(kr, kb)` of the matching YCbCr matrix.
    fn weights(self) -> (f64, f64) {
        match self {
            Self::Sdr => (0.2126, 0.0722),
            Self::Pq | Self::Hlg => (0.2627, 0.0593),
        }
    }
}

/// A 10-bit limited-range Y'CbCr colour, from linear nits per channel.
fn ycbcr(signal: Signal, [r, g, b]: [f64; 3]) -> [u16; 3] {
    let (r, g, b) = (signal.encode(r), signal.encode(g), signal.encode(b));
    let (kr, kb) = signal.weights();
    let y = kr * r + (1.0 - kr - kb) * g + kb * b;
    let cb = (b - y) / (2.0 * (1.0 - kb));
    let cr = (r - y) / (2.0 * (1.0 - kr));
    let code = |v: f64| v.round() as u16;
    [
        code(64.0 + 876.0 * y),
        code(512.0 + 896.0 * cb),
        code(512.0 + 896.0 * cr),
    ]
}

/// The bars at `(x, y)`, behind the disc.
fn bars(signal: Signal, (x, y): (u32, u32)) -> [u16; 3] {
    const GREYS: [f64; 10] = [
        0.2, 2.0, 10.0, 50.0, 100.0, 203.0, 400.0, 1000.0, 4000.0, 10000.0,
    ];
    const COLORS: [[f64; 3]; 7] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
    ];
    let (u, v) = (
        f64::from(x) / f64::from(SIZE.0),
        f64::from(y) / f64::from(SIZE.1),
    );
    if v < 0.55 {
        let nits = GREYS[((u * GREYS.len() as f64) as usize).min(GREYS.len() - 1)];
        return ycbcr(signal, [nits; 3]);
    }
    let bar = COLORS[((u * COLORS.len() as f64) as usize).min(COLORS.len() - 1)];
    let level = if v < 0.78 { 203.0 } else { 1000.0 };
    ycbcr(signal, bar.map(|c| c * level))
}

/// The picture of frame `frame`: `background` (the bars, row by row) with a
/// 4000-nit disc that crosses the grey bars once per clip.
fn pixel(signal: Signal, background: &[[u16; 3]], frame: i64, (x, y): (u32, u32)) -> [u16; 3] {
    let centre = (
        f64::from(SIZE.0) * (frame as f64 / FRAMES as f64),
        f64::from(SIZE.1) * 0.25,
    );
    let (dx, dy) = (f64::from(x) - centre.0, f64::from(y) - centre.1);
    if dx * dx + dy * dy < 60.0 * 60.0 {
        ycbcr(signal, [4000.0; 3])
    } else {
        background[(y * SIZE.0 + x) as usize]
    }
}

/// One P010 frame: 16-bit luma rows, then interleaved 16-bit chroma at half
/// resolution. The 10 bits sit in the high bits of each word.
fn frame_p010(signal: Signal, background: &[[u16; 3]], frame: i64) -> Vec<u8> {
    let (w, h) = SIZE;
    let mut words = Vec::with_capacity((w * h * 3 / 2) as usize);
    for y in 0..h {
        words.extend((0..w).map(|x| pixel(signal, background, frame, (x, y))[0] << 6));
    }
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let [_, cb, cr] = pixel(signal, background, frame, (x, y));
            words.extend([cb << 6, cr << 6]);
        }
    }
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let out = args
        .next()
        .context("usage: hdr_clip OUT.mp4 [pq|hlg|sdr]")?;
    let signal = match args.next().as_deref() {
        None | Some("pq") => Signal::Pq,
        Some("hlg") => Signal::Hlg,
        Some("sdr") => Signal::Sdr,
        Some(other) => bail!("unknown signal {other:?}: use pq, hlg or sdr"),
    };
    let (w, h) = SIZE;
    let pack = |hi: u32, lo: u32| (u64::from(hi) << 32) | u64::from(lo);

    // SAFETY: plain Media Foundation calls with valid arguments; every COM
    // object is dropped before `MFShutdown`.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        MFStartup(MF_VERSION, MFSTARTUP_LITE)?;
        let result = (|| -> Result<()> {
            let (primaries, transfer, matrix) = match signal {
                Signal::Sdr => (
                    MFVideoPrimaries_BT709,
                    MFVideoTransFunc_709,
                    MFVideoTransferMatrix_BT709,
                ),
                Signal::Pq => (
                    MFVideoPrimaries_BT2020,
                    MFVideoTransFunc_2084,
                    MFVideoTransferMatrix_BT2020_10,
                ),
                Signal::Hlg => (
                    MFVideoPrimaries_BT2020,
                    MFVideoTransFunc_HLG,
                    MFVideoTransferMatrix_BT2020_10,
                ),
            };
            let describe = |subtype| -> Result<_> {
                let ty = MFCreateMediaType()?;
                ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                ty.SetGUID(&MF_MT_SUBTYPE, subtype)?;
                ty.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
                ty.SetUINT64(&MF_MT_FRAME_RATE, pack(FPS, 1))?;
                ty.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                ty.SetUINT32(&MF_MT_VIDEO_PRIMARIES, primaries.0 as u32)?;
                ty.SetUINT32(&MF_MT_TRANSFER_FUNCTION, transfer.0 as u32)?;
                ty.SetUINT32(&MF_MT_YUV_MATRIX, matrix.0 as u32)?;
                ty.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
                Ok(ty)
            };
            let output = describe(&MFVideoFormat_HEVC)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000)?;
            output.SetUINT32(
                &MF_MT_MPEG2_PROFILE,
                eAVEncH265VProfile_Main_420_10.0 as u32,
            )?;
            let input = describe(&MFVideoFormat_P010)?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, w * 2)?;

            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.context("no attributes")?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            let writer =
                MFCreateSinkWriterFromURL(&HSTRING::from(out.as_str()), None, &attributes)?;
            let stream = writer
                .AddStream(&output)
                .context("no HEVC Main10 encoder for this output type")?;
            writer
                .SetInputMediaType(stream, &input, None)
                .context("the HEVC encoder does not take P010 input")?;
            writer.BeginWriting().context("starting the encoder")?;

            let background: Vec<[u16; 3]> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|at| bars(signal, at))
                .collect();
            let duration = TICKS / i64::from(FPS);
            for frame in 0..FRAMES {
                let data = frame_p010(signal, &background, frame);
                let buffer = MFCreateMemoryBuffer(data.len() as u32)?;
                let (mut dst, mut max) = (std::ptr::null_mut(), 0);
                buffer.Lock(&mut dst, Some(&mut max), None)?;
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len().min(max as usize));
                buffer.Unlock()?;
                buffer.SetCurrentLength(data.len() as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(frame * duration)?;
                sample.SetSampleDuration(duration)?;
                writer.WriteSample(stream, &sample)?;
            }
            writer.Finalize().context("finishing the file")?;
            Ok(())
        })();
        let _ = MFShutdown();
        result?;
    }
    println!("wrote {out}");
    Ok(())
}
