//! WGSL shader wallpapers, with optional render scale (draw at a fraction
//! of the screen resolution, then upscale through the blitter).

use std::time::Duration;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use mypaper_theme::{LoadedTheme, ShaderImageFormat, ShaderPrefilter, ShaderWallpaper};

use super::wic::{Budget, Decoder};
use super::{Frame, Redraw, Source, begin};
use crate::gpu::Gpu;

/// Appended *after* the theme shader, so compiler line numbers match the
/// theme file exactly (WGSL declarations are order-independent).
const PRELUDE: &str = include_str!("../prelude.wgsl");

/// Mirrors `struct Paper` in prelude.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    mouse: [f32; 2],
    time: f32,
    frame: u32,
    _pad: [f32; 2],
}

pub struct ShaderSource {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    interval: Duration,
    scale: f32,
    screens: Vec<Option<ScreenState>>,
    images: Vec<(u32, wgpu::TextureView)>,
    sampler: wgpu::Sampler,
}

struct ScreenState {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl ShaderSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &ShaderWallpaper) -> Result<Self> {
        let path = theme.asset(&config.source);
        let code = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        let mut images = Vec::new();
        if !config.images.is_empty() {
            let decoder = Decoder::new()?;
            for image in &config.images {
                // Raw unorm matches the former WebGL ShaderMaterial; no implicit sRGB decode.
                let decoded = match &image.source {
                    Some(source) => decoder.decode(
                        &theme.asset(source),
                        Budget::new(gpu, mypaper_theme::Fit::Cover),
                    )?,
                    None => super::wic::Decoded {
                        size: (1, 1),
                        bgra: vec![0, 0, 0, 255],
                    },
                };
                let effect = image
                    .prefilter
                    .as_ref()
                    .map(|effect| -> Result<_> {
                        let path = theme.asset(&effect.source);
                        Ok((
                            std::fs::read_to_string(&path)
                                .with_context(|| format!("reading {}", path.display()))?,
                            effect,
                        ))
                    })
                    .transpose()?;
                let texture = image_texture(
                    gpu,
                    decoded,
                    effect
                        .as_ref()
                        .map(|(code, effect)| (code.as_str(), *effect)),
                )?;
                images.push((image.binding, texture.create_view(&Default::default())));
                entries.extend([
                    wgpu::BindGroupLayoutEntry {
                        binding: image.binding,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float {
                                filterable: texture.format() != wgpu::TextureFormat::Rgba32Float,
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: image.binding + 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ]);
            }
        }
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader images"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("paper"),
                entries: &entries,
            });
        let pipeline = compile(gpu, &layout, &code).with_context(|| path.display().to_string())?;
        Ok(Self {
            pipeline,
            layout,
            interval: Duration::from_secs(1) / config.fps,
            scale: config.scale,
            screens: Vec::new(),
            images,
            sampler,
        })
    }

    fn state(&mut self, gpu: &Gpu, screen: usize) -> &mut ScreenState {
        if self.screens.len() <= screen {
            self.screens.resize_with(screen + 1, || None);
        }
        self.screens[screen].get_or_insert_with(|| {
            let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("paper"),
                size: size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }];
            for (binding, view) in &self.images {
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: *binding,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: binding + 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ]);
            }
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("paper"),
                layout: &self.layout,
                entries: &entries,
            });
            ScreenState {
                uniforms,
                bind_group,
            }
        })
    }
}

impl Source for ShaderSource {
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let pipeline = self.pipeline.clone();
        let state = self.state(gpu, frame.screen);

