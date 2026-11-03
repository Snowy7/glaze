//! `video` wallpapers: hardware-decoded by Media Foundation on a worker
//! thread ([`decode`]) and converted from NV12 to RGB in `video.wgsl`.
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
use windows::Win32::Media::MediaFoundation::{
    MFNominalRange_0_255, MFVideoTransferMatrix_BT601, MFVideoTransferMatrix_BT709,
    MFVideoTransferMatrix_BT2020_10, MFVideoTransferMatrix_BT2020_12,
};

use self::decode::{Nv12, Pixels, Spent, Stream, TICKS, Target};
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
}
const _: () = assert!(size_of::<Params>() == 64);

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
    fn upload(&mut self, gpu: &Gpu, size: (u32, u32), data: &[u8]) -> wgpu::BindGroup {
        if self.planes.as_ref().is_none_or(|p| p.size != size) {
            self.planes = Some(Planes::new(gpu, &self.draw, size));
        }
        let planes = self.planes.as_ref().expect("just created");
        let (luma, chroma) = data.split_at((size.0 * size.1) as usize);
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
    fn new(gpu: &Gpu, draw: &Draw, (width, height): (u32, u32)) -> Self {
        let none = wgpu::TextureUsages::empty();
        let (r8, rg8) = (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm);
        let chroma_size = (width.div_ceil(2), height.div_ceil(2));
        let luma = gpu.texture("video luma", (width, height), r8, none, None);
        let chroma = gpu.texture("video chroma", chroma_size, rg8, none, None);
        let bind_group = draw.bind_group(
            gpu,
            &luma.create_view(&Default::default()),
            &chroma.create_view(&Default::default()),
        );
        Self {
            size: (width, height),
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
                let bind_group = self.upload(gpu, frame.size, &data);
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
        let (scale, offset) = fit_transform(shown.size, frame.size, self.fit);
        let params = Params {
            scale,
            offset,
            rgb: shown.color.rgb_rows(),
        };
        // One buffer serves every screen: each screen's frame is its own submission.
        gpu.queue
            .write_buffer(&self.draw.params, 0, bytemuck::bytes_of(&params));
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        pass.set_pipeline(&self.draw.pipeline);
        pass.set_bind_group(0, &shown.bind_group, &[]);
        pass.draw(0..3, 0..1);
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
    frames: Receiver<Nv12>,
    recycle: SyncSender<Spent>,
    /// Received but not due yet.
    ahead: Option<Nv12>,
    /// Video time minus scaled engine time; set by the first frame, so
    /// playback starts with it whatever the engine clock reads.
    origin: Option<i64>,
    /// The worker is gone (decode error) and every frame it sent was taken.
    ended: bool,
}

impl Queue {
    fn new(frames: Receiver<Nv12>, recycle: SyncSender<Spent>) -> Self {
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
    fn take_due(&mut self, now: i64) -> Option<Nv12> {
        let mut newest: Option<Nv12> = None;
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

/// How a video's YUV samples map to RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Color {
    matrix: Matrix,
    /// 0-255 ("full") rather than 16-235 ("limited", the norm for video).
    full_range: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Color {
    /// From the `MF_MT_YUV_MATRIX` and `MF_MT_VIDEO_NOMINAL_RANGE` tags.
    /// Untagged video follows the usual convention: BT.601 below 720
    /// lines, BT.709 from there up, limited range.
    fn from_tags(matrix: Option<u32>, range: Option<u32>, height: usize) -> Self {
        let tagged = [
            (MFVideoTransferMatrix_BT601, Matrix::Bt601),
            (MFVideoTransferMatrix_BT709, Matrix::Bt709),
            (MFVideoTransferMatrix_BT2020_10, Matrix::Bt2020),
            (MFVideoTransferMatrix_BT2020_12, Matrix::Bt2020),
        ]
        .into_iter()
        .find(|(tag, _)| matrix == Some(tag.0 as u32));
        let untagged = if height < 720 {
            Matrix::Bt601
        } else {
            Matrix::Bt709
        };
        Self {
            matrix: tagged.map_or(untagged, |(_, matrix)| matrix),
            full_range: range == Some(MFNominalRange_0_255.0 as u32),
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
        // Unorm samples to Y' in 0..1 and Cb/Cr in -0.5..0.5.
        let (y_scale, y_offset, c_scale) = if self.full_range {
            (1.0, 0.0, 1.0)
        } else {
            (255.0 / 219.0, 16.0 / 255.0, 255.0 / 224.0)
        };
        let c_offset = 128.0 / 255.0;
        let row = |u: f32, v: f32| {
            let (u, v) = (u * c_scale, v * c_scale);
            [y_scale, u, v, -y_scale * y_offset - (u + v) * c_offset]
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

    use super::*;

    fn frame(time: i64) -> Nv12 {
        Nv12 {
            time,
            size: (2, 2),
            color: Color::from_tags(None, None, 2),
            pixels: Pixels::Cpu(Vec::new()),
        }
    }

    /// A queue fed with frames at `times`, plus its sender (room for 8
    /// frames, so tests never block) and the recycled buffers.
    fn queue(times: &[i64]) -> (Queue, SyncSender<Nv12>, Receiver<Spent>) {
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

    #[test]
    fn picks_the_tagged_matrix_or_falls_back_by_height() {
        let matrix = |tag: i32, height| Color::from_tags(Some(tag as u32), None, height).matrix;
        assert_eq!(matrix(MFVideoTransferMatrix_BT601.0, 1080), Matrix::Bt601);
        assert_eq!(
            matrix(MFVideoTransferMatrix_BT2020_10.0, 2160),
            Matrix::Bt2020
        );
        let matrix = |height| Color::from_tags(None, None, height).matrix;
        assert_eq!(matrix(480), Matrix::Bt601);
        assert_eq!(matrix(1080), Matrix::Bt709);
        assert!(!Color::from_tags(None, None, 1080).full_range);
        assert!(Color::from_tags(None, Some(MFNominalRange_0_255.0 as u32), 1080).full_range);
    }

    #[test]
    fn converts_limited_range_bt709_to_rgb() {
        let rows = Color::from_tags(None, None, 1080).rgb_rows();
        let rgb = |y: f32, u: f32, v: f32| {
            let yuv = [y / 255.0, u / 255.0, v / 255.0, 1.0];
            rows.map(|row| row.iter().zip(yuv).map(|(a, b)| a * b).sum::<f32>())
        };
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01);
        assert!(close(rgb(16.0, 128.0, 128.0), [0.0; 3]));
        assert!(close(rgb(235.0, 128.0, 128.0), [1.0; 3]));
        // BT.709 limited-range pure red.
        assert!(close(rgb(63.0, 102.0, 240.0), [1.0, 0.0, 0.0]));
    }
}
