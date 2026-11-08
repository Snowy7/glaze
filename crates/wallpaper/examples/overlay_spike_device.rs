//! N0.2's capture-device measurement: which D3D11 device feeds Windows.Graphics.Capture for
//! the engine's DX12 (wgpu) glass overlays, and what it costs.
//!
//! ```text
//! cargo run --release -p glaze-wallpaper --example overlay_spike_device -- <none|11on12|d3d11>
//! ```
//!
//! Each mode is its own process: wgpu opened the way the engine opens it (low-power adapter,
//! DX12, FXC), then, unless `none`, a capture device of that kind on the same adapter, a
//! one-buffer capture session on the primary monitor, and a 512x512 crop texture that wgpu
//! samples. It prints private MB after each step, and the CPU time of 300 crop updates (the
//! copy from the captured frame into the crop, the sync with wgpu's queue, a wgpu pass that
//! samples it).
//! - `11on12`: D3D11On12 on wgpu's own D3D12 device and queue; the crop is a wgpu-side D3D12
//!   texture wrapped for D3D11 (acquire, copy, release, flush on wgpu's queue).
//! - `d3d11`: a separate D3D11 device on the same adapter; the crop is a shared D3D12 texture
//!   opened in D3D11, synced by a shared fence that wgpu's queue waits on.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use wgpu::hal::api::Dx12;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureDirtyRegionMode, GraphicsCaptureItem,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HANDLE, HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D11on12::{
    D3D11_RESOURCE_FLAGS, D3D11On12CreateDevice, ID3D11On12Device,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IUnknown, Interface};

const CROP: u32 = 512;
const UPDATES: u32 = 300;

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "none".into());
    // SAFETY: per-thread COM init for the WinRT capture calls, kept for the process.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    if mode == "frames" {
        return frames::run();
    }
    if mode == "border" {
        return frames::border();
    }
    if let Some(kind) = mode.strip_prefix("swapchain-") {
        return swapchains::run(kind);
    }
    let base = private_mb();
    let (adapter, device, queue) = open_wgpu()?;
    println!("adapter: {}", adapter.get_info().name);
    settle();
    let wgpu_mb = private_mb();
    println!("private: {base:.1} MB at start, {wgpu_mb:.1} MB with wgpu");
    if mode == "none" {
        return Ok(());
    }

    // SAFETY: the raw DX12 objects behind wgpu's adapter, device and queue, used while they live.
    let (dxgi_adapter, device12, queue12) = unsafe {
        let a = adapter.as_hal::<Dx12>().context("not DX12")?;
        let d = device.as_hal::<Dx12>().context("not DX12")?;
        (
            a.as_raw().cast::<IDXGIAdapter>()?,
            d.raw_device().clone(),
            d.raw_queue().clone(),
        )
    };

    let capture: Box<dyn CaptureDevice> = match mode.as_str() {
        "11on12" => Box::new(On12::new(&device12, &queue12)?),
        "d3d11" => Box::new(Separate::new(&dxgi_adapter, &device12)?),
        other => bail!("unknown mode {other}"),
    };
    settle();
    let device_mb = private_mb();
    println!(
        "private: {device_mb:.1} MB with the {mode} device (+{:.1})",
        device_mb - wgpu_mb
    );

    // One-buffer session on the primary monitor.
    // SAFETY: a plain monitor lookup.
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: a live monitor handle.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
    let size = item.Size()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        capture.winrt(),
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    session.SetIsCursorCaptureEnabled(false)?;
    let _ = session.SetIsBorderRequired(false);
    session.SetDirtyRegionMode(GraphicsCaptureDirtyRegionMode::ReportOnly)?;
    let (tx, rx) = mpsc::channel();
    pool.FrameArrived(&windows::Foundation::TypedEventHandler::new(
        move |pool: windows_core::Ref<Direct3D11CaptureFramePool>, _| {
            if let Some(pool) = pool.as_ref()
                && let Ok(frame) = pool.TryGetNextFrame()
            {
                let _ = tx.send(frame);
            }
            Ok(())
        },
    ))?;
    session.StartCapture()?;
    let started = Instant::now();
    let frame = rx
        .recv_timeout(Duration::from_secs(5))
        .context("no frame in 5 s")?;
    println!(
        "first frame after {:.1} ms, {}x{}",
        started.elapsed().as_secs_f64() * 1000.0,
        size.Width,
        size.Height
    );
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    // SAFETY: the frame's own texture, used while the frame is held.
    let source: ID3D11Texture2D = unsafe { access.GetInterface()? };
    settle();
    let session_mb = private_mb();
    println!(
        "private: {session_mb:.1} MB with the session and one held frame (+{:.1})",
        session_mb - device_mb
    );

    // The crop texture wgpu samples, and a pass that samples it (the stand-in for the glass).
    let crop = capture.crop(&device)?;
    let sampler = Sampler::new(&device, &crop);
    settle();
    let crop_mb = private_mb();
    println!(
        "private: {crop_mb:.1} MB with the crop (+{:.1})",
        crop_mb - session_mb
    );

    let cpu0 = cpu_ms();
    let wall = Instant::now();
    for i in 0..UPDATES {
        let x = (i * 7) % (size.Width as u32 - CROP);
        capture.update(&source, x, &queue12)?;
        sampler.draw(&device, &queue);
    }
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let cpu = cpu_ms() - cpu0;
    println!(
        "{UPDATES} updates: {:.3} ms CPU each ({:.3} ms wall each)",
        cpu / f64::from(UPDATES),
        wall.elapsed().as_secs_f64() * 1000.0 / f64::from(UPDATES)
    );
    settle();
    println!("private: {:.1} MB after the updates", private_mb());
    drop(frame);
    session.Close()?;
    pool.Close()?;
    Ok(())
}

