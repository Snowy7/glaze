//! WGSL shader wallpapers, with optional render scale (draw at a fraction
//! of the screen resolution, then upscale through the blitter).

use std::time::Duration;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use mypaper_theme::{Fit, LoadedTheme, ShaderWallpaper};

use super::blit::BlitQuad;
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
}

struct ScreenState {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Offscreen target when `scale < 1`.
    target: Option<Target>,
}

struct Target {
    size: (u32, u32),
    view: wgpu::TextureView,
    quad: BlitQuad,
}

impl ShaderSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &ShaderWallpaper) -> Result<Self> {
        let path = theme.asset(&config.source);
        let code = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("paper"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let pipeline = compile(gpu, &layout, &code).with_context(|| path.display().to_string())?;
        Ok(Self {
            pipeline,
            layout,
            interval: Duration::from_secs(1) / config.fps,
            scale: config.scale,
            screens: Vec::new(),
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
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("paper"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                }],
            });
            ScreenState {
                uniforms,
                bind_group,
                target: None,
            }
        })
    }
}

impl Source for ShaderSource {
    fn render(&mut self, gpu: &Gpu, frame: Frame<'_>) {
        let scale = self.scale;
        let pipeline = self.pipeline.clone();
        let state = self.state(gpu, frame.screen);

        let size = if scale < 1.0 {
            let w = ((frame.size.0 as f32 * scale).ceil() as u32).max(1);
            let h = ((frame.size.1 as f32 * scale).ceil() as u32).max(1);
            (w, h)
        } else {
            frame.size
        };
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

        if scale >= 1.0 {
            let mut pass = begin(
                frame.encoder,
                frame.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &state.bind_group, &[]);
            pass.draw(0..3, 0..1);
            return;
        }

        if state.target.as_ref().is_none_or(|t| t.size != size) {
            let texture = gpu.texture(
                "shader target",
                size,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                None,
            );
            let view = texture.create_view(&Default::default());
            let quad = gpu.blit.quad(&gpu.device, &view);
            state.target = Some(Target { size, view, quad });
        }
        let target = state.target.as_ref().expect("just created");
        {
            let mut pass = begin(
                frame.encoder,
                &target.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &state.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        target
            .quad
            .set(&gpu.queue, size, frame.size, Fit::Stretch, 1.0);
        let mut pass = begin(
            frame.encoder,
            frame.view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        target.quad.draw(&gpu.blit, &mut pass);
    }

    fn redraw(&self, _clock: Duration) -> Redraw {
        Redraw::Every(self.interval)
    }
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
}
