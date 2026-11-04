//! Liquid glass under UI windows (widgets, and the bars when they ask): the plates other
//! processes publish (`mypaper_theme::glass`) drawn into the wallpaper's own frame, so the glass
//! is exactly as live as the wallpaper and costs no extra copies of it.
//!
//! Per screen and frame, only when that screen has plates: the region under each plate is copied
//! out of the finished frame into a scratch texture, then one instanced quad per plate is drawn
//! twice (shadows, then glass; see glass.wgsl). The clear centre of a plate is discarded, so the
//! cost is the rim band and the shadow, not the plate's area. Nothing is allocated while no plate
//! exists. Plates spring to new rects (and grow in, and fade out), which is the "liquid" morph
//! when a widget appears, moves or resizes; while one moves, the screens it touches redraw at
//! 60 fps, even a still wallpaper, and then stop.

use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use mypaper_desktop::Rect;
use mypaper_theme::glass::{self, Plate};

use crate::gpu::Gpu;

/// The shadow's reach past a plate, px (matches `MARGIN` in glass.wgsl).
const MARGIN: i32 = 12;
/// How fast plates follow their rects: a spring of this angular frequency (rad/s), a little
/// underdamped so a moved plate settles with the faintest wobble.
const OMEGA: f32 = 26.0;
const DAMPING: f32 = 0.82;
/// A redraw every frame while something moves.
const FRAME: Duration = Duration::from_millis(16);
/// Most plates drawn (each is drawn twice: shadow and glass).
const MAX_PLATES: usize = 96;

/// Mirrors `Plate` in glass.wgsl: four vec4s per instance.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    shape: [f32; 4],
    tint: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// A plate as drawn: where it is now, where it is going, and how present it is.
struct Moving {
    plate: Plate,
    /// x, y, width, height in virtual-screen px.
    at: [f32; 4],
    velocity: [f32; 4],
    target: [f32; 4],
    /// 0..1: grows in when a plate appears, fades out when it goes.
    fade: f32,
    leaving: bool,
}

impl Moving {
    fn settled(&self) -> bool {
        let still = (0..4)
            .all(|i| (self.at[i] - self.target[i]).abs() < 0.3 && self.velocity[i].abs() < 2.0);
        still && (self.fade - if self.leaving { 0.0 } else { 1.0 }).abs() < 0.01
    }

    /// The rect it covers, now or soon, with its shadow (for which screens redraw).
    fn reach(&self) -> [f32; 4] {
        let lo = [
            self.at[0].min(self.target[0]),
            self.at[1].min(self.target[1]),
        ];
        let hi = [
            (self.at[0] + self.at[2]).max(self.target[0] + self.target[2]),
            (self.at[1] + self.at[3]).max(self.target[1] + self.target[3]),
        ];
        let m = MARGIN as f32;
        [
            lo[0] - m,
            lo[1] - m,
            hi[0] - lo[0] + 2.0 * m,
            hi[1] - lo[1] + 2.0 * m,
        ]
    }
}

fn rect_of(plate: &Plate) -> [f32; 4] {
    plate.rect.map(|v| v as f32)
}

/// Moves `at` toward `target` with a damped spring over `dt` seconds (in small steps, so a long
/// frame can't overshoot wildly).
fn spring(at: &mut f32, velocity: &mut f32, target: f32, dt: f32) {
    let steps = (dt / 0.008).ceil().max(1.0);
    let h = dt / steps;
    for _ in 0..steps as u32 {
        let accel = OMEGA * OMEGA * (target - *at) - 2.0 * DAMPING * OMEGA * *velocity;
        *velocity += accel * h;
        *at += *velocity * h;
    }
}

fn overlaps(a: [f32; 4], b: &Rect) -> bool {
    a[0] < (b.x + b.width) as f32
        && a[0] + a[2] > b.x as f32
        && a[1] < (b.y + b.height) as f32
        && a[1] + a[3] > b.y as f32
}

/// The scratch copy of one screen's frame under its plates.
struct Scratch {
    size: (u32, u32),
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
}