/// What each kind of capture device provides.
trait CaptureDevice {
    /// The WinRT device the frame pool allocates on.
    fn winrt(&self) -> &IDirect3DDevice;
    /// A CROP x CROP texture wgpu can sample and this device can write.
    fn crop(&self, device: &wgpu::Device) -> Result<wgpu::Texture>;
    /// Copies the frame's CROP x CROP block at (x, 0) into the crop, ordered before wgpu's next
    /// submission on `queue`.
    fn update(&self, frame: &ID3D11Texture2D, x: u32, queue: &ID3D12CommandQueue) -> Result<()>;
}

struct On12 {
    on12: ID3D11On12Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
    device12: ID3D12Device,
    wrapped: std::cell::OnceCell<ID3D11Resource>,
}

impl On12 {
    fn new(device12: &ID3D12Device, queue12: &ID3D12CommandQueue) -> Result<Self> {
        let mut device = None;
        let mut context = None;
        let queue: IUnknown = queue12.cast()?;
        // SAFETY: wgpu's live device and queue; out pointers outlive the call.
        unsafe {
            D3D11On12CreateDevice(
                device12,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT.0,
                None,
                Some(&[Some(queue)]),
                0,
                Some(&mut device),
                Some(&mut context),
                None,
            )?;
        }
        let device: ID3D11Device = device.context("no device")?;
        Ok(Self {
            on12: device.cast()?,
            context: context.context("no context")?,
            winrt: winrt_device(&device)?,
            device12: device12.clone(),
            wrapped: Default::default(),
        })
    }
}

impl CaptureDevice for On12 {
    fn winrt(&self) -> &IDirect3DDevice {
        &self.winrt
    }

