//! GPU core: one device shared by every screen and one surface per window.
//! What gets drawn lives in `sources`; this module only owns the device,
//! the swapchains and the shared fit/blit pipeline.

use std::num::NonZeroIsize;

use anyhow::{Context, Result, anyhow, bail};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::sources::blit::Blitter;
use crate::window::Window;

pub struct Gpu {
    // Kept alive for as long as the surfaces made from it.
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Format of every screen's swapchain (non-sRGB: values are written as-is).
    pub format: wgpu::TextureFormat,
    /// Shared textured-quad pipeline with fit modes and opacity.
    pub blit: Blitter,
}

impl Gpu {
    /// Creates the device and a surface for each window (in order).
    pub fn new(windows: &[Window]) -> Result<(Self, Vec<wgpu::Surface<'static>>)> {
        let instance = instance();

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

        let (device, queue) = open_device(&adapter)?;

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

        let blit = Blitter::new(&device, format);
        let gpu = Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            blit,
        };
        Ok((gpu, surfaces))
    }

    /// A device with no windows, for tests that render to textures. `None`
    /// on a machine without a DX12 GPU.
    #[cfg(test)]
    pub fn headless() -> Option<Self> {
        let instance = instance();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;
        let (device, queue) = open_device(&adapter).ok()?;
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let blit = Blitter::new(&device, format);
        Some(Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            blit,
        })
    }

    /// Runs `build` with validation errors captured instead of panicking,
    /// so a broken asset is reported and the previous frame keeps running.
    pub fn checked<T>(&self, build: impl FnOnce() -> T) -> Result<T> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let value = build();
        if let Some(err) = pollster::block_on(scope.pop()) {
            bail!("{err}");
        }
        Ok(value)
    }

    /// A sampled texture in `format`, optionally filled with tightly packed pixels.
    pub fn texture(
        &self,
        label: &str,
        (width, height): (u32, u32),
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        pixels: Option<&[u8]>,
    ) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: usage | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if let Some(pixels) = pixels {
            self.write_texture(&texture, pixels);
        }
        texture
    }

    /// Replaces a whole texture's contents with tightly packed pixels.
    pub fn write_texture(&self, texture: &wgpu::Texture, pixels: &[u8]) {
        let size = texture.size();
        let block = texture.format().block_copy_size(None).unwrap_or(4);
        self.queue.write_texture(
            texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * block),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }
}

/// One window being rendered to.
pub struct Screen {
    // Field order matters: the surface must drop before its window.
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Whether `config` has been applied to `surface` yet (see `acquire`).
    configured: bool,
    pub window: Window,
    /// Hidden behind a maximized/fullscreen window (or minimized in dev mode).
    pub paused: bool,
    /// Needs a frame even if the source is static (new, resized, resumed, reloaded).
    pub dirty: bool,
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
        Ok(Self {
            surface,
            config,
            configured: false,
            window,
            paused: false,
            dirty: true,
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
            if self.configured {
                self.surface.configure(&gpu.device, &self.config);
            }
            self.dirty = true;
        }
    }

    /// The next swapchain image, or `None` if this frame should be skipped.
    /// The swapchain is created here on first use, so screens whose source
    /// draws with windows of its own (web, app) never pay for one.
    pub fn acquire(&mut self, gpu: &Gpu) -> Option<wgpu::SurfaceTexture> {
        if !self.configured {
            self.surface.configure(&gpu.device, &self.config);
            self.configured = true;
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                None
            }
            _ => None, // timeout / occluded: try next frame
        }
    }
}

/// The wgpu instance: DX12 only, compiling shaders with FXC.
///
/// wgpu's default (`Auto`) loads whatever `dxcompiler.dll` it finds on the
/// search path, such as the Vulkan SDK's, and silently falls back to FXC
/// without it, so the same build behaved differently per machine. FXC ships
/// with Windows, compiles all our shaders, and leaves the 21 MB
/// `dxcompiler.dll` out of the process: 5-8 MB less working set per theme
/// (see docs/engine-notes.md).
fn instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    wgpu::Instance::new(desc)
}

/// Opens the device and queue, routing GPU errors to the log.
fn open_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let (device, queue) = pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mypaper"),
            // 16-bit normalized formats: the planes of 10-bit (P010) video.
            required_features: adapter
                .features()
                .intersection(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }),
    )?;
    device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("GPU: {e}")));
    Ok((device, queue))
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
