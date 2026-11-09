//! GPU core: one device shared by every screen and one surface per window.
//! What gets drawn lives in `sources`; this module only owns the device,
//! the swapchains and the shared fit/blit pipeline.

use std::cell::Cell;
use std::num::NonZeroIsize;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow, bail};
use glaze_config::GpuPreference;
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::sources::blit::Blitter;
use crate::window::Window;

#[path = "device_slot.rs"]
mod device_slot;
pub use device_slot::DeviceSlot;

pub struct Gpu {
    // Kept alive for as long as the surfaces made from it.
    _instance: wgpu::Instance,
    /// Also the glass overlays' (their capture device opens on the same adapter).
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Format of every screen's swapchain (non-sRGB: values are written as-is).
    pub format: wgpu::TextureFormat,
    /// Shared textured-quad pipeline with fit modes and opacity.
    pub blit: Blitter,
    pub preference: GpuPreference,
}

/// The device being opened on its own thread, from the first moment of the process.
///
/// Opening the device is the slowest part of starting: wgpu creates a D3D12 device on every
/// adapter to see what it supports before picking one, about 350 ms on a machine with an
/// integrated and a discrete GPU (docs/performance.md). Nothing in it needs a window, so it runs
/// while the main thread loads the theme and makes the windows.
pub struct Opening(std::thread::JoinHandle<Result<Opened>>);

/// What [`Opening`] produces.
pub struct Opened {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    preference: GpuPreference,
}

impl Opening {
    pub fn start(preference: GpuPreference) -> Self {
        Self(std::thread::spawn(move || {
            let instance = instance();
            let adapter = pick_adapter(&instance, preference)?;
            let (device, queue) = open_device(&adapter, crate::timing::requested())?;
            Ok(Opened {
                instance,
                adapter,
                device,
                queue,
                preference,
            })
        }))
    }

    fn wait(self) -> Result<Opened> {
        self.0
            .join()
            .map_err(|_| anyhow!("opening the GPU panicked"))?
    }
}

/// The adapter for `preference`. `Auto`, the default, renders on the adapter that drives the
/// screens (`display_gpu`): the RTX on a desktop with monitors on it, the iGPU on a laptop whose
/// panel hangs off the iGPU. A frame drawn elsewhere is copied across to be shown, and liquid
/// glass costs 2.5-2.9 ms per screen on the AMD iGPU against 0.1 ms on the RTX
/// (docs/glass.md). Integrated and high-performance pick by power class, as Windows does.
fn pick_adapter(instance: &wgpu::Instance, preference: GpuPreference) -> Result<wgpu::Adapter> {
    if !force_warp()
        && preference == GpuPreference::Auto
        && let Some(display) = glaze_desktop::display_gpu::display_adapter()
    {
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        if let Some(adapter) = adapters.into_iter().find(|a| luid(a) == Some(display.luid)) {
            return Ok(adapter);
        }
        log::warn!(
            "GPU: {} drives the screens but wgpu can't use it",
            display.name
        );
    }
    Ok(pollster::block_on(
        instance.request_adapter(&adapter_options(None, preference)),
    )?)
}

/// Diagnostic software rendering for clean installs and GPU-independent reproduction.
fn force_warp() -> bool {
    std::env::var_os("GLAZE_FORCE_WARP").is_some_and(|value| value == "1")
}

/// The DXGI adapter LUID behind a wgpu adapter.
fn luid(adapter: &wgpu::Adapter) -> Option<windows::Win32::Foundation::LUID> {
    use windows::Win32::Graphics::Dxgi::IDXGIAdapter1;
    use windows::core::Interface;
    // SAFETY: reads the description of the live DXGI adapter behind wgpu's.
    unsafe {
        let hal = adapter.as_hal::<wgpu::hal::api::Dx12>()?;
        let desc = hal.as_raw().cast::<IDXGIAdapter1>().ok()?.GetDesc1().ok()?;
        Some(desc.AdapterLuid)
    }
}

/// Windows' own choice by power class; `Auto` falls back to wgpu's first adapter.
fn adapter_options<'a, 'w>(
    surface: Option<&'a wgpu::Surface<'w>>,
    preference: GpuPreference,
) -> wgpu::RequestAdapterOptions<'a, 'w> {
    wgpu::RequestAdapterOptions {
        power_preference: match preference {
            GpuPreference::Integrated => wgpu::PowerPreference::LowPower,
            GpuPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
            GpuPreference::Auto => wgpu::PowerPreference::None,
        },
        compatible_surface: surface,
        force_fallback_adapter: force_warp(),
        apply_limit_buckets: false,
    }
}

impl Gpu {
    /// Reopen after a real device loss, retaining the swapchain format. Existing screens
    /// recreate their surfaces lazily; covered screens remain free of drawing resources.
    pub fn reopen(preference: GpuPreference, format: wgpu::TextureFormat) -> Result<Self> {
        let Opened {
            instance,
            adapter,
            device,
            queue,
            preference,
        } = Opening::start(preference).wait()?;
        let blit = Blitter::new(&device, format);
        Ok(Self {
            _instance: instance,
            adapter,
            device,
            queue,
            preference,
            format,
            blit,
        })
    }