    fn crop(&self, device: &wgpu::Device) -> Result<wgpu::Texture> {
        let resource = committed(&self.device12, D3D12_HEAP_FLAG_NONE)?;
        let flags = D3D11_RESOURCE_FLAGS {
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let mut wrapped: Option<ID3D11Resource> = None;
        // SAFETY: a live D3D12 resource; simultaneous access keeps it in COMMON both sides.
        unsafe {
            self.on12.CreateWrappedResource(
                &resource,
                &flags,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COMMON,
                &mut wrapped,
            )?;
        }
        let _ = self.wrapped.set(wrapped.context("not wrapped")?);
        Ok(import(device, resource))
    }

    fn update(&self, frame: &ID3D11Texture2D, x: u32, _queue: &ID3D12CommandQueue) -> Result<()> {
        let wrapped = self.wrapped.get().context("no crop")?;
        let resources = [Some(wrapped.clone())];
        // SAFETY: live resources on this device; the box lies inside the frame.
        unsafe {
            self.on12.AcquireWrappedResources(&resources);
            self.context
                .CopySubresourceRegion(wrapped, 0, 0, 0, 0, frame, 0, Some(&crop_box(x)));
            self.on12.ReleaseWrappedResources(&resources);
            // Submits the copy to wgpu's queue, ahead of wgpu's next submission.
            self.context.Flush();
        }
        Ok(())
    }
}

struct Separate {
    device: ID3D11Device,
    context: ID3D11DeviceContext4,
    winrt: IDirect3DDevice,
    device12: ID3D12Device,
    fence11: ID3D11Fence,
    fence12: ID3D12Fence,
    value: std::cell::Cell<u64>,
    crop11: std::cell::OnceCell<ID3D11Texture2D>,
}

impl Separate {
    fn new(adapter: &IDXGIAdapter, device12: &ID3D12Device) -> Result<Self> {
        let mut device = None;
        let mut context = None;
        // SAFETY: a live adapter; out pointers outlive the call.
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device = device.context("no device")?;
        let context: ID3D11DeviceContext = context.context("no context")?;
        // SAFETY: plain object creation and handle sharing on live devices.
        let (fence12, fence11) = unsafe {
            let fence12: ID3D12Fence = device12.CreateFence(0, D3D12_FENCE_FLAG_SHARED)?;
            let handle = device12.CreateSharedHandle(&fence12, None, 0x1000_0000, None)?;
            let mut fence11: Option<ID3D11Fence> = None;
            device
                .cast::<ID3D11Device5>()?
                .OpenSharedFence(handle, &mut fence11)?;
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            (fence12, fence11.context("no fence")?)
        };
        Ok(Self {
            winrt: winrt_device(&device)?,
            context: context.cast()?,
            device,
            device12: device12.clone(),
            fence11,
            fence12,
            value: Default::default(),
            crop11: Default::default(),
        })
    }
}

impl CaptureDevice for Separate {
    fn winrt(&self) -> &IDirect3DDevice {
        &self.winrt
    }

    fn crop(&self, device: &wgpu::Device) -> Result<wgpu::Texture> {
        let resource = committed(&self.device12, D3D12_HEAP_FLAG_SHARED)?;
        // SAFETY: shares a live resource and opens it on the D3D11 device.
        let crop11: ID3D11Texture2D = unsafe {
            let handle: HANDLE =
                self.device12
                    .CreateSharedHandle(&resource, None, 0x1000_0000, None)?;
            let crop = self
                .device
                .cast::<ID3D11Device1>()?
                .OpenSharedResource1(handle)?;
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            crop
        };
        let _ = self.crop11.set(crop11);
        Ok(import(device, resource))
    }

    fn update(&self, frame: &ID3D11Texture2D, x: u32, queue: &ID3D12CommandQueue) -> Result<()> {
        let crop = self.crop11.get().context("no crop")?;
        let value = self.value.get() + 1;
        self.value.set(value);
        // SAFETY: live resources on this device; the box lies inside the frame.
        unsafe {
            self.context
                .CopySubresourceRegion(crop, 0, 0, 0, 0, frame, 0, Some(&crop_box(x)));
            self.context.Signal(&self.fence11, value)?;
            self.context.Flush();
            // wgpu's queue runs nothing more until the copy is done.
            queue.Wait(&self.fence12, value)?;
        }
        Ok(())
    }
}

fn crop_box(x: u32) -> D3D11_BOX {
    D3D11_BOX {
        left: x,
        top: 0,
        front: 0,
        right: x + CROP,
        bottom: CROP,
        back: 1,
    }
}

/// A CROP x CROP BGRA texture in a default heap, in COMMON, that both APIs can use at once.
fn committed(device: &ID3D12Device, heap_flags: D3D12_HEAP_FLAGS) -> Result<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(CROP),
        Height: CROP,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS
            | D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: valid descriptors; the out pointer outlives the call.
    unsafe {
        device.CreateCommittedResource(
            &heap,
            heap_flags,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )?;
    }
    resource.context("no resource")
}

