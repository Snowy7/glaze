//! `video` wallpapers: hardware-decoded by Media Foundation on a worker
//! thread ([`decode`]) and converted from NV12 (8-bit) or P010 (10-bit) to
//! RGB in `video.wgsl`, which also tone-maps HDR (PQ or HLG) to SDR.
//! Frames stay on the GPU when wgpu can share textures with the decoder
//! ([`share`]); otherwise they are copied through system memory.
//!
//! The playback position is the engine clock times `speed`, so a hidden
//! wallpaper pauses its video for free: the engine stops calling `update`,
//! the small frame channel fills up and the decoder blocks.

#[path = "video_decode.rs"]
mod decode;
#[path = "video_share.rs"]
mod share;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use mypaper_theme::{Fit, LoadedTheme, VideoWallpaper};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010};
use windows::Win32::Media::MediaFoundation::{
    MFNominalRange_0_255, MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoPrimaries_BT2020,
    MFVideoTransFunc_2084, MFVideoTransFunc_HLG, MFVideoTransferMatrix_BT601,
    MFVideoTransferMatrix_BT709, MFVideoTransferMatrix_BT2020_10, MFVideoTransferMatrix_BT2020_12,
};
use windows::core::GUID;

use self::decode::{Pixels, Spent, Stream, TICKS, Target, Yuv};
use super::blit::fit_transform;
use super::{Frame, Redraw, Source, begin, clear};
use crate::gpu::Gpu;

/// If decoding falls further behind the clock than this, playback slows
/// down to match instead of later racing (and skipping) to catch up.
const MAX_LAG: i64 = TICKS / 4;

/// Mirrors `struct Video` in video.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Params {
    scale: [f32; 2],
    offset: [f32; 2],
    rgb: [[f32; 4]; 3],
    /// [`Transfer`] as 0 (SDR), 1 (PQ) or 2 (HLG).
    transfer: u32,
    /// 1 if the primaries are BT.2020 and must be converted to BT.709.
    wide_gamut: u32,
    _pad: [u32; 2],
}
const _: () = assert!(size_of::<Params>() == 80);

impl Params {
    /// Maps screen UV to frame UV by `scale` and `offset`, and decodes the frame as `color`.
    fn new(scale: [f32; 2], offset: [f32; 2], color: Color) -> Self {
        Self {
            scale,
            offset,
            rgb: color.rgb_rows(),
            transfer: color.transfer as u32,
            wide_gamut: (color.gamut == Gamut::Bt2020) as u32,
            _pad: [0; 2],
        }
    }
}

/// How a decoded frame is stored: 8-bit NV12 or 10-bit P010. Both are a
/// luma plane plus one plane of interleaved chroma at half resolution;
/// P010 keeps its 10 bits in the high bits of 16-bit samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Nv12,
    P010,
}

impl Layout {
    const ALL: [Self; 2] = [Self::Nv12, Self::P010];

    fn bytes_per_sample(self) -> usize {
        match self {
            Self::Nv12 => 1,
            Self::P010 => 2,
        }
    }

    /// The luma and chroma texture formats wgpu samples the planes as.
    /// P010's need [`wgpu::Features::TEXTURE_FORMAT_16BIT_NORM`].
    fn plane_formats(self) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
        match self {
            Self::Nv12 => (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm),
            Self::P010 => (
                wgpu::TextureFormat::R16Unorm,
                wgpu::TextureFormat::Rg16Unorm,
            ),
        }
    }

    fn dxgi(self) -> DXGI_FORMAT {
        match self {
            Self::Nv12 => DXGI_FORMAT_NV12,
            Self::P010 => DXGI_FORMAT_P010,
        }
    }

    /// The Media Foundation subtype that asks a decoder for this layout.
    fn subtype(self) -> GUID {
        match self {
            Self::Nv12 => MFVideoFormat_NV12,
            Self::P010 => MFVideoFormat_P010,
        }
    }

    fn from_subtype(subtype: &GUID) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|layout| layout.subtype() == *subtype)
    }
}