    /// Takes the device `opening` made and creates a surface for each window (in order).
    pub fn new(
        opening: Opening,
        windows: &[Window],
    ) -> Result<(Self, Vec<wgpu::Surface<'static>>)> {
        let Opened {
            instance,
            mut adapter,
            mut device,
            mut queue,
            preference,
        } = opening.wait()?;
        let surfaces = windows
            .iter()
            .map(|w| create_surface(&instance, w))
            .collect::<Result<Vec<_>>>()?;
        // Picked without a surface: on DX12 every adapter presents to windows, but if this one
        // can't, pick again the slow way, with the screen to show on.
        if let Some(first) = surfaces.first()
            && !adapter.is_surface_supported(first)
        {
            adapter = pollster::block_on(
                instance.request_adapter(&adapter_options(Some(first), preference)),
            )?;
            (device, queue) = open_device(&adapter, crate::timing::requested())?;
        }
        let info = adapter.get_info();
        log::info!(
            "GPU: {} ({:?}), preference={preference:?}",
            info.name,
            info.backend
        );

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
            preference,
        };
        Ok((gpu, surfaces))
    }

    /// A device with no windows, for tests that render to textures, with timestamp queries
    /// where the adapter has them (the frame lab). `None` on a machine without a DX12 GPU.
    /// The integrated GPU, or the high-performance one with `GLAZE_LAB_GPU=high`.
    #[cfg(test)]
    pub fn headless() -> Option<Self> {
        let instance = instance();
        let preference = match std::env::var("GLAZE_LAB_GPU").as_deref() {
            Ok("high") => GpuPreference::HighPerformance,
            _ => GpuPreference::Integrated,
        };
        let adapter =
            pollster::block_on(instance.request_adapter(&adapter_options(None, preference)))
                .ok()?;
        eprintln!("lab GPU: {}", adapter.get_info().name);
        let (device, queue) = open_device(&adapter, true).ok()?;
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let blit = Blitter::new(&device, format);
        Some(Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            blit,
            preference: GpuPreference::Integrated,
        })
    }

    /// A surface for `window` (a screen whose swapchain was released gets a new one).
    pub fn surface(&self, window: &Window) -> Result<wgpu::Surface<'static>> {
        create_surface(&self._instance, window)
    }

    /// The device `opening` made, with no windows (the release lab and Studio's `--preview`
    /// child: the engine's real adapter choice and device, without surfaces).
    pub fn opened(opening: Opening) -> Self {
        let Opened {
            instance,
            adapter,
            device,
            queue,
            preference,
        } = opening.wait().expect("a DX12 GPU");
        log::info!(
            "GPU headless: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().device_type
        );
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let blit = Blitter::new(&device, format);
        Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            blit,
            preference,
        }
    }

    /// Drops the device and keeps what it was opened from (the release lab).
    #[cfg(test)]
    pub fn into_adapter(self) -> (wgpu::Instance, wgpu::Adapter) {
        (self._instance, self.adapter)
    }

    /// A new device on a kept adapter (the release lab).
    #[cfg(test)]
    pub fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Self {
        let (device, queue) = open_device(&adapter, false).expect("a device");
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let blit = Blitter::new(&device, format);
        Self {
            _instance: instance,
            adapter,
            device,
            queue,
            format,
            blit,
            preference: GpuPreference::Auto,
        }
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

    /// A sampled texture in `format`, optionally filled with tightly packed pixels. It can also
    /// be viewed as sRGB: Liquid Glass reads and draws linear colour (`glass_pass`).
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
            view_formats: &[format.add_srgb_suffix()],
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
    /// `None` once [`release`](Self::release)d; made again on the next [`acquire`](Self::acquire).
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    /// Whether `config` has been applied to `surface` yet (see `acquire`).
    configured: bool,
    pub window: Window,
    /// Hidden behind a maximized/fullscreen window (or minimized in dev mode).
    pub paused: bool,
    /// Needs a frame even if the source is static (new, resized, resumed, reloaded).
    pub dirty: bool,
    /// Unmodified last wallpaper, at its source's render scale. Never contains glass. Shared
    /// by every screen of the same render size when the source draws one frame for all.
    pub kept: Option<Rc<KeptFrame>>,
    pub glass_dirty: bool,
}

pub struct KeptFrame {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub quad: crate::sources::blit::BlitQuad,
    /// The engine frame whose wallpaper it holds, so screens that share it draw it once.
    pub drawn: Cell<Option<u32>>,
}

