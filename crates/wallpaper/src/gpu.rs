//! GPU side: one device shared by every screen, one surface per window, and
//! shader compilation that never takes the wallpaper down on a typo.

use std::num::NonZeroIsize;

use anyhow::{Context, Result, anyhow, bail};
use bytemuck::{Pod, Zeroable};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::window::Window;

/// Appended *after* the theme shader, so compiler line numbers match the
/// theme file exactly (WGSL declarations are order-independent).
const PRELUDE: &str = include_str!("prelude.wgsl");

/// Mirrors `struct Paper` in prelude.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub struct Uniforms {
    pub resolution: [f32; 2],
    pub mouse: [f32; 2],
    pub time: f32,
    pub frame: u32,
    pub _pad: [f32; 2],
}

pub struct Gpu {
    // Kept alive for as long as the surfaces made from it.
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}

impl Gpu {
    /// Creates the device and a surface for each window (in order).
    pub fn new(windows: &[Window]) -> Result<(Self, Vec<wgpu::Surface<'static>>)> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);

        let surfaces = windows
            .iter()
            .map(|w| create_surface(&instance, w))
            .collect::<Result<Vec<_>>>()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // Integrated GPU on laptops: a wallpaper should never wake the dGPU.
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: surfaces.first(),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("mypaper"),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))?;
        device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("GPU: {e}")));

        // Non-sRGB target: shader output is used as-is, matching Shadertoy-style code.
        let caps = surfaces
            .first()
            .context("no screens")?
            .get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or(caps.formats.first().copied())
            .context("surface has no formats")?;

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("paper"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let gpu = Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            bind_layout,
            pipeline_layout,
        };
        Ok((gpu, surfaces))
    }

    /// Compiles a theme shader. Errors come back as text (with line numbers)
    /// instead of panicking, so a bad save keeps the last good frame running.
    pub fn compile(&self, source: &str) -> Result<wgpu::RenderPipeline> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("wallpaper"),
                source: wgpu::ShaderSource::Wgsl(format!("{source}\n{PRELUDE}").into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("wallpaper"),
                layout: Some(&self.pipeline_layout),
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
                    targets: &[Some(self.format.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
        if let Some(err) = pollster::block_on(scope.pop()) {
            bail!("{err}");
        }
        Ok(pipeline)
    }
}

/// One window being rendered to.
pub struct Screen {
    // Field order matters: the surface must drop before its window.
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pub window: Window,
    pub paused: bool,
}

impl Screen {
    pub fn new(gpu: &Gpu, surface: wgpu::Surface<'static>, window: Window) -> Result<Self> {
        let (width, height) = window.client_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        if !caps.formats.contains(&gpu.format) {
            bail!("monitor does not support {:?}", gpu.format);
        }
        let mut config = surface
            .get_default_config(&gpu.adapter, width.max(1), height.max(1))
            .ok_or_else(|| anyhow!("surface not supported by adapter"))?;
        config.format = gpu.format;
        config.view_formats = vec![];
        // Mailbox never blocks on vsync; our own timer paces frames.
        if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            config.present_mode = wgpu::PresentMode::Mailbox;
        }
        config.desired_maximum_frame_latency = 1;
        surface.configure(&gpu.device, &config);

        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("paper"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("paper"),
            layout: &gpu.bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Ok(Self {
            surface,
            config,
            uniforms,
            bind_group,
            window,
            paused: false,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Picks up window resizes (dev mode). Cheap when nothing changed.
    pub fn sync_size(&mut self, gpu: &Gpu) {
        let (w, h) = self.window.client_size();
        if w > 0 && h > 0 && (w, h) != self.size() {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&gpu.device, &self.config);
        }
    }

    /// Draws one frame. `pipeline: None` (shader never compiled) draws a
    /// dim red screen so a broken theme is obvious, not silently black.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        pipeline: Option<&wgpu::RenderPipeline>,
        uniforms: &Uniforms,
    ) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                return;
            }
            _ => return, // timeout / occluded: try next frame
        };
        gpu.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let clear = if pipeline.is_some() {
                wgpu::Color::BLACK
            } else {
                wgpu::Color {
                    r: 0.18,
                    g: 0.0,
                    b: 0.02,
                    a: 1.0,
                }
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wallpaper"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(pipeline) = pipeline {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
}

fn create_surface(instance: &wgpu::Instance, window: &Window) -> Result<wgpu::Surface<'static>> {
    let mut handle =
        Win32WindowHandle::new(NonZeroIsize::new(window.0.0 as isize).context("null window")?);
    // SAFETY: querying our own module handle.
    handle.hinstance = NonZeroIsize::new(unsafe { GetModuleHandleW(None)? }.0 as isize);
    // SAFETY: `Screen` drops the surface before the window it was made from.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(WindowsDisplayHandle::new().into()),
            raw_window_handle: handle.into(),
        })?
    };
    Ok(surface)
}

#[cfg(test)]
mod tests {
    #[test]
    fn uniforms_match_the_wgsl_layout() {
        // vec2f, vec2f, f32, u32, vec2f padding: 32 bytes, 16-byte multiple.
        assert_eq!(size_of::<super::Uniforms>(), 32);
    }
}