pub struct VideoSource {
    queue: Queue,
    fit: Fit,
    speed: f64,
    interval: Duration,
    draw: Draw,
    /// Textures for frames copied through memory, reused while the size holds.
    planes: Option<Planes>,
    /// The decoder's shared slots, when frames stay on the GPU.
    shared: Option<share::Reader>,
    /// What `render` draws; `None` until the first frame arrives.
    shown: Option<Shown>,
    // Field order matters: `queue` drops first, closing the channel a
    // blocked worker waits on, and `shared` releases every slot; this then
    // joins the worker.
    _decoder: decode::Thread,
}

/// The YUV pipeline and what all its bind groups share.
struct Draw {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
}

/// Our own textures for frames copied through memory.
struct Planes {
    size: (u32, u32),
    layout: Layout,
    luma: wgpu::Texture,
    chroma: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct Shown {
    size: (u32, u32),
    color: Color,
    bind_group: wgpu::BindGroup,
    /// The shared slot on screen, if it is one.
    slot: Option<usize>,
}

impl VideoSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &VideoWallpaper) -> Result<Self> {
        let path = theme.asset(&config.source);
        ensure!(path.is_file(), "video {} not found", path.display());
        let draw = Draw::new(gpu);
        let (stream, shared) = start(gpu, &draw, path, config)?;
        let speed = f64::from(config.speed);
        // New frames arrive at the video's rate times speed; drawing faster only repeats them.
        let rate = f64::from(config.fps).min(stream.fps * speed);
        Ok(Self {
            queue: Queue::new(stream.frames, stream.recycle),
            fit: config.fit,
            speed,
            interval: Duration::from_secs_f64(1.0 / rate),
            draw,
            planes: None,
            shared,
            shown: None,
            _decoder: stream.thread,
        })
    }

    /// Uploads a frame copied through memory; returns its bind group.
    fn upload(
        &mut self,
        gpu: &Gpu,
        layout: Layout,
        size: (u32, u32),
        data: &[u8],
    ) -> wgpu::BindGroup {
        if (self.planes.as_ref()).is_none_or(|p| (p.size, p.layout) != (size, layout)) {
            self.planes = Some(Planes::new(gpu, &self.draw, layout, size));
        }
        let planes = self.planes.as_ref().expect("just created");
        let (luma, chroma) =
            data.split_at(size.0 as usize * size.1 as usize * layout.bytes_per_sample());
        gpu.write_texture(&planes.luma, luma);
        gpu.write_texture(&planes.chroma, chroma);
        planes.bind_group.clone()
    }
}

/// Starts the decoder: zero-copy if wgpu can import its slots, otherwise
/// (or if the import fails) copying frames through memory.
fn start(
    gpu: &Gpu,
    draw: &Draw,
    path: PathBuf,
    config: &VideoWallpaper,
) -> Result<(Stream, Option<share::Reader>)> {
    let spawn = |target| decode::spawn(path.clone(), config.fps, config.speed, target);
    let (adapter, released, handle) = match share::target(gpu) {
        Ok((luid, fence, handle)) => (Some(luid), Some(fence), Some(handle)),
        Err(e) => {
            log::warn!("zero-copy video unavailable: {e:#}");
            (None, None, None)
        }
    };
    let mut stream = spawn(Target {
        adapter,
        released: handle,
    })?;
    ensure!(
        stream.layout == Layout::Nv12
            || gpu
                .device
                .features()
                .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM),
        "10-bit video needs 16-bit normalized textures, which this GPU lacks"
    );
    let (Some(slots), Some(released)) = (stream.slots.take(), released) else {
        return Ok((stream, None));
    };
    let bind = |luma: &_, chroma: &_| draw.bind_group(gpu, luma, chroma);
    match share::Reader::import(gpu, released, &slots, bind) {
        Ok(reader) => Ok((stream, Some(reader))),
        Err(e) => {
            log::warn!("zero-copy video unavailable, copying frames: {e:#}");
            drop(stream);
            let target = Target {
                adapter,
                released: None,
            };
            Ok((spawn(target)?, None))
        }
    }
}