pub struct GlassPass {
    /// The monitors the screens show, in order (desktop mode); none in the dev window.
    monitors: Vec<Rect>,
    moving: Vec<Moving>,
    last_step: Option<Instant>,
    /// Per screen: a frame is due because plates on it changed or moved.
    dirty: Vec<bool>,
    pipeline: Option<Pipeline>,
    scratch: Vec<Option<Scratch>>,
    /// The registered message that says a plates file changed (`glass::MESSAGE`).
    pub message: u32,
    /// Benchmarks only: which halves of the pass run (copies, draw).
    #[cfg(test)]
    parts: (bool, bool),
}

impl GlassPass {
    pub fn new(monitors: Option<&[Rect]>) -> Self {
        let monitors = monitors.map(<[Rect]>::to_vec).unwrap_or_default();
        let count = monitors.len();
        let name: Vec<u16> = glass::MESSAGE.encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name that outlives the call.
        let message = unsafe {
            windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(windows::core::PCWSTR(
                name.as_ptr(),
            ))
        };
        let mut pass = Self {
            monitors,
            moving: Vec::new(),
            last_step: None,
            dirty: vec![false; count],
            pipeline: None,
            scratch: (0..count).map(|_| None).collect(),
            message,
            #[cfg(test)]
            parts: (true, true),
        };
        pass.reload();
        pass
    }

    /// Reads every live writer's plates (after its `mypaper.glass` message, and at startup).
    pub fn reload(&mut self) {
        self.set_plates(glass::read_all(alive));
    }

    /// Makes `plates` the targets: known ids spring to their new rects, new ones grow in, gone
    /// ones fade out.
    fn set_plates(&mut self, plates: Vec<Plate>) {
        for moving in &mut self.moving {
            moving.leaving = !plates.iter().any(|p| p.id == moving.plate.id);
        }
        for plate in plates.into_iter().take(MAX_PLATES) {
            let target = rect_of(&plate);
            match self.moving.iter_mut().find(|m| m.plate.id == plate.id) {
                Some(moving) => {
                    moving.target = target;
                    moving.plate = plate;
                    moving.leaving = false;
                }
                None => {
                    // Appearing: from a slightly smaller plate at the same centre.
                    let [x, y, w, h] = target;
                    self.moving.push(Moving {
                        plate,
                        at: [x + w * 0.06, y + h * 0.06, w * 0.88, h * 0.88],
                        velocity: [0.0; 4],
                        target,
                        fade: 0.0,
                        leaving: false,
                    });
                }
            }
        }
        self.mark(true);
        self.last_step = None;
    }

    /// Marks the screens that `moving` plates (all of them with `all`) touch.
    fn mark(&mut self, all: bool) {
        for (index, monitor) in self.monitors.iter().enumerate() {
            let touched = self
                .moving
                .iter()
                .any(|m| (all || !m.settled()) && overlaps(m.reach(), monitor));
            if let Some(dirty) = self.dirty.get_mut(index) {
                *dirty |= touched;
            }
        }
    }

    /// Steps the springs to `now`; plates still moving mark their screens for a frame.
    pub fn advance(&mut self, now: Instant) {
        if self.moving.iter().all(Moving::settled) {
            self.last_step = None;
            self.moving.retain(|m| !m.leaving);
            return;
        }
        let dt = self
            .last_step
            .map_or(1.0 / 60.0, |t| (now - t).as_secs_f32())
            .min(0.1);
        self.last_step = Some(now);
        for m in &mut self.moving {
            for i in 0..4 {
                spring(&mut m.at[i], &mut m.velocity[i], m.target[i], dt);
            }
            let goal = if m.leaving { 0.0 } else { 1.0 };
            m.fade += (goal - m.fade) * (1.0 - (-dt * 14.0).exp());
        }
        self.mark(false);
    }

    /// Whether screen `index` needs a frame for the glass; clears it.
    pub fn take_dirty(&mut self, index: usize) -> bool {
        self.dirty.get_mut(index).is_some_and(std::mem::take)
    }