impl KeptFrame {
    pub fn new(gpu: &Gpu, size: (u32, u32)) -> Self {
        let texture = gpu.texture(
            "kept wallpaper",
            size,
            gpu.format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            None,
        );
        let view = texture.create_view(&Default::default());
        let quad = gpu.blit.quad(&gpu.device, &view);
        Self {
            texture,
            view,
            quad,
            drawn: Cell::new(None),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    /// A full-scale frame copies directly when supported; a scaled frame uses the upscale blit,
    /// which covers every pixel: no clear first, no blending (each a full-screen write here).
    pub fn restore(
        &self,
        gpu: &Gpu,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if target.size() == self.texture.size()
            && target.usage().contains(wgpu::TextureUsages::COPY_DST)
        {
            encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                target.as_image_copy(),
                target.size(),
            );
        } else {
            self.quad.set(
                &gpu.queue,
                self.size(),
                (target.width(), target.height()),
                glaze_theme::Fit::Stretch,
                1.0,
            );
            // SAFETY: `cover` writes every pixel and reads none of the target before it does.
            let load = wgpu::LoadOp::DontCare(unsafe { wgpu::LoadOpDontCare::enabled() });
            let mut pass = crate::sources::begin(encoder, view, load);
            self.quad.cover(&gpu.blit, &mut pass);
        }
    }
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
        // Liquid Glass draws through an sRGB view (`glass_pass`).
        config.view_formats = vec![gpu.format.add_srgb_suffix()];
        // Mailbox never blocks on vsync; our own timer paces frames.
        if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            config.present_mode = wgpu::PresentMode::Mailbox;
        }
        config.desired_maximum_frame_latency = 1;
        if caps.usages.contains(wgpu::TextureUsages::COPY_DST) {
            config.usage |= wgpu::TextureUsages::COPY_DST;
        }
        Ok(Self {
            surface: Some(surface),
            config,
            configured: false,
            window,
            paused: false,
            dirty: true,
            kept: None,
            glass_dirty: false,
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
            if let (true, Some(surface)) = (self.configured, &self.surface) {
                surface.configure(&gpu.device, &self.config);
            }
            self.dirty = true;
        }
    }

    /// The next swapchain image, or `None` if this frame should be skipped.
    /// The swapchain is created here on first use, so screens whose source
    /// draws with windows of its own (web, app) never pay for one.
    pub fn acquire(&mut self, gpu: &Gpu) -> Option<wgpu::SurfaceTexture> {
        let surface = match &mut self.surface {
            Some(surface) => surface,
            None => match gpu.surface(&self.window) {
                Ok(surface) => self.surface.insert(surface),
                Err(e) => {
                    log::error!("no swapchain: {e:#}");
                    return None;
                }
            },
        };
        if !self.configured {
            surface.configure(&gpu.device, &self.config);
            self.configured = true;
        }
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&gpu.device, &self.config);
                None
            }
            _ => None, // timeout / occluded: try next frame
        }
    }

    /// Drops the swapchain and the kept frame (every screen is covered; see
    /// `Engine::release`). The next [`acquire`](Self::acquire) makes the swapchain again.
    pub fn release(&mut self) {
        self.surface = None;
        self.configured = false;
        self.kept = None;
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
    // Labels stay in wgpu's own errors, but D3D12 gets none: wgpu opens a command list per render
    // pass and named each labelled one, pass and resource (SetName), every frame, for debuggers only.
    desc.flags |= wgpu::InstanceFlags::DISCARD_HAL_LABELS;
    wgpu::Instance::new(desc)
}

/// Fixed 4 MiB suballocation blocks, the allocator's minimum. Anything larger (every screen-sized
/// texture) gets a heap of exactly its own size. `MemoryUsage` grows blocks from 8 to 64 MiB and
/// reserved 124 MiB for four 2560x1440 textures (56 MiB). On both GPUs here every heap is private
/// memory; a released one comes back a few seconds later (docs/engine-notes.md, Memory).
const MEMORY_HINTS: wgpu::MemoryHints = wgpu::MemoryHints::Manual {
    suballocated_device_memory_block_size: 4 << 20..4 << 20,
};

/// Size of DX12's shader-visible view descriptor heap: every bind group's textures and buffers
/// live in it while the group exists. wgpu's default, 1,000,000, is allocated up front and cost
/// 92 MiB private here; the engine's bind groups (a few per screen, source and pyramid level, a
/// few hundred for a large glTF scene) fit in this many with room to spare.
const DESCRIPTORS: u32 = 16_384;

/// Opens the device and queue. Timestamp support costs no queries until telemetry or a
/// one-off covered refresh uses it.
fn open_device(adapter: &wgpu::Adapter, _timing: bool) -> Result<(wgpu::Device, wgpu::Queue)> {
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Glaze"),
        // Immediate data (every DX12 adapter has it): a shader wallpaper's inputs ride in its
        // draw. 16-bit normalized formats: the planes of 10-bit (P010) video.
        required_features:
            wgpu::Features::IMMEDIATES
                | adapter.features().intersection(
                    wgpu::Features::TEXTURE_FORMAT_16BIT_NORM | crate::timing::FEATURES,
                ),
        required_limits: wgpu::Limits {
            max_non_sampler_bindings: DESCRIPTORS,
            max_immediate_size: crate::sources::shader::PAPER_SIZE,
            ..wgpu::Limits::default().using_resolution(adapter.limits())
        },
        memory_hints: MEMORY_HINTS,
        ..Default::default()
    }))?;
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