impl Draw {
    fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::include_wgsl!("video.wgsl"));
        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                plane(1),
                plane(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(gpu.format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            sampler,
            params,
        }
    }

    /// Uploads the uniforms for the next draw.
    fn set_params(&self, gpu: &Gpu, params: &Params) {
        gpu.queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(params));
    }

    /// Draws the frame in `bind_group` over the whole target.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, bind_group: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn bind_group(
        &self,
        gpu: &Gpu,
        luma: &wgpu::TextureView,
        chroma: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(luma),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(chroma),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

impl Planes {
    fn new(gpu: &Gpu, draw: &Draw, layout: Layout, (width, height): (u32, u32)) -> Self {
        let none = wgpu::TextureUsages::empty();
        let (luma_format, chroma_format) = layout.plane_formats();
        let chroma_size = (width.div_ceil(2), height.div_ceil(2));
        let luma = gpu.texture("video luma", (width, height), luma_format, none, None);
        let chroma = gpu.texture("video chroma", chroma_size, chroma_format, none, None);
        let bind_group = draw.bind_group(
            gpu,
            &luma.create_view(&Default::default()),
            &chroma.create_view(&Default::default()),
        );
        Self {
            size: (width, height),
            layout,
            luma,
            chroma,
            bind_group,
        }
    }
}

impl Source for VideoSource {
    fn update(&mut self, gpu: &Gpu, clock: Duration) {
        let now = (clock.as_secs_f64() * self.speed * TICKS as f64) as i64;
        let Some(frame) = self.queue.take_due(now) else {
            return;
        };
        let (bind_group, slot) = match frame.pixels {
            Pixels::Cpu(data) => {
                let bind_group = self.upload(gpu, frame.color.layout, frame.size, &data);
                self.queue.recycle(Spent::Buffer(data));
                (bind_group, None)
            }
            Pixels::Gpu { slot, ready } => {
                let Some(shared) = &self.shared else {
                    // Never sent without slots; hand it back regardless.
                    return self.queue.recycle(Spent::Slot { slot, after: 0 });
                };
                shared.show(ready);
                (shared.bind_groups[slot].clone(), Some(slot))
            }
        };
        let previous = self.shown.replace(Shown {
            size: frame.size,
            color: frame.color,
            bind_group,
            slot,
        });
        if let (Some(slot), Some(shared)) = (previous.and_then(|s| s.slot), &mut self.shared) {
            let after = shared.release();
            self.queue.recycle(Spent::Slot { slot, after });
        }
    }

    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let Some(shown) = &self.shown else {
            return clear(frame.encoder, frame.view, wgpu::Color::BLACK);
        };
        let (scale, offset) = fit_transform(shown.size, frame.size, self.fit, super::blit::CENTER);
        let params = Params::new(scale, offset, shown.color);
        // One buffer serves every screen: each screen's frame is its own submission.
        self.draw.set_params(gpu, &params);
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        self.draw.draw(&mut pass, &shown.bind_group);
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        if self.queue.ended() {
            Redraw::OnChange // decoding stopped: the last frame is all there is
        } else {
            Redraw::Every(self.interval)
        }
    }
}

/// The engine's end of the frame channel: picks which decoded frame to show.
struct Queue {
    frames: Receiver<Yuv>,
    recycle: SyncSender<Spent>,
    /// Received but not due yet.
    ahead: Option<Yuv>,
    /// Video time minus scaled engine time; set by the first frame, so
    /// playback starts with it whatever the engine clock reads.
    origin: Option<i64>,
    /// The worker is gone (decode error) and every frame it sent was taken.
    ended: bool,
}

impl Queue {
    fn new(frames: Receiver<Yuv>, recycle: SyncSender<Spent>) -> Self {
        Self {
            frames,
            recycle,
            ahead: None,
            origin: None,
            ended: false,
        }
    }