    /// When the next animation frame is due, while a plate moves.
    pub fn deadline(&self) -> Option<Instant> {
        self.moving
            .iter()
            .any(|m| !m.settled())
            .then(|| Instant::now() + FRAME)
    }

    /// Draws screen `index`'s plates into `texture`, the frame about to be presented. Does nothing
    /// without plates there, or when the swapchain can't be copied from (`copy_src`).
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        index: usize,
        size: (u32, u32),
        texture: &wgpu::Texture,
        copy_src: bool,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let Some(monitor) = self.monitors.get(index).copied() else {
            return;
        };
        let instances = self.instances(&monitor, size);
        if instances.is_empty() || !copy_src {
            return;
        }
        let pipeline = self.pipeline.get_or_insert_with(|| Pipeline::new(gpu));
        let fresh = self.scratch[index].as_ref().is_none_or(|s| s.size != size);
        if fresh {
            self.scratch[index] = Some(Scratch::new(gpu, pipeline, size));
        }
        let Some(scratch) = self.scratch[index].as_ref() else {
            return;
        };

        // Copy only what the plates can sample: the strips along each plate's rim (the band and how
        // far it reaches inward), or the whole plate when a blur or a tint fills it.
        #[cfg(test)]
        let (copies, draw) = self.parts;
        #[cfg(not(test))]
        let (copies, draw) = (true, true);
        for glass in instances
            .chunks(2)
            .filter_map(|pair| pair.get(1))
            .filter(|_| copies)
        {
            for [x0, y0, x1, y1] in strips(glass, size) {
                let origin = wgpu::Origin3d { x: x0, y: y0, z: 0 };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        origin,
                        ..texture.as_image_copy()
                    },
                    wgpu::TexelCopyTextureInfo {
                        origin,
                        ..scratch.texture.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width: x1 - x0,
                        height: y1 - y0,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }

        if !draw {
            return;
        }
        // Shadows first, then glass: `instances` alternates them, so draw the even, then the odd.
        let (shadows, glasses): (Vec<Instance>, Vec<Instance>) =
            instances.chunks(2).map(|p| (p[0], p[1])).unzip();
        let ordered: Vec<Instance> = shadows.iter().chain(&glasses).copied().collect();
        let count = shadows.len() as u32;
        gpu.queue
            .write_buffer(&pipeline.instances, 0, bytemuck::cast_slice(&ordered));
        let uniforms = Uniforms {
            size: [size.0 as f32, size.1 as f32],
            _pad: [0.0; 2],
        };
        gpu.queue
            .write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let view = texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &scratch.bind_group, &[]);
        pass.set_vertex_buffer(0, pipeline.instances.slice(..));
        pass.draw(0..24, 0..count * 2);
    }

    /// This screen's plates as instances (shadow, glass, shadow, glass, ...), in its own px.
    fn instances(&self, monitor: &Rect, size: (u32, u32)) -> Vec<Instance> {
        let (sx, sy) = (
            size.0 as f32 / monitor.width.max(1) as f32,
            size.1 as f32 / monitor.height.max(1) as f32,
        );
        self.moving
            .iter()
            .filter(|m| m.fade > 0.004 && overlaps(m.reach(), monitor))
            .flat_map(|m| {
                let p = &m.plate;
                let rect = [
                    (m.at[0] - monitor.x as f32) * sx,
                    (m.at[1] - monitor.y as f32) * sy,
                    m.at[2] * sx,
                    m.at[3] * sy,
                ];
                let unit = |v: f32| v.clamp(0.0, 1.0);
                let tint = p.tint.map(|c| f32::from(c) / 255.0);
                let shape = [
                    p.radius.max(0.0) * sx,
                    unit(p.intensity),
                    unit(p.refraction),
                    unit(p.highlight),
                ];
                let base = Instance {
                    rect,
                    shape,
                    tint,
                    extra: [p.blur.clamp(0.0, 12.0), unit(m.fade), 0.0, 0.0],
                };
                let glass = Instance {
                    extra: [base.extra[0], base.extra[1], 1.0, 0.0],
                    ..base
                };
                [base, glass]
            })
            .collect()
    }
}

