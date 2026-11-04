//! `scene` wallpapers: a glTF file drawn by a small forward renderer. The
//! whole scene is merged into one vertex/index buffer at load time, drawn
//! with 4x MSAA behind a sky gradient, and orbited by a camera that leans
//! toward the mouse. A still camera makes it a static wallpaper that is
//! drawn once.

use std::rc::{Rc, Weak};
use std::time::Duration;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use mypaper_theme::{Fit, LoadedTheme, SceneWallpaper};
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use windows::Win32::Foundation::HWND;

use super::blit::BlitQuad;
use super::{Frame, Redraw, Source, Visibility, begin};
use crate::gpu::Gpu;
use input::SceneInput;
use load::{SceneData, Vertex};
use math::{Sphere, View};

#[path = "scene_input.rs"]
mod input;
#[path = "scene_load.rs"]
mod load;
#[path = "scene_math.rs"]
mod math;

const SAMPLES: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Direction toward the sun in world space (it does not move with the camera).
const SUN_DIR: Vec3 = Vec3::new(0.55, 0.38, 0.45);
/// Warm dusk sunlight, linear.
const SUN_COLOR: Vec3 = Vec3::new(1.3, 0.82, 0.55);
/// Fog thickness: fraction hidden per scene radius of depth is `1 - exp(-FOG)`.
const FOG: f32 = 0.3;
/// How quickly the parallax lean catches up with the mouse, per second.
const LEAN_RATE: f32 = 5.0;

/// Mirrors `struct Globals` in scene.wgsl. Everything except the camera
/// part is fixed per scene, so it is built once and copied per frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    /// xyz; w = distance from the eye where fog starts.
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    /// rgb; w = density per world unit.
    fog: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
}

impl Globals {
    /// Lighting and sky derived from the theme's linear `background`: it is
    /// the top of the sky; the horizon warms toward a dusk glow, and the
    /// hemisphere ambient and fog take their colors from the two.
    fn new(background: Vec3, radius: f32) -> Self {
        let luma = background.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        let glow = Vec3::new(1.0, 0.45, 0.4) * 0.19 * (1.0 - luma.min(1.0));
        let horizon = (background + glow).min(Vec3::ONE);
        let sky = background * 3.0 + Vec3::splat(0.05);
        let ground = horizon * 1.2;
        let fog = background.lerp(horizon, 0.6);
        let v4 = |v: Vec3| v.extend(0.0).to_array();
        Self {
            view_proj: Default::default(),
            eye: [0.0; 4],
            sun_dir: v4(SUN_DIR.normalize()),
            sun_color: v4(SUN_COLOR),
            sky: v4(sky),
            ground: v4(ground),
            fog: fog.extend(FOG / radius).to_array(),
            bg_top: v4(background),
            bg_bottom: v4(horizon),
        }
    }

    fn with_view(mut self, view: &View) -> Self {
        self.view_proj = view.view_proj.to_cols_array_2d();
        self.eye = view.eye.extend(view.nearest).to_array();
        self
    }
}

pub struct SceneSource {
    sphere: Sphere,
    globals: Globals,
    /// Degrees per second.
    orbit: f32,
    parallax: f32,
    scale: f32,
    interval: Duration,
    /// Safety-net clear color (sRGB-encoded); the sky pass covers it.
    clear: wgpu::Color,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    batches: Vec<DrawBatch>,
    sky_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    screens: Vec<Option<ScreenState>>,
    /// The live multisampled attachments, shared by same-sized screens.
    attachments: Vec<Weak<Attachments>>,
    /// Drag-to-orbit for `interactive` scenes; `None` (and nothing listening) otherwise.
    input: Option<SceneInput>,
    /// Per screen: visible (not covered, not held). Input listens while any is.
    live: Vec<bool>,
}

/// One draw: an index range plus the texture bound for it.
struct DrawBatch {
    bind_group: wgpu::BindGroup,
    indices: std::ops::Range<u32>,
}

struct ScreenState {
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Smoothed mouse position, -1..1 (x right, y up).
    lean: Vec2,
    last_clock: Duration,
    target: Option<Target>,
}

/// Per-screen render targets, rebuilt only when the render size changes.
struct Target {
    size: (u32, u32),
    attachments: Rc<Attachments>,
    /// Single-sample color and its upscaler, when `scale < 1`.
    upscale: Option<(wgpu::TextureView, BlitQuad)>,
}

/// The multisampled color and depth that a pass draws into. Both are cleared
/// when a pass starts and discarded when it ends, so screens with the same
/// render size can take turns with one set instead of each holding its own
/// (about 120 MB of GPU memory per 1440p screen).
struct Attachments {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
}

impl Attachments {
    fn new(gpu: &Gpu, size: (u32, u32)) -> Self {
        let attachment = |label, format| {
            gpu.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            size,
            msaa: attachment("scene msaa", gpu.format),
            depth: attachment("scene depth", DEPTH_FORMAT),
        }
    }
}

