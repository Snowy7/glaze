//! Shared glTF renderer for scene bases and model layers.
//!
//! `SceneRenderer::load(gpu, path, &SceneOptions)` uploads a model once.
//! `render_into(gpu, encoder, color, depth, size, clock, mouse, transparent)`
//! records into a single-sample `gpu.format` color target. `mouse` is in output
//! pixels. Optional depth is an `R32Float` render attachment at output size:
//! linear camera depth, 0 at near, 1 at far (including the background).
//! Transparent output is premultiplied, including glTF BLEND materials.
//! `scale` reduces internal render resolution; the caller supplies rect size,
//! never a full-screen texture for a small model. Targets persist until resize.
//! `animated()` is false for no clip, orbit=0 and parallax=0: bake that output.
//! `render_into` uses view slot 0. For several screens before one submit, use
//! `render_into_for_screen(..., screen)`; mesh and material resources are shared.

use super::{Frame, Redraw, Source, Visibility, begin};
use crate::gpu::Gpu;
use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec2, Vec3};
use mypaper_theme::{LoadedTheme, SceneWallpaper};
use std::{path::Path, time::Duration};
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use windows::Win32::Foundation::HWND;
#[path = "scene_input.rs"]
mod input;
#[path = "scene_load.rs"]
mod load;
#[path = "scene_math.rs"]
mod math;
#[cfg(test)]
#[path = "scene/tests.rs"]
mod tests;
#[path = "scene/texture.rs"]
mod texture;
use load::{SceneData, Vertex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Environment {
    #[default]
    Sky,
    Color,
    Transparent,
}
#[derive(Debug, Clone)]
pub struct SceneOptions {
    pub orbit: f32,
    pub parallax: f32,
    /// Render resolution multiplier, 0.25..1 (model size comes from its rect).
    pub scale: f32,
    pub rotation: [f32; 3],
    pub light: [f32; 3],
    pub background: [f32; 3],
    pub environment: Environment,
    pub exposure: f32,
    pub animation: Option<String>,
}
impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            orbit: 0.0,
            parallax: 0.0,
            scale: 1.0,
            rotation: [0.0; 3],
            light: [0.4, 0.8, 0.5],
            background: [0.02, 0.025, 0.04],
            environment: Environment::Sky,
            exposure: 1.0,
            animation: None,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    background: [f32; 4],
    horizon: [f32; 4],
    clip: [f32; 4],
}
struct Material {
    bind: wgpu::BindGroup,
    blend: bool,
}
struct Target {
    size: (u32, u32),
    depth: wgpu::TextureView,
    depth_bind: wgpu::BindGroup,
    upscale: Option<wgpu::TextureView>,
}
struct ViewState {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    target: Option<Target>,
    lean: Vec2,
    clock: Duration,
}
pub struct SceneRenderer {
    options: SceneOptions,
    data: SceneData,
    sphere: math::Sphere,
    clip: Option<usize>,
    last_animation: Option<Duration>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    materials: Vec<Material>,
    order: Vec<usize>,
    globals_layout: wgpu::BindGroupLayout,
    depth_layout: wgpu::BindGroupLayout,
    opaque: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    blended_depth: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    depth: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    states: Vec<ViewState>,
}
impl SceneRenderer {
    pub fn load(gpu: &Gpu, path: &Path, options: &SceneOptions) -> Result<Self> {
        if !options.scale.is_finite()
            || !options.exposure.is_finite()
            || options
                .rotation
                .iter()
                .chain(&options.light)
                .chain(&options.background)
                .chain([&options.orbit, &options.parallax])
                .any(|x| !x.is_finite())
        {
            bail!("scene options must be finite");
        }
        let data = load::load(path)?;
        let sphere = math::Sphere::around(data.vertices.iter().map(|v| Vec3::from(v.position)))
            .context("empty model")?;
        let sphere = math::Sphere {
            radius: sphere.radius.max(0.001),
            ..sphere
        };
        let clip = options
            .animation
            .as_ref()
            .map(|name| {
                data.clips
                    .iter()
                    .position(|c| &c.name == name)
                    .with_context(|| format!("animation {name:?} not found"))
            })
            .transpose()?;
        gpu.checked(|| Self::upload(gpu, options.clone(), data, sphere, clip))
            .with_context(|| path.display().to_string())
    }
    pub fn animated(&self) -> bool {
        self.options.orbit != 0.0
            || self.options.parallax != 0.0
            || self.clip.is_some_and(|i| self.data.clips[i].duration > 0.0)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render_into(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_out: Option<&wgpu::TextureView>,
        size: (u32, u32),
        clock: Duration,
        mouse: [f32; 2],
        transparent: bool,
    ) {
        self.render_view(
            gpu,
            encoder,
            color_view,
            depth_out,
            size,
            clock,
            mouse,
            transparent,
            0,
            Vec2::ZERO,
        );
    }
    /// Same API with a persistent view slot for stacks shared by several screens.
    /// Distinct slots may be encoded before one submit, sharing mesh/material resources.
    #[allow(clippy::too_many_arguments)]
    pub fn render_into_for_screen(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_out: Option<&wgpu::TextureView>,
        size: (u32, u32),
        clock: Duration,
        mouse: [f32; 2],
        transparent: bool,
        screen: usize,
    ) {
        self.render_view(
            gpu,
            encoder,
            color_view,
            depth_out,
            size,
            clock,
            mouse,
            transparent,
            screen,
            Vec2::ZERO,
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn render_view(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_out: Option<&wgpu::TextureView>,
        size: (u32, u32),
        clock: Duration,
        mouse: [f32; 2],
        transparent: bool,
        screen: usize,
        drag: Vec2,
    ) {
        let size = (size.0.max(1), size.1.max(1));
        let internal = render_size(size, self.options.scale);
        if let Some(clip) = self.clip.filter(|_| self.last_animation != Some(clock)) {
            let duration = self.data.clips[clip].duration;
            let time = if duration > 0.0 {
                (clock.as_secs_f64() % f64::from(duration)) as f32
            } else {
                0.0
            };
            self.data.animate(clip, time);
            gpu.queue
                .write_buffer(&self.vertices, 0, bytemuck::cast_slice(&self.data.vertices));
            self.last_animation = Some(clock);
        }
        while self.states.len() <= screen {
            self.states.push(ViewState::new(gpu, &self.globals_layout));
        }
        let state = &mut self.states[screen];
        if state.target.as_ref().is_none_or(|t| t.size != internal) {
            state.target = Some(Target::new(
                gpu,
                internal,
                size,
                &self.depth_layout,
                self.options.scale < 1.0,
            ));
        }
        let mouse = Vec2::new(
            mouse[0] / size.0 as f32 * 2.0 - 1.0,
            1.0 - mouse[1] / size.1 as f32 * 2.0,
        )
        .clamp(Vec2::NEG_ONE, Vec2::ONE);
        state.lean = math::approach(
            state.lean,
            mouse,
            clock.saturating_sub(state.clock).as_secs_f32(),
            5.0,
        );
        state.clock = clock;
        let view = math::camera(
            self.sphere,
            size.0 as f32 / size.1 as f32,
            math::azimuth(self.options.orbit, clock) + drag.x,
            state.lean + Vec2::new(0.0, drag.y),
            self.options
                .parallax
                .max(if drag.y != 0.0 { 0.12 } else { 0.0 }),
        );
        let r = self.options.rotation.map(f32::to_radians);
        let rotation = Quat::from_euler(glam::EulerRot::XYZ, r[0], r[1], r[2]);
        let model = Mat4::from_translation(self.sphere.center)
            * Mat4::from_quat(rotation)
            * Mat4::from_translation(-self.sphere.center);
        let bg = Vec3::from(self.options.background);
        let horizon = bg + Vec3::new(0.12, 0.07, 0.035);
        let transparent = transparent || self.options.environment == Environment::Transparent;
        let globals = Globals {
            view_proj: view.view_proj.to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            eye: view.eye.extend(0.0).to_array(),
            light: Vec3::from(self.options.light)
                .normalize_or(Vec3::Y)
                .extend(self.options.exposure.max(0.0))
                .to_array(),
            background: bg
                .extend(if self.options.environment == Environment::Color {
                    1.0
                } else {
                    0.0
                })
                .to_array(),
            horizon: horizon.extend(0.0).to_array(),
            clip: [view.near, view.far, 0.0, 0.0],
        };
        gpu.queue
            .write_buffer(&state.uniform, 0, bytemuck::bytes_of(&globals));
        let target = state.target.as_ref().expect("created target");
        let color = target.upscale.as_ref().map_or(color_view, |v| v);
        // Opaque first; BLEND primitives back-to-front, without allocating a sort buffer.
        self.order.sort_unstable_by(|&a, &b| {
            let a = &self.data.batches[a];
            let b = &self.data.batches[b];
            let aa = self.materials[a.material].blend;
            let bb = self.materials[b.material].blend;
            aa.cmp(&bb).then_with(|| {
                if aa {
                    model
                        .transform_point3(b.center)
                        .distance_squared(view.eye)
                        .total_cmp(&model.transform_point3(a.center).distance_squared(view.eye))
                } else {
                    a.material.cmp(&b.material)
                }
            })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene PBR"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &state.bind, &[]);
            if !transparent {
                pass.set_pipeline(&self.sky);
                pass.draw(0..3, 0..1);
            }
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for &i in &self.order {
                let batch = &self.data.batches[i];
                let m = &self.materials[batch.material];
                pass.set_pipeline(if m.blend { &self.blend } else { &self.opaque });
                pass.set_bind_group(1, &m.bind, &[]);
                pass.draw_indexed(batch.indices.clone(), 0, 0..1);
            }
            // Depth consumers see the nearest visible translucent surface too.
            // Keep this after color blending so it cannot disturb transparency order.
            if depth_out.is_some() {
                pass.set_pipeline(&self.blended_depth);
                for &i in &self.order {
                    let batch = &self.data.batches[i];
                    let material = &self.materials[batch.material];
                    if material.blend {
                        pass.set_bind_group(1, &material.bind, &[]);
                        pass.draw_indexed(batch.indices.clone(), 0, 0..1);
                    }
                }
            }
        }
        if target.upscale.is_some() {
            let mut pass = begin(
                encoder,
                color_view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(&self.resolve);
            pass.set_bind_group(0, &state.bind, &[]);
            pass.set_bind_group(1, &target.depth_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(depth) = depth_out {
            let mut pass = begin(encoder, depth, wgpu::LoadOp::Clear(wgpu::Color::WHITE));
            pass.set_pipeline(&self.depth);
            pass.set_bind_group(0, &state.bind, &[]);
            pass.set_bind_group(1, &target.depth_bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
    fn upload(
        gpu: &Gpu,
        options: SceneOptions,
        data: SceneData,
        sphere: math::Sphere,
        clip: Option<usize>,
    ) -> Self {
        let d = &gpu.device;
        let buffer = |label, usage, contents: &[u8]| {
            d.create_buffer_init(&BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let vertices = buffer(
            "scene vertices",
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
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
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let globals_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene globals"),
            entries: &[entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, uniform)],
        });
        let mut entries = vec![
            entry(0, wgpu::ShaderStages::FRAGMENT, uniform),
            entry(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ];
        for i in 2..7 {
            entries.push(entry(
                i,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ));
        }
        for i in 7..11 {
            entries.push(entry(
                i,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ));
        }
        let material_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene materials"),
            entries: &entries,
        });
        let depth_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene linear depth"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let upload = |label, size, pixels: &[u8], srgb| {
            gpu.texture(
                label,
                size,
                if srgb {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                wgpu::TextureUsages::empty(),
                Some(pixels),
            )
            .create_view(&Default::default())
        };
        let white = upload("scene white", (1, 1), &[255; 4], false);
        let normal = upload("scene flat normal", (1, 1), &[128, 128, 255, 255], false);
        let textures: Vec<_> = data
            .textures
            .iter()
            .enumerate()
            .map(|(image, t)| {
                let used = |srgb| {
                    data.materials.iter().any(|m| {
                        m.textures.iter().enumerate().any(|(i, index)| {
                            index.is_some_and(|index| {
                                data.texture_info[index].image == image
                                    && (i == 0 || i == 3) == srgb
                            })
                        })
                    })
                };
                (
                    used(true).then(|| texture::upload(gpu, t, true)),
                    used(false).then(|| texture::upload(gpu, t, false)),
                )
            })
            .collect();
        let samplers: Vec<_> = data
            .texture_info
            .iter()
            .map(|info| texture::sampler(gpu, info))
            .collect();
        let materials = data
            .materials
            .iter()
            .map(|m| {
                let u = buffer(
                    "scene material",
                    wgpu::BufferUsages::UNIFORM,
                    bytemuck::bytes_of(&m.uniform),
                );
                let mut entries = vec![
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: u.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(
                            m.textures[0].map_or(&sampler, |i| &samplers[i]),
                        ),
                    },
                ];
                for (i, texture) in m.textures.iter().enumerate() {
                    let view = texture.map_or(if i == 2 { &normal } else { &white }, |t| {
                        if i == 0 || i == 3 {
                            textures[data.texture_info[t].image]
                                .0
                                .as_ref()
                                .expect("uploaded color map")
                        } else {
                            textures[data.texture_info[t].image]
                                .1
                                .as_ref()
                                .expect("uploaded data map")
                        }
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 + 2,
                        resource: wgpu::BindingResource::TextureView(view),
                    });
                    if i > 0 {
                        entries.push(wgpu::BindGroupEntry {
                            binding: i as u32 + 6,
                            resource: wgpu::BindingResource::Sampler(
                                texture.map_or(&sampler, |i| &samplers[i]),
                            ),
                        });
                    }
                }
                Material {
                    bind: d.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("scene material"),
                        layout: &material_layout,
                        entries: &entries,
                    }),
                    blend: m.uniform.alpha[0] == 2.0,
                }
            })
            .collect();
        let module = d.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let output_module = d.create_shader_module(wgpu::include_wgsl!("scene_output.wgsl"));
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let mesh_layout = layout(
            "scene mesh",
            &[Some(&globals_layout), Some(&material_layout)],
        );
        let sky_layout = layout("scene sky", &[Some(&globals_layout)]);
        let depth_pipeline_layout =
            layout("scene depth", &[Some(&globals_layout), Some(&depth_layout)]);
        let pipeline = |module: &wgpu::ShaderModule,
                        label,
                        layout,
                        vs,
                        fs,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        depth: Option<wgpu::DepthStencilState>,
                        format,
                        blend,
                        write_mask| {
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: Default::default(),
                depth_stencil: depth,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let depth_state = |write| {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let vertex_layout = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32x2,4=>Float32x4],
        })];
        let opaque = pipeline(
            &module,
            "scene opaque",
            &mesh_layout,
            "vs_main",
            "fs_main",
            &vertex_layout,
            depth_state(true),
            gpu.format,
            None,
            wgpu::ColorWrites::ALL,
        );
        let blend = pipeline(
            &module,
            "scene blend",
            &mesh_layout,
            "vs_main",
            "fs_main",
            &vertex_layout,
            depth_state(false),
            gpu.format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            wgpu::ColorWrites::ALL,
        );
        let blended_depth = pipeline(
            &module,
            "scene blended depth",
            &mesh_layout,
            "vs_main",
            "fs_nearest",
            &vertex_layout,
            depth_state(true),
            gpu.format,
            None,
            wgpu::ColorWrites::empty(),
        );
        let sky = pipeline(
            &module,
            "scene sky",
            &sky_layout,
            "vs_sky",
            "fs_sky",
            &[],
            depth_state(false),
            gpu.format,
            None,
            wgpu::ColorWrites::ALL,
        );
        let depth = pipeline(
            &output_module,
            "scene linear depth",
            &depth_pipeline_layout,
            "vs",
            "fs_depth",
            &[],
            None,
            wgpu::TextureFormat::R32Float,
            None,
            wgpu::ColorWrites::ALL,
        );
        let resolve = pipeline(
            &output_module,
            "scene upscale",
            &depth_pipeline_layout,
            "vs",
            "fs_color",
            &[],
            None,
            gpu.format,
            None,
            wgpu::ColorWrites::ALL,
        );
        let order = (0..data.batches.len()).collect();
        Self {
            options,
            data,
            sphere,
            clip,
            last_animation: None,
            vertices,
            indices,
            materials,
            order,
            globals_layout,
            depth_layout,
            opaque,
            blend,
            blended_depth,
            sky,
            depth,
            resolve,
            states: Vec::new(),
        }
    }
}
impl ViewState {
    fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene view"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene view"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            uniform,
            bind,
            target: None,
            lean: Vec2::ZERO,
            clock: Duration::ZERO,
        }
    }
}
impl Target {
    fn new(
        gpu: &Gpu,
        size: (u32, u32),
        _screen: (u32, u32),
        layout: &wgpu::BindGroupLayout,
        upscale: bool,
    ) -> Self {
        let depth = gpu
            .texture(
                "scene hardware depth",
                size,
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                None,
            )
            .create_view(&Default::default());
        let upscale = upscale.then(|| {
            gpu.texture(
                "scene scaled color",
                size,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                None,
            )
            .create_view(&Default::default())
        });
        let dummy = gpu
            .texture(
                "scene unused color",
                (1, 1),
                gpu.format,
                wgpu::TextureUsages::empty(),
                None,
            )
            .create_view(&Default::default());
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let depth_bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene output"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        upscale.as_ref().unwrap_or(&dummy),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            size,
            depth,
            depth_bind,
            upscale,
        }
    }
}
fn render_size(size: (u32, u32), scale: f32) -> (u32, u32) {
    let scale = scale.clamp(0.25, 1.0);
    (
        ((size.0 as f32 * scale).ceil() as u32).max(1),
        ((size.1 as f32 * scale).ceil() as u32).max(1),
    )
}

/// Legacy `kind: scene` uses precisely the same renderer as a stack model.
pub struct SceneSource {
    renderer: SceneRenderer,
    interval: Duration,
    input: Option<input::SceneInput>,
    live: Vec<bool>,
}
impl SceneSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &SceneWallpaper) -> Result<Self> {
        let options = SceneOptions {
            orbit: config.orbit,
            parallax: config.parallax,
            scale: config.scale,
            background: config.background,
            exposure: config.exposure,
            environment: match config.environment {
                mypaper_theme::Environment::Sky => Environment::Sky,
                mypaper_theme::Environment::Color => Environment::Color,
                mypaper_theme::Environment::Transparent => Environment::Transparent,
            },
            ..Default::default()
        };
        Ok(Self {
            renderer: SceneRenderer::load(gpu, &theme.asset(&config.source), &options)?,
            interval: Duration::from_secs(1) / config.fps.max(1),
            input: config
                .interactive
                .then(input::SceneInput::start)
                .transpose()
                .inspect_err(|e| log::warn!("scene input: {e:#}"))
                .unwrap_or(None),
            live: vec![true; mypaper_desktop::monitors().len().max(1)],
        })
    }
}
impl Source for SceneSource {
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let drag = self
            .input
            .as_ref()
            .map(input::SceneInput::drag)
            .unwrap_or_default();
        self.renderer.render_view(
            gpu,
            frame.encoder,
            frame.view,
            None,
            frame.size,
            frame.clock,
            frame.mouse,
            false,
            frame.screen,
            Vec2::new(drag.yaw, drag.pitch),
        );
    }
    fn redraw(&self, _clock: Duration) -> Redraw {
        if self.renderer.animated() || self.input.is_some() {
            Redraw::Every(self.interval)
        } else {
            Redraw::OnChange
        }
    }
    fn set_visibility(&mut self, screen: usize, _hwnd: HWND, visibility: Visibility) {
        if let Some(input) = &mut self.input {
            if self.live.len() <= screen {
                self.live.resize(screen + 1, true);
            }
            self.live[screen] = visibility == Visibility::Live;
            input.set_live(self.live.iter().any(|&v| v));
        }
    }
}