/// The rim band's width (`band_of` in glass.wgsl).
fn band_of(w: f32, h: f32, radius: f32) -> f32 {
    let r = radius.min(w.min(h) / 2.0);
    (r * 0.8 + 6.0).min(w.min(h) / 2.0 * 0.5)
}

/// The regions of the frame a plate samples, `[x0, y0, x1, y1]` clamped to the screen: its four
/// rim strips (the band, and the farthest the bend reaches inward), or the whole plate (with a
/// blur's reach) when a blur or a tint covers it.
fn strips(glass: &Instance, (sw, sh): (u32, u32)) -> Vec<[u32; 4]> {
    let [x, y, w, h] = glass.rect;
    let blur = glass.extra[0];
    let full = blur >= 0.5 || glass.tint[3] >= 0.004;
    let t = if full {
        w.max(h)
    } else {
        band_of(w, h, glass.shape[0]) * 1.5 + 2.0
    };
    let pad = blur.ceil() + 2.0;
    let (x, y, w, h) = (x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad);
    let (tw, th) = (t.min(w / 2.0), t.min(h / 2.0));
    let parts = if tw * 2.0 >= w || th * 2.0 >= h {
        vec![[x, y, x + w, y + h]]
    } else {
        vec![
            [x, y, x + w, y + th],
            [x, y + h - th, x + w, y + h],
            [x, y + th, x + tw, y + h - th],
            [x + w - tw, y + th, x + w, y + h - th],
        ]
    };
    let clamp = |v: f32, max: u32| v.round().clamp(0.0, max as f32) as u32;
    parts
        .into_iter()
        .map(|[x0, y0, x1, y1]| [clamp(x0, sw), clamp(y0, sh), clamp(x1, sw), clamp(y1, sh)])
        .filter(|[x0, y0, x1, y1]| x1 > x0 && y1 > y0)
        .collect()
}

/// Whether process `pid` still runs (a crashed writer's plates must not linger).
fn alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain handle queries; the handle is closed before returning.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let running =
            GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(handle);
        running
    }
}

impl Pipeline {
    fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::include_wgsl!("glass.wgsl"));
        let entry = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glass"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let buffer = |label: &str, size: usize, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            pipeline,
            layout,
            sampler,
            uniforms: buffer("glass", size_of::<Uniforms>(), wgpu::BufferUsages::UNIFORM),
            instances: buffer(
                "glass plates",
                size_of::<Instance>() * MAX_PLATES * 2,
                wgpu::BufferUsages::VERTEX,
            ),
        }
    }
}