/// The attachments for `size`: the ones another screen already uses, else new
/// ones. `cache` only watches them, so a set is freed with its last screen.
fn shared_attachments(
    cache: &mut Vec<Weak<Attachments>>,
    gpu: &Gpu,
    size: (u32, u32),
) -> Rc<Attachments> {
    cache.retain(|weak| weak.strong_count() > 0);
    let found = cache
        .iter()
        .filter_map(Weak::upgrade)
        .find(|attachments| attachments.size == size);
    found.unwrap_or_else(|| {
        let attachments = Rc::new(Attachments::new(gpu, size));
        cache.push(Rc::downgrade(&attachments));
        attachments
    })
}

impl SceneSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &SceneWallpaper) -> Result<Self> {
        let path = theme.asset(&config.source);
        let data = load::load(&path)?;
        let sphere = Sphere::around(data.vertices.iter().map(|v| Vec3::from(v.position)))
            .context("scene has no vertices")?;
        let background = Vec3::from(config.background);
        let [r, g, b] = background.to_array().map(math::linear_to_srgb);

        let gpu_resources = gpu
            .checked(|| GpuResources::new(gpu, &data))
            .with_context(|| path.display().to_string())?;
        Ok(Self {
            sphere,
            globals: Globals::new(background, sphere.radius.max(f32::EPSILON)),
            orbit: config.orbit,
            parallax: config.parallax,
            scale: config.scale,
            interval: Duration::from_secs(1) / config.fps,
            clear: wgpu::Color {
                r: r.into(),
                g: g.into(),
                b: b.into(),
                a: 1.0,
            },
            vertices: gpu_resources.vertices,
            indices: gpu_resources.indices,
            batches: gpu_resources.batches,
            sky_pipeline: gpu_resources.sky_pipeline,
            mesh_pipeline: gpu_resources.mesh_pipeline,
            globals_layout: gpu_resources.globals_layout,
            screens: Vec::new(),
            attachments: Vec::new(),
            input: config
                .interactive
                .then(SceneInput::start)
                .transpose()
                .inspect_err(|e| log::warn!("scene: no mouse input: {e:#}"))
                .unwrap_or(None),
            live: Vec::new(),
        })
    }

    fn animated(&self) -> bool {
        self.orbit != 0.0 || self.parallax != 0.0 || self.input.is_some()
    }
}

impl Source for SceneSource {
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let size = render_size(frame.size, self.scale);
        let state = screen_state(&mut self.screens, gpu, &self.globals_layout, frame.screen);
        if state.target.as_ref().is_none_or(|t| t.size != size) {
            let attachments = shared_attachments(&mut self.attachments, gpu, size);
            state.target = Some(Target::new(
                gpu,
                attachments,
                size,
                frame.size,
                self.scale < 1.0,
            ));
        }
        let target = state.target.as_ref().expect("just created");

        // Camera: orbit on the engine clock, lean toward the (eased) mouse.
        let (w, h) = (frame.size.0.max(1) as f32, frame.size.1.max(1) as f32);
        let mouse = Vec2::new(
            frame.mouse[0] / w * 2.0 - 1.0,
            1.0 - frame.mouse[1] / h * 2.0,
        )
        .clamp(Vec2::NEG_ONE, Vec2::ONE);
        let dt = frame.clock.saturating_sub(state.last_clock).as_secs_f32();
        state.last_clock = frame.clock;
        state.lean = math::approach(state.lean, mouse, dt, LEAN_RATE);
        // Dragging adds to the orbit, and (as lean, so it works with no parallax) tilts the camera.
        let drag = self
            .input
            .as_ref()
            .map(SceneInput::drag)
            .unwrap_or_default();
        let lean_gain = if self.input.is_some() {
            self.parallax.max(0.12)
        } else {
            self.parallax
        };
        let lean = if lean_gain > 0.0 {
            state.lean * (self.parallax / lean_gain) + Vec2::new(0.0, drag.pitch)
        } else {
            state.lean
        };
        let view = math::camera(
            self.sphere,
            w / h,
            math::azimuth(self.orbit, frame.clock) + drag.yaw,
            lean,
            lean_gain,
        );
        let globals = self.globals.with_view(&view);
        gpu.queue
            .write_buffer(&state.globals, 0, bytemuck::bytes_of(&globals));

        // Draw into the MSAA target, resolving straight to the screen, or to
        // the scaled texture that is then upscaled onto it.
        let resolve = target.upscale.as_ref().map_or(frame.view, |(view, _)| view);
        {
            let mut pass = frame
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("scene"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.attachments.msaa,
                        depth_slice: None,
                        resolve_target: Some(resolve),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.clear),
                            store: wgpu::StoreOp::Discard,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &target.attachments.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
            pass.set_bind_group(0, &state.bind_group, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);

            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for batch in &self.batches {
                pass.set_bind_group(1, &batch.bind_group, &[]);
                pass.draw_indexed(batch.indices.clone(), 0, 0..1);
            }
        }
        if let Some((_, quad)) = &target.upscale {
            let mut pass = begin(frame.encoder, frame.view, wgpu::LoadOp::Clear(self.clear));
            quad.draw(&gpu.blit, &mut pass);
        }
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        if self.animated() {
            Redraw::Every(self.interval)
        } else {
            Redraw::OnChange
        }
    }

    fn set_visibility(&mut self, screen: usize, _hwnd: HWND, visibility: Visibility) {
        let Some(input) = &mut self.input else { return };
        if self.live.len() <= screen {
            self.live.resize(screen + 1, true);
        }
        self.live[screen] = visibility == Visibility::Live;
        input.set_live(self.live.iter().any(|&live| live));
    }
}

