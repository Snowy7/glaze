//! Liquid glass under UI windows (widgets, and the bars when they ask): the plates other
//! processes publish (`mypaper_theme::glass`) drawn into the wallpaper's own frame, so the glass
//! is exactly as live as the wallpaper.
//!
//! Per screen and frame, only when that screen has plates: the frame around each plate's rim (its
//! band, and as far out as the rim reaches for what it shows) is copied into a scratch texture,
//! in as few copies as cover it (strips of plates side by side merge), then one instance per plate
//! is drawn (see glass.wgsl). Only the rim band samples; a clear centre is not rasterized, a
//! tinted one is a flat wash, so the cost follows the rims, not the plates' area. Nothing is
//! allocated while no plate exists. Plates spring to new rects (and grow in, and fade out), the
//! "liquid" morph when a widget appears, moves or resizes; while one moves, the screens it
//! touches redraw at 60 fps, even a still wallpaper, then once more when it settles, and stop.

use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use mypaper_desktop::Rect;
use mypaper_theme::glass::{self, Plate};

use crate::gpu::Gpu;

/// How fast plates follow their rects: a spring of this angular frequency (rad/s), a little
/// underdamped so a moved plate settles with the faintest wobble.
const OMEGA: f32 = 26.0;
const DAMPING: f32 = 0.82;
/// A redraw every frame while something moves.
const FRAME: Duration = Duration::from_millis(16);
/// Most plates drawn.
const MAX_PLATES: usize = 96;
/// Vertices per plate: the four quads of the rim band and the centre's (glass.wgsl).
const VERTICES: u32 = 30;
/// Strips are merged into one copy while that copies at most this much more than they cover.
const MERGE_SLACK: f32 = 1.25;