/// Hands a D3D12 texture to wgpu as an initialized, sampled texture.
fn import(device: &wgpu::Device, resource: ID3D12Resource) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width: CROP,
        height: CROP,
        depth_or_array_layers: 1,
    };
    // SAFETY: a live BGRA8 2D texture of `size`, one mip, one sample, in COMMON.
    unsafe {
        let raw = wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        );
        device.create_texture_from_hal::<Dx12>(
            raw,
            &wgpu::TextureDescriptor {
                label: Some("crop"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::TextureUses::UNINITIALIZED,
        )
    }
}

fn winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi: IDXGIDevice = device.cast()?;
    // SAFETY: a live DXGI device.
    Ok(unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?)
}

/// A full-screen pass sampling the crop into a small target: what an update costs wgpu.
struct Sampler {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    target: wgpu::TextureView,
}

impl Sampler {
    fn new(device: &wgpu::Device, crop: &wgpu::Texture) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var t: texture_2d<f32>;
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
                     let p = vec2f(f32(i & 1u), f32(i >> 1u)) * 4.0 - 1.0;
                     return vec4f(p, 0.0, 1.0);
                 }
                 @fragment fn fs(@builtin(position) p: vec4f) -> @location(0) vec4f {
                     return textureLoad(t, vec2i(p.xy), 0);
                 }"
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
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
                targets: &[Some(wgpu::TextureFormat::Bgra8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &crop.create_view(&Default::default()),
                ),
            }],
        });
        let target = device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: CROP,
                    height: CROP,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            pipeline,
            group,
            target,
        }
    }

    fn draw(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

/// wgpu as the engine opens it (gpu.rs): DX12 only, FXC, the low-power adapter.
fn open_wgpu() -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    Ok((adapter, device, queue))
}

fn settle() {
    std::thread::sleep(Duration::from_millis(400));
}

fn private_mb() -> f64 {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: our own process; the struct's size is passed with it.
    let _ = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    counters.PrivateUsage as f64 / (1024.0 * 1024.0)
}

/// CPU time (user + kernel) of the whole process so far, in ms.
fn cpu_ms() -> f64 {
    let (mut a, mut b, mut kernel, mut user) = Default::default();
    // SAFETY: our own process; out pointers outlive the call.
    let _ = unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user) };
    let ticks = |t: windows::Win32::Foundation::FILETIME| {
        (u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)) as f64
    };
    (ticks(kernel) + ticks(user)) / 10_000.0
}

/// `frames`: which events make the capture deliver a frame, and with which dirty rects, around
/// a small topmost layered window ("the popup") that is on the session's exclusion list.
mod frames {
    use std::sync::{Arc, Mutex};

    use windows::Graphics::RectInt32;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    use super::*;

    #[path = "../../src/overlay/abi.rs"]
    mod abi;

    const AT: RECT = RECT {
        left: 40,
        top: 1120,
        right: 440,
        bottom: 1380,
    };

    #[derive(Default)]
    struct Log {
        /// Frames, and those whose dirty rects touch the watched rect.
        frames: u32,
        touching: u32,
        stale: u32,
        watch: RECT,
        min_iteration: u64,
        last: String,
    }

    extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: forwards the window's own message.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn touches(r: &RectInt32, w: &RECT) -> bool {
        r.X < w.right && r.X + r.Width > w.left && r.Y < w.bottom && r.Y + r.Height > w.top
    }

    /// Paints the popup a flat colour with per-pixel alpha (alpha 255).
    fn paint(hwnd: HWND, at: POINT, shade: u8) {
        let (w, h) = (AT.right - AT.left, AT.bottom - AT.top);
        // SAFETY: a DIB section we own, selected into a memory DC for one UpdateLayeredWindow.
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bmp =
                CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
            px.fill(0xFF00_0000 | u32::from(shade) << 16 | 0x4080);
            let old = SelectObject(dc, bmp.into());
            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: 1,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&at),
                Some(&SIZE { cx: w, cy: h }),
                Some(dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(dc, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
        }
    }

    fn pump_for(ms: u64, tick: &mut dyn FnMut(u32)) {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut i = 0;
        while Instant::now() < end {
            // SAFETY: a plain message pump on the thread that owns the popup.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
            tick(i);
            i += 1;
            std::thread::sleep(Duration::from_millis(33));
        }
    }

    /// `border`: the control for the spike's "no yellow border" check: yellow pixels along the
    /// primary monitor's edges (a GDI screenshot) while a session captures it with the border
    /// required, then not required.
    pub fn border() -> Result<()> {
        let (adapter, device, _queue) = open_wgpu()?;
        // SAFETY: wgpu's live adapter and device, used while they live.
        let (dxgi_adapter, device12) = unsafe {
            (
                adapter
                    .as_hal::<Dx12>()
                    .context("not DX12")?
                    .as_raw()
                    .cast::<IDXGIAdapter>()?,
                device
                    .as_hal::<Dx12>()
                    .context("not DX12")?
                    .raw_device()
                    .clone(),
            )
        };
        let capture = Separate::new(&dxgi_adapter, &device12)?;
        // SAFETY: a plain monitor lookup.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let yellow = || -> usize {
            // SAFETY: GDI reads of the screen into a DIB we own.
            unsafe {
                let (w, h) = (2560, 1440);
                let screen = GetDC(None);
                let dc = CreateCompatibleDC(Some(screen));
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                let Ok(bmp) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                else {
                    return 0;
                };
                let old = SelectObject(dc, bmp.into());
                let _ = BitBlt(dc, 0, 0, w, h, Some(screen), 0, 0, SRCCOPY | CAPTUREBLT);
                let px = std::slice::from_raw_parts(bits.cast::<u8>(), (w * h * 4) as usize);
                let edge = |x: i32, y: i32| x < 4 || y < 4 || x >= w - 4 || y >= h - 4;
                let mut count = 0;
                for y in 0..h {
                    for x in 0..w {
                        let p = &px[((y * w + x) * 4) as usize..][..4];
                        if edge(x, y) && p[2] > 200 && p[1] > 140 && p[0] < 90 {
                            count += 1;
                        }
                    }
                }
                SelectObject(dc, old);
                let _ = DeleteObject(bmp.into());
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                count
            }
        };
        println!("no session: {} yellow edge pixels", yellow());
        for required in [true, false] {
            // SAFETY: a live monitor handle.
            let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                capture.winrt(),
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                1,
                item.Size()?,
            )?;
            let session = pool.CreateCaptureSession(&item)?;
            session.SetIsBorderRequired(required)?;
            session.StartCapture()?;
            std::thread::sleep(Duration::from_millis(1200));
            println!(
                "session with the border required = {required}: {} yellow edge pixels",
                yellow()
            );
            session.Close()?;
            pool.Close()?;
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(())
    }

    pub fn run() -> Result<()> {
        let (adapter, device, _queue) = open_wgpu()?;
        // SAFETY: wgpu's live adapter and device, used while they live.
        let (dxgi_adapter, device12) = unsafe {
            (
                adapter
                    .as_hal::<Dx12>()
                    .context("not DX12")?
                    .as_raw()
                    .cast::<IDXGIAdapter>()?,
                device
                    .as_hal::<Dx12>()
                    .context("not DX12")?
                    .raw_device()
                    .clone(),
            )
        };
        let capture = Separate::new(&dxgi_adapter, &device12)?;

        // SAFETY: plain window creation; the window lives until the process exits.
        let popup = unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("overlay-spike-frames"),
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("overlay-spike-frames"),
                w!("popup"),
                WS_POPUP | WS_VISIBLE,
                AT.left,
                AT.top,
                AT.right - AT.left,
                AT.bottom - AT.top,
                None,
                None,
                None,
                None,
            )?
        };
        paint(
            popup,
            POINT {
                x: AT.left,
                y: AT.top,
            },
            0,
        );

        // SAFETY: a plain monitor lookup.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: a live monitor handle.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            capture.winrt(),
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsCursorCaptureEnabled(false)?;
        println!("border off: {:?}", session.SetIsBorderRequired(false));
        session.SetDirtyRegionMode(GraphicsCaptureDirtyRegionMode::ReportOnly)?;
        let exclusion = abi::Exclusion::of(&session);
        println!("exclusion list supported: {}", exclusion.is_some());
        let log = Arc::new(Mutex::new(Log {
            watch: RECT {
                left: AT.left - 8,
                top: AT.top - 8,
                right: AT.right + 208,
                bottom: AT.bottom + 8,
            },
            ..Default::default()
        }));
        let sink = log.clone();
        pool.FrameArrived(&windows::Foundation::TypedEventHandler::new(
            move |pool: windows_core::Ref<Direct3D11CaptureFramePool>, _| {
                let Some(pool) = pool.as_ref() else {
                    return Ok(());
                };
                let frame = pool.TryGetNextFrame()?;
                let mut log = sink.lock().unwrap();
                log.frames += 1;
                let iteration = abi::iteration(&frame);
                if iteration < log.min_iteration {
                    log.stale += 1;
                }
                let dirty: Vec<RectInt32> = frame.DirtyRegions()?.into_iter().collect();
                let watch = log.watch;
                if dirty.iter().any(|r| touches(r, &watch)) {
                    log.touching += 1;
                    log.last = format!(
                        "it={iteration} {} rects, first {:?}",
                        dirty.len(),
                        dirty.iter().find(|r| touches(r, &watch))
                    );
                }
                Ok(())
            },
        ))?;
        session.StartCapture()?;

        let phase = |name: &str, ms: u64, tick: &mut dyn FnMut(u32)| {
            {
                let mut l = log.lock().unwrap();
                l.frames = 0;
                l.touching = 0;
                l.stale = 0;
                l.last.clear();
            }
            pump_for(ms, tick);
            let l = log.lock().unwrap();
            println!(
                "{name:<44} frames {:>3}  touching popup {:>3}  stale {:>2}  {}",
                l.frames, l.touching, l.stale, l.last
            );
        };
        let set = |list: &[HWND]| {
            if let Some(ex) = &exclusion {
                let it = ex.set(list).unwrap();
                log.lock().unwrap().min_iteration = it;
                println!("  set exclusion {list:?} -> iteration {it}");
            }
        };

        phase("start, popup visible, not excluded", 1500, &mut |_| {});
        phase("idle", 1500, &mut |_| {});
        set(&[popup]);
        phase("after excluding the popup", 1500, &mut |_| {});
        phase("idle", 1500, &mut |_| {});
        phase("excluded popup repaints at 30 Hz", 1500, &mut |i| {
            paint(
                popup,
                POINT {
                    x: AT.left,
                    y: AT.top,
                },
                (i * 20) as u8,
            )
        });
        phase("excluded popup moves 4 px per tick", 1500, &mut |i| {
            let x = AT.left + (i as i32 % 50) * 4;
            // SAFETY: moving our own window.
            let _ = unsafe {
                SetWindowPos(
                    popup,
                    None,
                    x,
                    AT.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        });
        phase("idle", 1500, &mut |_| {});
        set(&[popup]);
        phase("after setting the same list again", 1500, &mut |_| {});
        // SAFETY: hiding and showing our own window.
        let _ = unsafe { ShowWindow(popup, SW_HIDE) };
        phase("excluded popup hidden", 1500, &mut |_| {});
        // SAFETY: as above.
        let _ = unsafe { ShowWindow(popup, SW_SHOWNA) };
        phase("excluded popup shown", 1500, &mut |_| {});
        pool.Recreate(
            capture.winrt(),
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        phase("after pool.Recreate", 1500, &mut |_| {});
        set(&[]);
        phase("after clearing the list", 1500, &mut |_| {});
        phase(
            "visible popup repaints at 30 Hz (control)",
            1500,
            &mut |i| {
                paint(
                    popup,
                    POINT {
                        x: AT.left,
                        y: AT.top,
                    },
                    (i * 20) as u8,
                )
            },
        );
        session.Close()?;
        pool.Close()?;
        Ok(())
    }
}

/// `swapchain-<kind>`: what a small composition target costs on the low-power adapter when the
/// monitors hang off another one. Kinds: `wgpu` (a wgpu surface on a DirectComposition visual,
/// as overlays use), `wgpu-hp` (the same on the high-performance adapter), `d3d11` (a D3D11
/// composition swapchain), `surface` (a DirectComposition surface drawn by D3D11).
mod swapchains {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::DirectComposition::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    use super::*;

    const SIZES: [(u32, u32); 3] = [(272, 242), (272, 242), (600, 400)];

    extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: forwards the window's own message.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn window(i: usize, (w, h): (u32, u32)) -> Result<HWND> {
        // SAFETY: plain window creation; windows live until the process exits.
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("overlay-spike-swapchain"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW,
                w!("overlay-spike-swapchain"),
                w!("swapchain"),
                WS_POPUP | WS_VISIBLE,
                60 + i as i32 * 20,
                1130,
                w as i32,
                h as i32,
                None,
                None,
                None,
                None,
            )?;
            SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA)?;
            Ok(hwnd)
        }
    }

    fn step(label: &str, last: &mut f64) {
        settle();
        let now = private_mb();
        println!("{label}: private {now:.1} MB (+{:.1})", now - *last);
        *last = now;
    }

    pub fn run(kind: &str) -> Result<()> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
        let instance = wgpu::Instance::new(desc);
        let power = if kind == "wgpu-hp" {
            wgpu::PowerPreference::HighPerformance
        } else {
            wgpu::PowerPreference::LowPower
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: power,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        println!("{kind} on {}", adapter.get_info().name);
        let mut last = private_mb();
        step("wgpu device", &mut last);

        // A D3D11 device on the same adapter, for the D3D11 kinds.
        let d3d11 = if kind == "d3d11" || kind == "surface" {
            // SAFETY: wgpu's live adapter.
            let dxgi = unsafe {
                adapter
                    .as_hal::<Dx12>()
                    .context("dx12")?
                    .as_raw()
                    .cast::<IDXGIAdapter>()?
            };
            let mut dev = None;
            let mut ctx = None;
            // SAFETY: plain device creation.
            unsafe {
                D3D11CreateDevice(
                    &dxgi,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    Some(&mut ctx),
                )?;
            }
            step("d3d11 device", &mut last);
            let dev: ID3D11Device = dev.context("dev")?;
            let ctx: ID3D11DeviceContext = ctx.context("ctx")?;
            Some((dev, ctx, dxgi))
        } else {
            None
        };
        // SAFETY: DirectComposition device creation (with the D3D11 device for surfaces).
        let dcomp: IDCompositionDesktopDevice = unsafe {
            match (&d3d11, kind) {
                (Some((dev, _, _)), "surface") => {
                    DCompositionCreateDevice3(&dev.cast::<IDXGIDevice>()?)?
                }
                _ => DCompositionCreateDevice3(None::<&IUnknown>)?,
            }
        };
        step("dcomp device", &mut last);

        let mut keep: Vec<Box<dyn std::any::Any>> = Vec::new();
        for (i, size) in SIZES.into_iter().enumerate() {
            let hwnd = window(i, size)?;
            // SAFETY: DirectComposition calls on live objects.
            let (target, visual) = unsafe {
                let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
                let visual = dcomp.CreateVisual()?;
                target.SetRoot(&visual)?;
                (target, visual)
            };
            step(&format!("  window {i} {size:?} + target"), &mut last);
            match kind {
                "wgpu" | "wgpu-hp" => {
                    // SAFETY: the visual outlives the surface (both kept to the end).
                    let surface = unsafe {
                        instance.create_surface_unsafe(
                            wgpu::SurfaceTargetUnsafe::CompositionVisual(visual.as_raw()),
                        )?
                    };
                    let config = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: wgpu::TextureFormat::Bgra8Unorm,
                        width: size.0,
                        height: size.1,
                        present_mode: wgpu::PresentMode::Fifo,
                        desired_maximum_frame_latency: 1,
                        alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                        color_space: wgpu::SurfaceColorSpace::Auto,
                        view_formats: vec![],
                    };
                    surface.configure(&device, &config);
                    step("    configured", &mut last);
                    if let wgpu::CurrentSurfaceTexture::Success(frame) =
                        surface.get_current_texture()
                    {
                        let view = frame.texture.create_view(&Default::default());
                        let mut encoder = device.create_command_encoder(&Default::default());
                        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        }));
                        queue.submit([encoder.finish()]);
                        queue.present(frame);
                    }
                    // SAFETY: a commit on a live device.
                    unsafe { dcomp.Commit()? };
                    step("    presented", &mut last);
                    keep.push(Box::new(surface));
                }
                "d3d11" => {
                    let (dev, ctx, dxgi) = d3d11.as_ref().context("d3d11")?;
                    // SAFETY: a composition swapchain on the D3D11 device.
                    unsafe {
                        let factory: IDXGIFactory2 = dxgi.GetParent()?;
                        let desc = DXGI_SWAP_CHAIN_DESC1 {
                            Width: size.0,
                            Height: size.1,
                            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                            BufferCount: 2,
                            Scaling: DXGI_SCALING_STRETCH,
                            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                            ..Default::default()
                        };
                        let chain = factory.CreateSwapChainForComposition(dev, &desc, None)?;
                        visual.SetContent(&chain)?;
                        step("    configured", &mut last);
                        let back: ID3D11Texture2D = chain.GetBuffer(0)?;
                        let mut rtv = None;
                        dev.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
                        ctx.ClearRenderTargetView(rtv.as_ref().context("rtv")?, &[0.0; 4]);
                        chain.Present(1, DXGI_PRESENT(0)).ok()?;
                        dcomp.Commit()?;
                        step("    presented", &mut last);
                        keep.push(Box::new(chain));
                    }
                }
                "surface" => {
                    let (dev, ctx, _) = d3d11.as_ref().context("d3d11")?;
                    // SAFETY: a DirectComposition surface drawn once by D3D11.
                    unsafe {
                        let surface = dcomp.CreateSurface(
                            size.0,
                            size.1,
                            DXGI_FORMAT_B8G8R8A8_UNORM,
                            DXGI_ALPHA_MODE_PREMULTIPLIED,
                        )?;
                        visual.SetContent(&surface)?;
                        step("    configured", &mut last);
                        let mut offset = POINT::default();
                        let texture: ID3D11Texture2D = surface.BeginDraw(None, &mut offset)?;
                        let mut rtv = None;
                        dev.CreateRenderTargetView(&texture, None, Some(&mut rtv))?;
                        ctx.ClearRenderTargetView(rtv.as_ref().context("rtv")?, &[0.0; 4]);
                        surface.EndDraw()?;
                        dcomp.Commit()?;
                        step("    presented", &mut last);
                        keep.push(Box::new(surface));
                    }
                }
                other => bail!("unknown kind {other}"),
            }
            keep.push(Box::new((target, visual, hwnd.0 as usize)));
        }
        Ok(())
    }
}