/// The render resolution for a screen of `size` at render scale `scale`.
fn render_size(size: (u32, u32), scale: f32) -> (u32, u32) {
    if scale >= 1.0 {
        return size;
    }
    let scaled = |n: u32| ((n as f32 * scale).ceil() as u32).max(1);
    (scaled(size.0), scaled(size.1))
}

/// The state for `screen`, created on first use.
fn screen_state<'a>(
    screens: &'a mut Vec<Option<ScreenState>>,
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    screen: usize,
) -> &'a mut ScreenState {
    if screens.len() <= screen {
        screens.resize_with(screen + 1, || None);
    }
    screens[screen].get_or_insert_with(|| {
        let globals = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene globals"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        ScreenState {
            globals,
            bind_group,
            lean: Vec2::ZERO,
            last_clock: Duration::ZERO,
            target: None,
        }
    })
}

impl Target {
    /// `size` is the render size; `screen` the swapchain size to upscale to.
    fn new(
        gpu: &Gpu,
        attachments: Rc<Attachments>,
        size: (u32, u32),
        screen: (u32, u32),
        upscale: bool,
    ) -> Self {
        let upscale = upscale.then(|| {
            let texture = gpu.texture(
                "scene resolve",
                size,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                None,
            );
            let view = texture.create_view(&Default::default());
            let quad = gpu.blit.quad(&gpu.device, &view);
            quad.set(&gpu.queue, size, screen, Fit::Stretch, 1.0);
            (view, quad)
        });
        Self {
            size,
            attachments,
            upscale,
        }
    }
}

/// Everything uploaded to the GPU at load time.
struct GpuResources {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    batches: Vec<DrawBatch>,
    sky_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
}

impl GpuResources {
    fn new(gpu: &Gpu, data: &SceneData) -> Self {
        let device = &gpu.device;
        let buffer = |label, usage, contents: &[u8]| {
            device.create_buffer_init(&BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let vertices = buffer(
            "scene vertices",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&data.vertices),
        );
        let indices = buffer(
            "scene indices",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&data.indices),
        );

        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene globals"),
            entries: &[entry(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            )],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene texture"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });

        // Untextured batches sample a 1x1 white texture, so one shader serves all.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let upload = |label: &str, size, rgba: &[u8]| {
            gpu.texture(
                label,
                size,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureUsages::empty(),
                Some(rgba),
            )
            .create_view(&Default::default())
        };
        let white = upload("scene white", (1, 1), &[255; 4]);
        let textures: Vec<_> = data
            .textures
            .iter()
            .map(|t| upload("scene texture", t.size, &t.rgba))
            .collect();
        let batches = data
            .batches
            .iter()
            .map(|batch| {
                let view = batch.texture.map_or(&white, |i| &textures[i]);
                DrawBatch {
                    bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("scene texture"),
                        layout: &texture_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                        ],
                    }),
                    indices: batch.indices.clone(),
                }
            })
            .collect();

        let module = device.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let sky_layout = layout("scene sky", &[Some(&globals_layout)]);
        let mesh_layout = layout(
            "scene mesh",
            &[Some(&globals_layout), Some(&texture_layout)],
        );
        let pipeline = |label,
                        layout,
                        vs,
                        fs,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        depth_write,
                        depth_compare| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                // Both sides are drawn: the shader flips back-face normals.
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(gpu.format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sky_pipeline = pipeline(
            "scene sky",
            &sky_layout,
            "vs_sky",
            "fs_sky",
            &[],
            false,
            wgpu::CompareFunction::Always,
        );
        let mesh_pipeline = pipeline(
            "scene mesh",
            &mesh_layout,
            "vs_main",
            "fs_main",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3, 4 => Float32x3
                ],
            })],
            true,
            wgpu::CompareFunction::Less,
        );

        Self {
            vertices,
            indices,
            batches,
            sky_pipeline,
            mesh_pipeline,
            globals_layout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_structs_match_the_wgsl_layout() {
        // mat4x4f + eight vec4f.
        assert_eq!(size_of::<Globals>(), 64 + 8 * 16);
        // Three vec3f, one vec2f, tightly packed (see `VsIn`).
        assert_eq!(size_of::<Vertex>(), (3 + 3 + 2 + 3 + 3) * 4);
    }

    #[test]
    fn render_scale_shrinks_the_target_but_never_to_nothing() {
        assert_eq!(render_size((1920, 1080), 1.0), (1920, 1080));
        assert_eq!(render_size((1920, 1080), 0.5), (960, 540));
        assert_eq!(render_size((1001, 1), 0.25), (251, 1));
    }
}