/// Mirrors `Plate` in glass.wgsl: four vec4s per instance.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
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

    /// The rect it covers, now or soon, with a pixel for the antialiased edge (for which screens
    /// redraw). The glass never draws outside its plate.
    fn reach(&self) -> [f32; 4] {
        let lo = [
            self.at[0].min(self.target[0]),
            self.at[1].min(self.target[1]),
        ];
        let hi = [
            (self.at[0] + self.at[2]).max(self.target[0] + self.target[2]),
            (self.at[1] + self.at[3]).max(self.target[1] + self.target[3]),
        ];
        [
            lo[0] - 1.0,
            lo[1] - 1.0,
            hi[0] - lo[0] + 2.0,
            hi[1] - lo[1] + 2.0,
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
    /// Per screen: the last frame drawn there had plates (so it must redraw once they leave).
    shown: Vec<bool>,
    pipeline: Option<Pipeline>,
    scratch: Vec<Option<Scratch>>,
    /// Reused every frame: this screen's instances and the regions to copy.
    instances: Vec<Instance>,
    copies: Vec<[u32; 4]>,
    /// The registered message that says a plates file changed (`glass::MESSAGE`).
    pub message: u32,
    /// Benchmarks only: which halves of the pass run (copies, draw).
    #[cfg(test)]
    parts: (bool, bool),
}

impl GlassPass {
    pub fn new(monitors: Option<&[Rect]>) -> Self {
        let name: Vec<u16> = glass::MESSAGE.encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name that outlives the call.
        let message = unsafe {
            windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(windows::core::PCWSTR(
                name.as_ptr(),
            ))
        };
        let mut pass = Self::with(monitors.map(<[Rect]>::to_vec).unwrap_or_default(), message);
        pass.reload();
        pass
    }

    fn with(monitors: Vec<Rect>, message: u32) -> Self {
        let count = monitors.len();
        Self {
            monitors,
            moving: Vec::new(),
            last_step: None,
            dirty: vec![false; count],
            shown: vec![false; count],
            pipeline: None,
            scratch: (0..count).map(|_| None).collect(),
            instances: Vec::new(),
            copies: Vec::new(),
            message,
            #[cfg(test)]
            parts: (true, true),
        }
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
        if !self.moving.iter().all(Moving::settled) {
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
        if self.last_step.is_some() && self.moving.iter().all(Moving::settled) {
            // Just settled, in this very frame: it redraws wherever plates were or are, so none is
            // left half-faded or on a screen it moved off.
            self.last_step = None;
            self.moving.retain(|m| !m.leaving);
            for (dirty, shown) in self.dirty.iter_mut().zip(&self.shown) {
                *dirty |= *shown;
            }
            self.mark(true);
        }
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
        let mut instances = std::mem::take(&mut self.instances);
        self.fill(&mut instances, &monitor, size);
        let drawn = !instances.is_empty() && copy_src;
        if let Some(shown) = self.shown.get_mut(index) {
            *shown = drawn;
        }
        if drawn {
            self.draw(gpu, index, size, texture, &instances, encoder);
        }
        self.instances = instances;
    }

    fn draw(
        &mut self,
        gpu: &Gpu,
        index: usize,
        size: (u32, u32),
        texture: &wgpu::Texture,
        instances: &[Instance],
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let pipeline = self.pipeline.get_or_insert_with(|| Pipeline::new(gpu));
        if self.scratch[index].as_ref().is_none_or(|s| s.size != size) {
            self.scratch[index] = Some(Scratch::new(gpu, pipeline, size));
        }
        let Some(scratch) = self.scratch[index].as_ref() else {
            return;
        };

        #[cfg(test)]
        let (copies, draw) = self.parts;
        #[cfg(not(test))]
        let (copies, draw) = (true, true);
        if copies {
            regions(instances, size, &mut self.copies);
            for &[x0, y0, x1, y1] in &self.copies {
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
        gpu.queue
            .write_buffer(&pipeline.instances, 0, bytemuck::cast_slice(instances));
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
        pass.draw(0..VERTICES, 0..instances.len() as u32);
    }

    /// This screen's plates as instances, in its own px, into `out`.
    fn fill(&self, out: &mut Vec<Instance>, monitor: &Rect, size: (u32, u32)) {
        out.clear();
        let (sx, sy) = (
            size.0 as f32 / monitor.width.max(1) as f32,
            size.1 as f32 / monitor.height.max(1) as f32,
        );
        let unit = |v: f32| v.clamp(0.0, 1.0);
        out.extend(
            self.moving
                .iter()
                .filter(|m| m.fade > 0.004 && overlaps(m.reach(), monitor))
                .map(|m| {
                    let p = &m.plate;
                    Instance {
                        rect: [
                            (m.at[0] - monitor.x as f32) * sx,
                            (m.at[1] - monitor.y as f32) * sy,
                            m.at[2] * sx,
                            m.at[3] * sy,
                        ],
                        shape: [
                            p.radius.max(0.0) * sx,
                            unit(p.intensity),
                            unit(p.refraction),
                            unit(p.highlight),
                        ],
                        tint: p.tint.map(|c| f32::from(c) / 255.0),
                        extra: [p.blur.clamp(0.0, 12.0) * sx, unit(m.fade), 0.0, 0.0],
                    }
                }),
        );
    }
}

/// The rim band's width (`band_of` in glass.wgsl).
fn band_of(w: f32, h: f32, radius: f32) -> f32 {
    let r = radius.min(w.min(h) / 2.0);
    (r * 0.5 + 5.0).min(w.min(h) / 2.0 * 0.45)
}

/// The parts of the frame `instances` sample, `[x0, y0, x1, y1]` clamped to the screen, into
/// `out`: each rim's four strips (its band, and as far out as it reaches, `reach_of` in
/// glass.wgsl), or the whole plate for a blur; then strips that cover nearly the same pixels as
/// their union (one plate's four, or the facing edges of plates side by side) are merged, since
/// a copy costs more than its pixels.
fn regions(instances: &[Instance], (sw, sh): (u32, u32), out: &mut Vec<[u32; 4]>) {
    out.clear();
    let mut parts: Vec<[f32; 4]> = Vec::with_capacity(instances.len() * 4);
    for glass in instances {
        let [x, y, w, h] = glass.rect;
        let blur = glass.extra[0];
        let band = band_of(w, h, glass.shape[0]);
        // The farthest a rim pixel samples: its reach out, the chromatic split, a filter pixel.
        let reach = (band * glass.shape[2] * glass.shape[1]) * 1.06 + blur + 2.0;
        let (x0, y0, x1, y1) = (x - reach, y - reach, x + w + reach, y + h + reach);
        let t = band + reach;
        if blur >= 0.5 || t * 2.0 >= (w + 2.0 * reach).min(h + 2.0 * reach) {
            parts.push([x0, y0, x1, y1]);
        } else {
            parts.extend([
                [x0, y0, x1, y0 + t],
                [x0, y1 - t, x1, y1],
                [x0, y0 + t, x0 + t, y1 - t],
                [x1 - t, y0 + t, x1, y1 - t],
            ]);
        }
    }
    merge(&mut parts);
    let clamp = |v: f32, max: u32| v.round().clamp(0.0, max as f32) as u32;
    out.extend(
        parts
            .into_iter()
            .map(|[x0, y0, x1, y1]| [clamp(x0, sw), clamp(y0, sh), clamp(x1, sw), clamp(y1, sh)])
            .filter(|[x0, y0, x1, y1]| x1 > x0 && y1 > y0),
    );
}

/// Merges rects (`[x0, y0, x1, y1]`) while a union covers at most [`MERGE_SLACK`] times the
/// pixels of the two it replaces (overlaps count twice, which only helps a merge).
fn merge(parts: &mut Vec<[f32; 4]>) {
    let area = |r: &[f32; 4]| (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0);
    // Sweeps until one changes nothing; a grown rect keeps absorbing the rest of its sweep.
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < parts.len() {
            let mut j = i + 1;
            while j < parts.len() {
                let (a, b) = (parts[i], parts[j]);
                let union = [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ];
                if area(&union) <= (area(&a) + area(&b)) * MERGE_SLACK {
                    parts[i] = union;
                    parts.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            return;
        }
    }
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
                size_of::<Instance>() * MAX_PLATES,
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
            radius: 22.0,
            intensity: 1.0,
            refraction: 0.6,
            highlight: 0.55,
            tint: [0; 4],
            blur: 0.0,
        }
    }

    fn pass(monitors: &[Rect]) -> GlassPass {
        GlassPass::with(monitors.to_vec(), 0)
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

        // Moved to the other screen: both redraw while it travels, and once more when it has
        // settled, so none of it is left on the screen it left.
        glass.set_plates(vec![plate("clock-1", [100, 100, 300, 200])]);
        assert!(glass.take_dirty(0) && glass.take_dirty(1));
        glass.shown = vec![true, true];
        for i in 1..=60 {
            glass.advance(start + FRAME * (60 + i));
            glass.take_dirty(0);
            glass.take_dirty(1);
        }
        glass.advance(start + FRAME * 121);
        assert!(glass.deadline().is_none());

        // Removed: it fades, then goes, with a last frame on the screen it was on.
        glass.set_plates(Vec::new());
        glass.shown = vec![false, true];
        for i in 1..=90 {
            glass.advance(start + FRAME * (130 + i));
            glass.take_dirty(1);
        }
        glass.advance(start + FRAME * 300);
        assert!(glass.moving.is_empty());
    }

    #[test]
    fn settling_redraws_the_screens_that_showed_plates_once() {
        let mut glass = pass(&[LEFT, RIGHT]);
        glass.set_plates(vec![plate("a", [100, 100, 300, 200])]);
        glass.take_dirty(1);
        glass.shown = vec![true, true]; // it was on the left screen a moment ago
        let start = Instant::now();
        let mut last = (false, false);
        for i in 1..=80 {
            glass.advance(start + FRAME * i);
            last = (glass.take_dirty(0), glass.take_dirty(1));
            if glass.deadline().is_none() {
                break;
            }
        }
        assert_eq!(last, (true, true), "the settling frame redraws both");
        glass.advance(start + FRAME * 90);
        assert_eq!(
            (glass.take_dirty(0), glass.take_dirty(1)),
            (false, false),
            "then nothing"
        );
    }

    #[test]
    fn instances_are_one_per_plate_in_screen_pixels() {
        let mut glass = pass(&[LEFT]);
        glass.set_plates(vec![plate("a", [-2460, 50, 300, 200])]);
        glass.moving[0].at = glass.moving[0].target;
        glass.moving[0].fade = 1.0;
        let mut out = Vec::new();
        glass.fill(&mut out, &LEFT, (2560, 1440));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rect, [100.0, 50.0, 300.0, 200.0]);
        assert_eq!(out[0].shape[0], 22.0);
        glass.fill(&mut out, &RIGHT, (2560, 1440));
        assert!(out.is_empty(), "not on a screen it doesn't touch");
    }

    #[test]
    fn copies_cover_the_rims_and_merge_side_by_side() {
        let mut glass = pass(&[RIGHT]);
        // A row of four widgets with 8 px gaps, and one far away.
        let mut plates: Vec<Plate> = (0..4)
            .map(|i| plate(&format!("w{i}"), [16 + i * 308, 16, 300, 200]))
            .collect();
        plates.push(plate("far", [2000, 1000, 300, 200]));
        glass.set_plates(plates);
        for m in &mut glass.moving {
            m.at = m.target;
            m.fade = 1.0;
        }
        let mut instances = Vec::new();
        glass.fill(&mut instances, &RIGHT, (2560, 1440));
        let mut copies = Vec::new();
        regions(&instances, (2560, 1440), &mut copies);
        assert!(copies.len() < 4 * 5, "merged: {} copies", copies.len());
        // Every pixel a rim samples is copied: the plates' edges and a little outside them.
        let inside = |x: u32, y: u32| {
            copies
                .iter()
                .any(|c| x >= c[0] && x < c[2] && y >= c[1] && y < c[3])
        };
        for &(x, y) in &[
            (16, 16),
            (315, 215),
            (14, 100),
            (2000, 1100),
            (2302, 1205),
            (2150, 994),
        ] {
            assert!(inside(x, y), "({x}, {y}) is not copied: {copies:?}");
        }
        // The clear centres are not.
        assert!(!inside(166, 116) && !inside(2150, 1100));
        // Clamped to the screen.
        assert!(copies.iter().all(|c| c[2] <= 2560 && c[3] <= 1440));
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
        // latency (which every frame pays anyway): cost = (t(51) - t(1)) / 50. Encoding is timed
        // on its own (it runs before the submit), so the GPU's share is the rest.
        let run = |glass: &mut GlassPass, passes: usize| {
            let start = Instant::now();
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            for _ in 0..passes {
                glass.encode(&gpu, 0, size, &target, true, &mut encoder);
            }
            let commands = encoder.finish();
            let encoded = start.elapsed();
            gpu.queue.submit([commands]);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            (encoded, start.elapsed())
        };
        let time = |glass: &mut GlassPass, passes: usize| {
            run(glass, passes);
            let (mut encode, mut total) = (Duration::ZERO, Duration::ZERO);
            for _ in 0..20 {
                let (e, t) = run(glass, passes);
                encode += e;
                total += t;
            }
            let ms = |d: Duration| d.as_secs_f64() * 1000.0 / 20.0;
            (ms(encode), ms(total))
        };
        for keep in [24usize, 4, 1] {
            glass.moving.truncate(keep);
            for (name, parts) in [
                ("all", (true, true)),
                ("copies", (true, false)),
                ("draw", (false, true)),
            ] {
                glass.parts = parts;
                let (encode_one, one) = time(&mut glass, 1);
                let (encode_many, many) = time(&mut glass, 51);
                let encode = (encode_many - encode_one) / 50.0;
                println!(
                    "glass {name}: {:.3} ms of GPU, {encode:.3} ms of CPU encoding per frame with {keep} plates on {}x{}",
                    (many - one) / 50.0 - encode,
                    size.0,
                    size.1
                );
            }
        }
    }

    /// Draws a few widget-sized plates over a picture, to look at the glass without a desktop:
    /// `GLASS_PREVIEW_IN=wallpaper.bmp GLASS_PREVIEW_OUT=out.bmp cargo test -p mypaper-wallpaper
    /// glass_preview -- --ignored` (24- or 32-bit BMPs).
    #[test]
    #[ignore = "writes a picture; run by hand"]
    fn glass_preview() {
        let (Ok(input), Ok(output)) = (
            std::env::var("GLASS_PREVIEW_IN"),
            std::env::var("GLASS_PREVIEW_OUT"),
        ) else {
            return eprintln!("set GLASS_PREVIEW_IN and GLASS_PREVIEW_OUT");
        };
        let Some(gpu) = Gpu::headless() else { return };
        let (size, bgra) = read_bmp(&std::fs::read(input).unwrap());
        let frame = gpu.texture(
            "frame",
            size,
            gpu.format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            Some(&bgra),
        );
        let screen = Rect {
            x: 0,
            y: 0,
            width: size.0 as i32,
            height: size.1 as i32,
        };
        let plates = vec![
            plate("calendar", [40, 40, 300, 300]),
            plate("clock", [380, 40, 300, 300]),
            plate("note", [720, 40, 200, 200]),
            Plate {
                radius: 36.0,
                ..plate("pill", [40, 380, 420, 72])
            },
            Plate {
                tint: [16, 16, 16, 40],
                ..plate("tinted", [500, 380, 300, 140])
            },
        ];
        let mut glass = pass(&[screen]);
        glass.set_plates(plates);
        for m in &mut glass.moving {
            m.at = m.target;
            m.fade = 1.0;
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        glass.encode(&gpu, 0, size, &frame, true, &mut encoder);
        let row = (size.0 * 4).next_multiple_of(256);
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview"),
            size: u64::from(row * size.1),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            frame.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size.1),
                },
            },
            frame.size(),
        );
        gpu.queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = readback.slice(..).get_mapped_range().unwrap();
        let pixels: Vec<u8> = data
            .chunks(row as usize)
            .flat_map(|r| r[..size.0 as usize * 4].to_vec())
            .collect();
        std::fs::write(output, write_bmp(size, &pixels)).unwrap();
    }

    /// A 24- or 32-bit BMP as tightly packed BGRA, top row first.
    fn read_bmp(file: &[u8]) -> ((u32, u32), Vec<u8>) {
        let u32_at = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap());
        let offset = u32_at(10) as usize;
        let width = u32_at(18);
        let height = u32_at(22) as i32;
        let bits = u16::from_le_bytes([file[28], file[29]]);
        assert!(bits == 24 || bits == 32, "{bits}-bit BMPs aren't read");
        let bpp = usize::from(bits / 8);
        let stride = (width as usize * bpp).next_multiple_of(4);
        let rows = height.unsigned_abs();
        let mut out = Vec::with_capacity((width * rows * 4) as usize);
        for y in 0..rows as usize {
            let src = if height > 0 { rows as usize - 1 - y } else { y };
            let row = &file[offset + src * stride..][..width as usize * bpp];
            for px in row.chunks(bpp) {
                out.extend([px[0], px[1], px[2], 255]);
            }
        }
        ((width, rows), out)
    }

    /// Tightly packed BGRA, top row first, as a top-down 32-bit BMP.
    fn write_bmp((width, height): (u32, u32), bgra: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(54 + bgra.len());
        let size = 54 + bgra.len() as u32;
        out.extend(b"BM");
        out.extend(size.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(width.to_le_bytes());
        out.extend((-(height as i32)).to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend([0u8; 24]);
        out.extend(bgra);
        out
    }
}