    /// The newest frame due at `now` (scaled engine time, in ticks), if a
    /// new one is due. Older due frames are skipped; later ones wait.
    fn take_due(&mut self, now: i64) -> Option<Yuv> {
        let mut newest: Option<Yuv> = None;
        loop {
            let frame = match self.ahead.take() {
                Some(frame) => frame,
                None => match self.frames.try_recv() {
                    Ok(frame) => frame,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.ended = true;
                        break;
                    }
                },
            };
            let origin = *self.origin.get_or_insert(frame.time - now);
            if frame.time > now + origin {
                self.ahead = Some(frame);
                break;
            }
            if let Some(skipped) = newest.replace(frame) {
                self.recycle(skipped.pixels.spent());
            }
        }
        if let (Some(frame), Some(origin)) = (&newest, &mut self.origin)
            && now + *origin - frame.time > MAX_LAG
        {
            *origin = frame.time - now;
        }
        newest
    }

    /// Hands a buffer or slot back to the worker for reuse. The channel has
    /// room for everything in flight, so this only fails once the worker is gone.
    fn recycle(&self, spent: Spent) {
        let _ = self.recycle.try_send(spent);
    }

    fn ended(&self) -> bool {
        self.ended
    }
}

/// The `MF_MT_*` colour tags of a video; `None` where untagged.
#[derive(Debug, Clone, Copy, Default)]
struct Tags {
    /// `MF_MT_YUV_MATRIX`
    matrix: Option<u32>,
    /// `MF_MT_VIDEO_NOMINAL_RANGE`
    range: Option<u32>,
    /// `MF_MT_TRANSFER_FUNCTION`
    transfer: Option<u32>,
    /// `MF_MT_VIDEO_PRIMARIES`
    primaries: Option<u32>,
}

/// How a video's YUV samples map to RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Color {
    layout: Layout,
    matrix: Matrix,
    /// Full ("0-255", or 0-1023) rather than "limited" range, the norm for video.
    full_range: bool,
    transfer: Transfer,
    gamut: Gamut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// The curve the RGB values are encoded with. `video.wgsl` tone-maps the
/// HDR ones to SDR; the discriminants are what it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transfer {
    /// BT.709 or similar: shown as is.
    Sdr = 0,
    /// SMPTE ST 2084 (HDR10, Dolby Vision's base layer).
    Pq = 1,
    /// ARIB STD-B67 (HLG, as phones record HDR).
    Hlg = 2,
}

/// The RGB primaries, which only matter when tone-mapping in linear light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gamut {
    Bt709,
    Bt2020,
}

impl Color {
    /// From the container's colour tags. Untagged video follows the usual
    /// convention: BT.601 below 720 lines, BT.709 from there up, limited
    /// range, SDR. Untagged primaries follow the matrix.
    fn from_tags(layout: Layout, tags: Tags, height: usize) -> Self {
        let tagged = [
            (MFVideoTransferMatrix_BT601, Matrix::Bt601),
            (MFVideoTransferMatrix_BT709, Matrix::Bt709),
            (MFVideoTransferMatrix_BT2020_10, Matrix::Bt2020),
            (MFVideoTransferMatrix_BT2020_12, Matrix::Bt2020),
        ]
        .into_iter()
        .find(|(tag, _)| tags.matrix == Some(tag.0 as u32));
        let untagged = if height < 720 {
            Matrix::Bt601
        } else {
            Matrix::Bt709
        };
        let matrix = tagged.map_or(untagged, |(_, matrix)| matrix);
        let transfer = match tags.transfer {
            Some(tag) if tag == MFVideoTransFunc_2084.0 as u32 => Transfer::Pq,
            Some(tag) if tag == MFVideoTransFunc_HLG.0 as u32 => Transfer::Hlg,
            _ => Transfer::Sdr,
        };
        let wide = match tags.primaries {
            Some(tag) if tag == MFVideoPrimaries_BT2020.0 as u32 => true,
            // Reserved or unknown primaries: guess from the matrix.
            Some(0 | 1) | None => matrix == Matrix::Bt2020,
            Some(_) => false,
        };
        Self {
            layout,
            matrix,
            full_range: tags.range == Some(MFNominalRange_0_255.0 as u32),
            transfer,
            gamut: if wide { Gamut::Bt2020 } else { Gamut::Bt709 },
        }
    }