impl Scratch {
    fn new(gpu: &Gpu, pipeline: &Pipeline, size: (u32, u32)) -> Self {
        let texture = gpu.texture(
            "glass behind",
            size,
            gpu.format,
            wgpu::TextureUsages::empty(),
            None,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pipeline.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                },
            ],
        });
        Self {
            size,
            texture,
            bind_group,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(id: &str, rect: [i32; 4]) -> Plate {
        Plate {
            id: id.into(),
            rect,
            radius: 18.0,
            intensity: 1.0,
            refraction: 0.6,
            highlight: 0.7,
            tint: [0; 4],
            blur: 0.0,
        }
    }

    fn pass(monitors: &[Rect]) -> GlassPass {
        GlassPass {
            monitors: monitors.to_vec(),
            moving: Vec::new(),
            last_step: None,
            dirty: vec![false; monitors.len()],
            pipeline: None,
            scratch: monitors.iter().map(|_| None).collect(),
            message: 0,
            parts: (true, true),
        }
    }

    const LEFT: Rect = Rect {
        x: -2560,
        y: 0,
        width: 2560,
        height: 1440,
    };
    const RIGHT: Rect = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };

    #[test]
    fn plates_grow_in_spring_to_new_rects_and_mark_only_their_screens() {
        let mut glass = pass(&[LEFT, RIGHT]);
        glass.set_plates(vec![plate("clock-1", [-2000, 100, 300, 200])]);
        assert!(
            glass.take_dirty(0) && !glass.take_dirty(1),
            "only the left screen has it"
        );
        assert!(glass.deadline().is_some(), "it grows in");
        let start = Instant::now();
        for i in 1..=60 {
            glass.advance(start + FRAME * i);
        }
        assert!(glass.deadline().is_none(), "settled within a second");
        assert!((glass.moving[0].at[2] - 300.0).abs() < 0.5);

        // Moved to the other screen: both redraw while it travels.
        glass.set_plates(vec![plate("clock-1", [100, 100, 300, 200])]);
        assert!(glass.take_dirty(0) && glass.take_dirty(1));
        // Removed: it fades, then goes.
        glass.set_plates(Vec::new());
        for i in 1..=90 {
            glass.advance(start + FRAME * (60 + i));
        }
        glass.advance(start + FRAME * 200);
        assert!(glass.moving.is_empty());
    }

    #[test]
    fn instances_are_in_screen_pixels_shadow_then_glass() {
        let mut glass = pass(&[LEFT]);
        glass.set_plates(vec![plate("a", [-2460, 50, 300, 200])]);
        glass.moving[0].at = glass.moving[0].target;
        glass.moving[0].fade = 1.0;
        let instances = glass.instances(&LEFT, (2560, 1440));
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].rect, [100.0, 50.0, 300.0, 200.0]);
        assert_eq!((instances[0].extra[2], instances[1].extra[2]), (0.0, 1.0));
        assert!(
            glass.instances(&RIGHT, (2560, 1440)).is_empty(),
            "not on a screen it doesn't touch"
        );
    }

    /// The pass's GPU cost: 24 widget-sized plates on a 2560x1440 frame. Run it with
    /// `cargo test -p mypaper-wallpaper glass_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "measures the GPU; run by hand"]
    fn glass_cost() {
        let Some(gpu) = Gpu::headless() else { return };
        let size: (u32, u32) = std::env::var("GLASS_BENCH_SIZE")
            .ok()
            .and_then(|s| {
                let (w, h) = s.split_once("x")?;
                Some((w.parse().ok()?, h.parse().ok()?))
            })
            .unwrap_or((2560, 1440));
        // Like a swapchain texture: a render target that can be copied from, never sampled.
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut glass = pass(&[RIGHT]);
        let plates = (0..24)
            .map(|i| {
                plate(
                    &format!("w{i}"),
                    [16 + (i % 8) * 312, 16 + (i / 8) * 208, 300, 200],
                )
            })
            .collect();
        glass.set_plates(plates);
        for m in &mut glass.moving {
            m.at = m.target;
            m.fade = 1.0;
        }
        // Many passes in one submission, so the GPU's own time shows, not the submit-and-wait
        // latency (which every frame pays anyway): cost = (t(51) - t(1)) / 50.
        let run = |glass: &mut GlassPass, passes: usize| {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            for _ in 0..passes {
                glass.encode(&gpu, 0, size, &target, true, &mut encoder);
            }
            gpu.queue.submit([encoder.finish()]);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        };
        let time = |glass: &mut GlassPass, passes: usize| {
            run(glass, passes);
            let start = Instant::now();
            for _ in 0..20 {
                run(glass, passes);
            }
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        };
        for keep in [24usize, 4, 1] {
            glass.moving.truncate(keep);
            for (name, parts) in [
                ("all", (true, true)),
                ("copies", (true, false)),
                ("draw", (false, true)),
            ] {
                glass.parts = parts;
                let one = time(&mut glass, 1);
                let many = time(&mut glass, 51);
                println!(
                    "glass {name}: {:.3} ms of GPU per frame with {keep} plates on {}x{}",
                    (many - one) / 50.0,
                    size.0,
                    size.1
                );
            }
        }
    }
}