        let size = frame.size;
        let uniforms = Uniforms {
            resolution: [size.0 as f32, size.1 as f32],
            mouse: [
                frame.mouse[0] * size.0 as f32 / frame.size.0.max(1) as f32,
                frame.mouse[1] * size.1 as f32 / frame.size.1.max(1) as f32,
            ],
            time: frame.clock.as_secs_f32(),
            frame: frame.frame,
            _pad: [0.0; 2],
        };
        gpu.queue
            .write_buffer(&state.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &state.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_size(&self, size: (u32, u32)) -> (u32, u32) {
        (
            ((size.0 as f32 * self.scale).ceil() as u32).max(1),
            ((size.1 as f32 * self.scale).ceil() as u32).max(1),
        )
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::Every(self.interval)
    }
}

/// The former WebGL photo sources used trilinear mipmaps: implicit derivatives of their
/// refracted UVs are part of the look (the bokeh around a drop). Upload once, build once.
fn image_texture(
    gpu: &Gpu,
    decoded: super::wic::Decoded,
    effect: Option<(&str, &ShaderPrefilter)>,
) -> Result<wgpu::Texture> {
    let size = decoded.size;
    let levels = size.0.max(size.1).ilog2() + 1;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shader image"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.write_texture(&texture, &decoded.bgra);
    let blit = super::blit::Blitter::new(&gpu.device, wgpu::TextureFormat::Bgra8Unorm);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    for level in 1..levels {
        let view = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let source = view(level - 1);
        let target = view(level);
        let quad = blit.quad(&gpu.device, &source);
        quad.set(&gpu.queue, (1, 1), (1, 1), mypaper_theme::Fit::Stretch, 1.0);
        let mut pass = begin(
            &mut encoder,
            &target,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        quad.draw(&blit, &mut pass);
    }
    gpu.queue.submit([encoder.finish()]);
    if let Some((code, effect)) = effect {
        gpu.checked(|| prefilter(gpu, &texture, code, effect))
    } else {
        Ok(texture)
    }
}

/// Runs a theme-owned static image pass, then builds its mips. Artistic shader code and its
/// licence stay with the theme; this engine code only supplies the input texture/sampler.
fn prefilter(
    gpu: &Gpu,
    input: &wgpu::Texture,
    code: &str,
    effect: &ShaderPrefilter,
) -> wgpu::Texture {
    let [width, height] = effect.size.unwrap_or([input.width(), input.height()]);
    let format = match effect.format {
        ShaderImageFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        ShaderImageFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
    };
    let levels = if format == wgpu::TextureFormat::Rgba32Float {
        1
    } else {
        width.max(height).ilog2() + 1
    };
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("static image output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image prefilter"),
            source: wgpu::ShaderSource::Wgsl(code.into()),
        });
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("static image pass"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("static image pass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("static image pass"),
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
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
    let view = input.create_view(&Default::default());
    let bindings = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("photo prefilter"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let view = output.create_view(&wgpu::TextureViewDescriptor {
        mip_level_count: Some(1),
        ..Default::default()
    });
    {
        let mut pass = begin(&mut encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
    if levels == 1 {
        gpu.queue.submit([encoder.finish()]);
        return output;
    }
    let blit = super::blit::Blitter::new(&gpu.device, format);
    for level in 1..output.mip_level_count() {
        let view = |level| {
            output.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let source = view(level - 1);
        let target = view(level);
        let quad = blit.quad(&gpu.device, &source);
        quad.set(&gpu.queue, (1, 1), (1, 1), mypaper_theme::Fit::Stretch, 1.0);
        let mut pass = begin(
            &mut encoder,
            &target,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        quad.draw(&blit, &mut pass);
    }
    gpu.queue.submit([encoder.finish()]);
    output
}

/// Compiles a theme shader. Errors come back as text with line numbers.
fn compile(gpu: &Gpu, layout: &wgpu::BindGroupLayout, code: &str) -> Result<wgpu::RenderPipeline> {
    gpu.checked(|| {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("wallpaper"),
                source: wgpu::ShaderSource::Wgsl(format!("{code}\n{PRELUDE}").into()),
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("paper"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
        gpu.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("wallpaper"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("mypaper_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("mypaper_fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(gpu.format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn uniforms_match_the_wgsl_layout() {
        // vec2f, vec2f, f32, u32, vec2f padding: 32 bytes, a 16-byte multiple.
        assert_eq!(size_of::<super::Uniforms>(), 32);
    }

    #[test]
    fn bundled_shader_ports_compile_with_the_engine_prelude() {
        let gpu =
            crate::gpu::Gpu::headless().expect("DX12 is required to validate wallpaper ports");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let mut compiled = 0;
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            if !entry.path().join(mypaper_theme::MANIFEST).is_file() {
                continue;
            }
            let theme = mypaper_theme::LoadedTheme::load(entry.path()).unwrap();
            if let mypaper_theme::Wallpaper::Shader(config) = &theme.theme.wallpaper {
                super::ShaderSource::new(&gpu, &theme, config)
                    .unwrap_or_else(|error| panic!("{}: {error:#}", theme.dir.display()));
                compiled += 1;
            }
        }
        assert!(compiled >= 3, "expected the curated shader ports");
    }
}