    /// Rows of the affine transform from sampled (unorm) Y, U, V to RGB:
    /// `channel = row · (y, u, v, 1)`.
    fn rgb_rows(self) -> [[f32; 4]; 3] {
        let (kr, kb) = match self.matrix {
            Matrix::Bt601 => (0.299, 0.114),
            Matrix::Bt709 => (0.2126, 0.0722),
            Matrix::Bt2020 => (0.2627, 0.0593),
        };
        let kg = 1.0 - kr - kb;
        // Limited range spans Y 16-235 and Cb/Cr 16-240 around 128, times
        // 4 for 10 bits. The codes below are 8-bit ones; `max` is the top code.
        let (max, depth) = match self.layout {
            Layout::Nv12 => (255.0, 1.0),
            Layout::P010 => (1023.0, 4.0),
        };
        // A sample is `code / max`, except P010's 16-bit samples hold the
        // 10-bit code shifted up 6 bits, which reads as `code * 64 / 65535`.
        let sample_scale = match self.layout {
            Layout::Nv12 => 1.0,
            Layout::P010 => 65535.0 / (64.0 * max),
        };
        // Samples to Y' in 0..1 and Cb/Cr in -0.5..0.5.
        let (y_scale, y_offset, c_scale) = if self.full_range {
            (1.0, 0.0, 1.0)
        } else {
            (
                max / (219.0 * depth),
                16.0 * depth / max,
                max / (224.0 * depth),
            )
        };
        let c_offset = 128.0 * depth / max;
        let row = |u: f32, v: f32| {
            let (u, v) = (u * c_scale, v * c_scale);
            [
                y_scale * sample_scale,
                u * sample_scale,
                v * sample_scale,
                -y_scale * y_offset - (u + v) * c_offset,
            ]
        };
        [
            row(0.0, 2.0 * (1.0 - kr)),
            row(-2.0 * kb * (1.0 - kb) / kg, -2.0 * kr * (1.0 - kr) / kg),
            row(2.0 * (1.0 - kb), 0.0),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use windows::Win32::Media::MediaFoundation::{
        MFVideoPrimaries_BT709, MFVideoTransFunc_709, MFVideoTransferFunction,
    };

    use super::*;

    fn frame(time: i64) -> Yuv {
        Yuv {
            time,
            size: (2, 2),
            color: Color::from_tags(Layout::Nv12, Tags::default(), 2),
            pixels: Pixels::Cpu(Vec::new()),
        }
    }

    /// A queue fed with frames at `times`, plus its sender (room for 8
    /// frames, so tests never block) and the recycled buffers.
    fn queue(times: &[i64]) -> (Queue, SyncSender<Yuv>, Receiver<Spent>) {
        let (tx, rx) = mpsc::sync_channel(8);
        let (recycle, spent) = mpsc::sync_channel(8);
        for &time in times {
            tx.send(frame(time)).unwrap();
        }
        (Queue::new(rx, recycle), tx, spent)
    }

    #[test]
    fn shows_the_newest_due_frame_and_keeps_later_ones() {
        let (mut q, _tx, spent) = queue(&[0, 100, 200, 300]);
        // The first frame anchors playback, whatever the clock reads.
        assert_eq!(q.take_due(5_000).map(|f| f.time), Some(0));
        assert!(q.take_due(5_050).is_none());
        // 100 and 200 are both due: 100 is skipped (and recycled), 300 waits.
        assert_eq!(q.take_due(5_250).map(|f| f.time), Some(200));
        assert_eq!(spent.try_iter().count(), 1);
        assert_eq!(q.take_due(5_300).map(|f| f.time), Some(300));
    }

    #[test]
    fn slows_down_instead_of_racing_when_decoding_lags() {
        let (mut q, tx, _spent) = queue(&[0]);
        assert!(q.take_due(0).is_some());
        // The next frame arrives a second late: it shows now, and becomes the new anchor.
        tx.send(frame(100)).unwrap();
        tx.send(frame(200)).unwrap();
        assert_eq!(q.take_due(TICKS).map(|f| f.time), Some(200));
        tx.send(frame(300)).unwrap();
        assert!(q.take_due(TICKS + 50).is_none());
        assert_eq!(q.take_due(TICKS + 100).map(|f| f.time), Some(300));
    }

    #[test]
    fn ends_only_after_the_last_frame_is_shown() {
        let (mut q, tx, _spent) = queue(&[0, 100]);
        drop(tx);
        assert!(q.take_due(0).is_some());
        assert!(!q.ended());
        assert!(q.take_due(100).is_some());
        assert!(q.take_due(200).is_none());
        assert!(q.ended());
    }

    fn color(layout: Layout, tags: Tags, height: usize) -> Color {
        Color::from_tags(layout, tags, height)
    }

    #[test]
    fn picks_the_tagged_matrix_or_falls_back_by_height() {
        let matrix = |tag: i32, height| {
            let tags = Tags {
                matrix: Some(tag as u32),
                ..Tags::default()
            };
            color(Layout::Nv12, tags, height).matrix
        };
        assert_eq!(matrix(MFVideoTransferMatrix_BT601.0, 1080), Matrix::Bt601);
        assert_eq!(
            matrix(MFVideoTransferMatrix_BT2020_10.0, 2160),
            Matrix::Bt2020
        );
        let matrix = |height| color(Layout::Nv12, Tags::default(), height).matrix;
        assert_eq!(matrix(480), Matrix::Bt601);
        assert_eq!(matrix(1080), Matrix::Bt709);
        assert!(!color(Layout::Nv12, Tags::default(), 1080).full_range);
        let full = Tags {
            range: Some(MFNominalRange_0_255.0 as u32),
            ..Tags::default()
        };
        assert!(color(Layout::Nv12, full, 1080).full_range);
    }

    #[test]
    fn tells_hdr_from_sdr_by_transfer_and_gamut_by_primaries() {
        let hdr = |transfer: MFVideoTransferFunction, primaries: Option<i32>, matrix: i32| {
            let tags = Tags {
                transfer: Some(transfer.0 as u32),
                primaries: primaries.map(|p| p as u32),
                matrix: Some(matrix as u32),
                ..Tags::default()
            };
            color(Layout::P010, tags, 2160)
        };
        let bt2020 = MFVideoTransferMatrix_BT2020_10.0;
        let pq = hdr(
            MFVideoTransFunc_2084,
            Some(MFVideoPrimaries_BT2020.0),
            bt2020,
        );
        assert_eq!((pq.transfer, pq.gamut), (Transfer::Pq, Gamut::Bt2020));
        let hlg = hdr(MFVideoTransFunc_HLG, None, bt2020);
        assert_eq!((hlg.transfer, hlg.gamut), (Transfer::Hlg, Gamut::Bt2020));
        // BT.709 content is SDR and keeps BT.709 primaries, even at 10 bits.
        let sdr = hdr(
            MFVideoTransFunc_709,
            Some(MFVideoPrimaries_BT709.0),
            MFVideoTransferMatrix_BT709.0,
        );
        assert_eq!((sdr.transfer, sdr.gamut), (Transfer::Sdr, Gamut::Bt709));
        let untagged = color(Layout::P010, Tags::default(), 2160);
        assert_eq!(untagged.transfer, Transfer::Sdr);
        // Transfer values are what the shader switches on.
        assert_eq!(
            [Transfer::Sdr, Transfer::Pq, Transfer::Hlg].map(|t| t as u32),
            [0, 1, 2]
        );
    }

    /// RGB for samples given as `code / max` fractions of the coded range.
    fn rgb(rows: [[f32; 4]; 3], y: f32, u: f32, v: f32) -> [f32; 3] {
        rows.map(|row| row[0] * y + row[1] * u + row[2] * v + row[3])
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01)
    }

    #[test]
    fn converts_limited_range_bt709_to_rgb() {
        let rows = color(Layout::Nv12, Tags::default(), 1080).rgb_rows();
        let rgb = |y: f32, u: f32, v: f32| rgb(rows, y / 255.0, u / 255.0, v / 255.0);
        assert!(close(rgb(16.0, 128.0, 128.0), [0.0; 3]));
        assert!(close(rgb(235.0, 128.0, 128.0), [1.0; 3]));
        // BT.709 limited-range pure red.
        assert!(close(rgb(63.0, 102.0, 240.0), [1.0, 0.0, 0.0]));
    }

    /// Renders a 2x2 frame whose samples are all the codes `[y, u, v]` (8-bit
    /// for NV12, 10-bit for P010) through the real pipeline and shader, and
    /// returns the RGB it draws. `None` without a GPU.
    fn render(color: Color, [y, u, v]: [u16; 3]) -> Option<[u8; 3]> {
        let gpu = Gpu::headless()?;
        let draw = Draw::new(&gpu);
        let planes = Planes::new(&gpu, &draw, color.layout, (2, 2));
        // P010 keeps its 10 bits in the high bits of 16-bit words.
        let samples = |codes: &[u16]| -> Vec<u8> {
            match color.layout {
                Layout::Nv12 => codes.iter().map(|&c| c as u8).collect(),
                Layout::P010 => codes.iter().flat_map(|&c| (c << 6).to_le_bytes()).collect(),
            }
        };
        gpu.write_texture(&planes.luma, &samples(&[y; 4]));
        gpu.write_texture(&planes.chroma, &samples(&[u, v]));

        let target = gpu.texture(
            "test target",
            (2, 2),
            gpu.format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            None,
        );
        // Buffer rows must be 256-byte aligned.
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test readback"),
            size: 256 * 2,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        draw.set_params(&gpu, &Params::new([1.0; 2], [0.0; 2], color));
        {
            let view = target.create_view(&Default::default());
            let mut pass = begin(&mut encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            draw.draw(&mut pass, &planes.bind_group);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(2),
                },
            },
            wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let bytes = readback.slice(..).get_mapped_range().unwrap();
        // The target is BGRA.
        Some([bytes[2], bytes[1], bytes[0]])
    }

    /// Whether `got` is within `tolerance` of `want` in every channel.
    fn near(got: [u8; 3], want: [u8; 3], tolerance: u8) -> bool {
        got.iter()
            .zip(want)
            .all(|(g, w)| g.abs_diff(w) <= tolerance)
    }

    /// The code of a limited-range 10-bit luma for the encoded signal `e` (0..1).
    fn luma10(e: f64) -> u16 {
        (64.0 + 876.0 * e).round() as u16
    }

    /// SMPTE ST 2084 inverse EOTF: `nits` to an encoded signal.
    fn pq_encode(nits: f64) -> f64 {
        let y = (nits / 10_000.0).powf(2610.0 / 16384.0);
        let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
        ((c1 + c2 * y) / (1.0 + c3 * y)).powf(2523.0 / 32.0)
    }

    fn hdr_tags(transfer: MFVideoTransferFunction) -> Tags {
        Tags {
            matrix: Some(MFVideoTransferMatrix_BT2020_10.0 as u32),
            transfer: Some(transfer.0 as u32),
            primaries: Some(MFVideoPrimaries_BT2020.0 as u32),
            ..Tags::default()
        }
    }

    #[test]
    fn ten_bit_and_eight_bit_sdr_draw_the_same_colors() {
        let eight = color(Layout::Nv12, Tags::default(), 1080);
        let ten = color(Layout::P010, Tags::default(), 1080);
        // Limited-range BT.709: black, mid grey, white, and pure red (8-bit codes, then 10-bit).
        let cases = [
            ([16, 128, 128], [64, 512, 512], [0, 0, 0]),
            ([126, 128, 128], [504, 512, 512], [128, 128, 128]),
            ([235, 128, 128], [940, 512, 512], [255, 255, 255]),
            ([63, 102, 240], [252, 408, 960], [255, 0, 0]),
        ];
        for (nv12, p010, want) in cases {
            let Some(got8) = render(eight, nv12) else {
                return eprintln!("no GPU: skipped");
            };
            assert!(
                near(got8, want, 3),
                "NV12 {nv12:?}: {got8:?}, want {want:?}"
            );
            let got10 = render(ten, p010).unwrap();
            assert!(
                near(got10, want, 3),
                "P010 {p010:?}: {got10:?}, want {want:?}"
            );
        }
    }

    #[test]
    fn pq_video_is_tone_mapped_to_sdr() {
        let pq = color(Layout::P010, hdr_tags(MFVideoTransFunc_2084), 2160);
        // Grey at a luminance in nits: black stays black, 100 nits lands on the
        // sRGB value of 100/203 of SDR white, reference white (203) is just
        // under full, and 1000 and 10000 nits roll off but stay distinct.
        let grey = |nits: f64| {
            let y = luma10(pq_encode(nits));
            render(pq, [y, 512, 512])
        };
        let Some(black) = grey(0.0) else {
            return eprintln!("no GPU: skipped");
        };
        assert!(near(black, [0; 3], 2), "0 nits: {black:?}");
        let (n100, n203) = (grey(100.0).unwrap(), grey(203.0).unwrap());
        let (n1000, n10000) = (grey(1000.0).unwrap(), grey(10_000.0).unwrap());
        assert!(near(n100, [186; 3], 3), "100 nits: {n100:?}");
        assert!(near(n203, [244; 3], 3), "203 nits: {n203:?}");
        assert!(near(n1000, [254; 3], 2), "1000 nits: {n1000:?}");
        assert!(
            n10000[0] >= n1000[0] && n10000[0] >= 254,
            "10000 nits: {n10000:?}"
        );
    }

    #[test]
    fn hlg_reference_white_matches_pq_reference_white() {
        // HLG at 75% signal is its reference white, about 203 nits on a 1000-nit display.
        let hlg = color(Layout::P010, hdr_tags(MFVideoTransFunc_HLG), 2160);
        let Some(got) = render(hlg, [luma10(0.75), 512, 512]) else {
            return eprintln!("no GPU: skipped");
        };
        assert!(near(got, [244; 3], 3), "HLG 75%: {got:?}");
    }

    #[test]
    fn bt2020_primaries_are_converted_to_bt709() {
        // BT.2020 green at 203 nits lies outside BT.709: red and blue clip to
        // zero and green is tone-mapped just under full (about 246).
        let pq = color(Layout::P010, hdr_tags(MFVideoTransFunc_2084), 2160);
        let g = pq_encode(203.0);
        // BT.2020 non-constant-luminance Y'CbCr of (0, g, 0), limited 10-bit.
        let (kr, kb) = (0.2627, 0.0593);
        let y = (1.0 - kr - kb) * g;
        let (cb, cr) = (-y / (2.0 * (1.0 - kb)), -y / (2.0 * (1.0 - kr)));
        let code = |c: f64| (512.0 + 896.0 * c).round() as u16;
        let Some(got) = render(pq, [luma10(y), code(cb), code(cr)]) else {
            return eprintln!("no GPU: skipped");
        };
        assert!(
            got[0] <= 4 && got[2] <= 4,
            "red and blue should clip: {got:?}"
        );
        assert!(got[1].abs_diff(246) <= 4, "green: {got:?}");
    }

    #[test]
    fn converts_ten_bit_p010_samples_like_the_eight_bit_ones() {
        let rows = color(Layout::P010, Tags::default(), 1080).rgb_rows();
        // A P010 sample holds the 10-bit code in its high bits, and the
        // texture reads it as a fraction of 65535.
        let rgb = |y: u32, u: u32, v: u32| {
            let sample = |code: u32| (code << 6) as f32 / 65535.0;
            rgb(rows, sample(y), sample(u), sample(v))
        };
        assert!(close(rgb(64, 512, 512), [0.0; 3]));
        assert!(close(rgb(940, 512, 512), [1.0; 3]));
        // The same BT.709 limited-range pure red as in 8 bits (63, 102, 240), times 4.
        assert!(close(rgb(252, 408, 960), [1.0, 0.0, 0.0]));
    }
}
