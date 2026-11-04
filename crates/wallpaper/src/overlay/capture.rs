//! The backdrop behind the overlays: one Windows.Graphics.Capture session per monitor that has an
//! open overlay, on a D3D11 device that hands crops to wgpu and shows the glass it draws.
//!
//! **The device.** WGC's frame pool needs a D3D11 device; the engine is DX12. A separate D3D11
//! device on wgpu's adapter costs +16 MB private here, D3D11On12 on wgpu's own device +92 MB
//! (`examples/overlay_spike_device.rs`, docs/engine-notes.md "Glass overlays"), so it is a
//! separate device. Crops (and the drawn glass) are D3D12 textures in a shared heap with
//! simultaneous access (no state hand-off between the APIs), opened in D3D11; one shared fence
//! orders D3D11's copies before wgpu reads them, and wgpu's glass before D3D11 copies it into
//! the window's DirectComposition surface.
//!
//! **The session.** One frame-pool buffer; `DirtyRegionMode::ReportOnly`; no cursor; no border.
//! The frame handler runs on WGC's thread and drops, there and then, every frame whose
//! configuration iteration predates the exclusion list, or whose dirty rects miss every open
//! overlay's region: such frames never wake the engine's thread. A frame that hits is held until
//! that thread takes it (holding the only buffer pauses delivery, which is what we want).

use std::sync::{Arc, Mutex, Once};

use anyhow::{Context, Result};
use mypaper_desktop::Rect;
use wgpu::hal::api::Dx12;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess,
    GraphicsCaptureAccessKind, GraphicsCaptureDirtyRegionMode, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_QUERY_DATA_TIMESTAMP_DISJOINT,
    D3D11_QUERY_DESC, D3D11_QUERY_TIMESTAMP, D3D11_QUERY_TIMESTAMP_DISJOINT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device, ID3D11Device1, ID3D11Device5, ID3D11DeviceContext,
    ID3D11DeviceContext4, ID3D11Fence, ID3D11Query, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
    D3D12_RESOURCE_STATE_COMMON, ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::DirectComposition::IDCompositionSurface;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows_core::Interface;

use super::abi::{self, Exclusion};
use super::geometry;

/// `GENERIC_ALL` for shared handles.
const SHARE_ALL: u32 = 0x1000_0000;
/// The shortest time between two captured frames: 1/60 s, in 100 ns units.
const MIN_INTERVAL: windows::Foundation::TimeSpan = windows::Foundation::TimeSpan {
    Duration: 10_000_000 / 60,
};

/// The D3D11 device frames are captured with, and its fence shared with wgpu's queue.
pub struct CaptureDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext4,
    winrt: IDirect3DDevice,
    device12: ID3D12Device,
    queue12: ID3D12CommandQueue,
    fence11: ID3D11Fence,
    fence12: ID3D12Fence,
    value: u64,
}

/// A texture both devices use: a crop of the backdrop (written by D3D11, sampled by wgpu), or the
/// glass (drawn by wgpu, copied by D3D11 into the window's DirectComposition surface).
pub struct Crop {
    pub size: (u32, u32),
    d3d11: ID3D11Texture2D,
    pub texture: wgpu::Texture,
}

impl CaptureDevice {
    /// A D3D11 device on wgpu's adapter, fenced with wgpu's queue.
    pub fn new(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Result<Self> {
        // SAFETY: the raw DX12 objects behind wgpu's adapter and device, kept alive here.
        let (dxgi_adapter, device12, queue12) = unsafe {
            let adapter = adapter
                .as_hal::<Dx12>()
                .context("the engine is not on DX12")?;
            let hal = device
                .as_hal::<Dx12>()
                .context("the engine is not on DX12")?;
            (
                adapter.as_raw().cast::<IDXGIAdapter>()?,
                hal.raw_device().clone(),
                hal.raw_queue().clone(),
            )
        };
        let mut d3d11 = None;
        let mut context = None;
        // SAFETY: a live adapter; the out pointers outlive the call.
        unsafe {
            D3D11CreateDevice(
                &dxgi_adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d11),
                None,
                Some(&mut context),
            )
            .context("D3D11CreateDevice")?;
        }
        let d3d11: ID3D11Device = d3d11.context("no D3D11 device")?;
        let context: ID3D11DeviceContext = context.context("no D3D11 context")?;
        // SAFETY: creates a fence and shares it between two live devices; the handle is closed
        // once both hold the fence.
        let (fence12, fence11) = unsafe {
            let fence12: ID3D12Fence = device12.CreateFence(0, D3D12_FENCE_FLAG_SHARED)?;
            let handle = device12.CreateSharedHandle(&fence12, None, SHARE_ALL, None)?;
            let mut fence11: Option<ID3D11Fence> = None;
            let opened = d3d11
                .cast::<ID3D11Device5>()?
                .OpenSharedFence(handle, &mut fence11);
            let _ = CloseHandle(handle);
            opened?;
            (fence12, fence11.context("no shared fence")?)
        };
        let dxgi: IDXGIDevice = d3d11.cast()?;
        // SAFETY: a live DXGI device.
        let winrt: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        Ok(Self {
            device: d3d11,
            context: context.cast()?,
            winrt,
            device12,
            queue12,
            fence11,
            fence12,
            value: 0,
        })
    }

    /// A shared BGRA texture of `size`, used by `device` as `usage` (and by this device for copies).
    pub fn crop(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        usage: wgpu::TextureUsages,
    ) -> Result<Crop> {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: u64::from(size.0),
            Height: size.1,
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
        // SAFETY: valid descriptors; the shared handle is closed once D3D11 holds the texture.
        let d3d11: ID3D11Texture2D = unsafe {
            self.device12.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_SHARED,
                &desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )?;
            let resource = resource.as_ref().context("no crop texture")?;
            let handle = self
                .device12
                .CreateSharedHandle(resource, None, SHARE_ALL, None)?;
            let opened = self
                .device
                .cast::<ID3D11Device1>()?
                .OpenSharedResource1(handle);
            let _ = CloseHandle(handle);
            opened?
        };
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        // SAFETY: a live BGRA8 2D texture of `extent`, one mip and sample, in COMMON (and, with
        // simultaneous access, never needing a transition for another device).
        let texture = unsafe {
            let raw = wgpu::hal::dx12::Device::texture_from_raw(
                resource.context("no crop texture")?,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureDimension::D2,
                extent,
                1,
                1,
            );
            device.create_texture_from_hal::<Dx12>(
                raw,
                &wgpu::TextureDescriptor {
                    label: Some("glass overlay crop"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            )
        };
        Ok(Crop {
            size,
            d3d11,
            texture,
        })
    }

    /// Queues a copy of `frame`'s monitor-local `from` (clamped to the frame) into `to` at 0, 0.
    pub fn copy(&self, frame: &ID3D11Texture2D, from: Rect, to: &Crop) {
        let source = D3D11_BOX {
            left: from.x.max(0) as u32,
            top: from.y.max(0) as u32,
            front: 0,
            right: (from.x + from.width) as u32,
            bottom: (from.y + from.height) as u32,
            back: 1,
        };
        // SAFETY: live textures on this device; the box lies inside the frame (frames are the
        // monitor's size and regions are kept on their monitor).
        unsafe {
            self.context
                .CopySubresourceRegion(&to.d3d11, 0, 0, 0, 0, frame, 0, Some(&source));
        }
    }

    /// Submits the queued copies, and makes wgpu's queue wait for them on the GPU.
    pub fn hand_to_wgpu(&mut self) -> Result<()> {
        self.value += 1;
        // SAFETY: live context, fences and queue.
        unsafe {
            self.context.Signal(&self.fence11, self.value)?;
            self.context.Flush();
            self.queue12.Wait(&self.fence12, self.value)?;
        }
        Ok(())
    }

    /// Makes this device wait, on the GPU, for everything wgpu has submitted so far.
    pub fn wait_for_wgpu(&mut self) -> Result<()> {
        self.value += 1;
        // SAFETY: live queue, fences and context.
        unsafe {
            self.queue12.Signal(&self.fence12, self.value)?;
            self.context.Wait(&self.fence11, self.value)?;
        }
        Ok(())
    }

    /// Copies `glass` into the whole of `surface` (after [`wait_for_wgpu`](Self::wait_for_wgpu)).
    /// The surface shows it at the next DirectComposition commit.
    pub fn present(&self, glass: &Crop, surface: &IDCompositionSurface) -> Result<()> {
        let mut offset = windows::Win32::Foundation::POINT::default();
        // SAFETY: a surface of this device's DirectComposition device, drawn whole and ended.
        unsafe {
            let target: ID3D11Texture2D = surface.BeginDraw(None, &mut offset)?;
            self.context.CopySubresourceRegion(
                &target,
                0,
                offset.x as u32,
                offset.y as u32,
                0,
                &glass.d3d11,
                0,
                None,
            );
            surface.EndDraw()?;
        }
        Ok(())
    }

    /// The DXGI device a DirectComposition device draws surfaces with.
    pub fn dxgi(&self) -> Result<IDXGIDevice> {
        Ok(self.device.cast()?)
    }

    /// Timestamp queries around the copies, for measuring (see [`CopyTimer`]).
    pub fn copy_timer(&self) -> Result<CopyTimer> {
        let desc = |query| D3D11_QUERY_DESC {
            Query: query,
            MiscFlags: 0,
        };
        let mut queries: [Option<ID3D11Query>; 3] = Default::default();
        for (slot, kind) in queries.iter_mut().zip([
            D3D11_QUERY_TIMESTAMP_DISJOINT,
            D3D11_QUERY_TIMESTAMP,
            D3D11_QUERY_TIMESTAMP,
        ]) {
            // SAFETY: plain query creation on a live device.
            unsafe { self.device.CreateQuery(&desc(kind), Some(slot))? };
        }
        let [disjoint, start, end] = queries.map(Option::unwrap);
        Ok(CopyTimer {
            context: self.context.clone(),
            disjoint,
            start,
            end,
        })
    }
}

/// The GPU time of one batch of copies on the capture device: `begin` before them, `end` after
/// [`CaptureDevice::hand_to_wgpu`], `read` once wgpu has waited for them.
pub struct CopyTimer {
    context: ID3D11DeviceContext4,
    disjoint: ID3D11Query,
    start: ID3D11Query,
    end: ID3D11Query,
}

impl CopyTimer {
    pub fn begin(&self) {
        // SAFETY: queries of this context.
        unsafe {
            self.context.Begin(&self.disjoint);
            self.context.End(&self.start);
        }
    }

    pub fn end(&self) {
        // SAFETY: as above.
        unsafe {
            self.context.End(&self.end);
            self.context.End(&self.disjoint);
            self.context.Flush();
        }
    }

    /// The copies' GPU ms, if the GPU has reported them.
    pub fn read(&self) -> Option<f64> {
        // The raw call: the wrapper maps S_FALSE ("not ready yet") to Ok too.
        let context: &ID3D11DeviceContext = &self.context;
        let get = |query: &ID3D11Query, out: *mut core::ffi::c_void, size: usize| {
            for _ in 0..1000 {
                // SAFETY: live context and query; `out` points at `size` writable bytes.
                let hr = unsafe {
                    (Interface::vtable(context).GetData)(
                        Interface::as_raw(context),
                        Interface::as_raw(query),
                        out,
                        size as u32,
                        0,
                    )
                };
                if hr == windows::Win32::Foundation::S_OK {
                    return true;
                }
                std::thread::yield_now();
            }
            false
        };
        let mut disjoint = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
        let (mut start, mut end) = (0u64, 0u64);
        let ok = get(
            &self.disjoint,
            (&raw mut disjoint).cast(),
            size_of::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>(),
        ) && get(&self.start, (&raw mut start).cast(), 8)
            && get(&self.end, (&raw mut end).cast(), 8);
        (ok && !disjoint.Disjoint.as_bool() && disjoint.Frequency > 0)
            .then(|| end.saturating_sub(start) as f64 * 1000.0 / disjoint.Frequency as f64)
    }
}

/// What the frame handler and the engine's thread share about one session.
#[derive(Default)]
struct Watch {
    /// The open overlays' frames, monitor-local: a frame matters if its dirty rects touch one.
    regions: Vec<Rect>,
    /// Frames rendered with an older exclusion list (they may show our own windows) are dropped.
    min_iteration: u64,
    /// The frame waiting for the engine's thread.
    pending: Option<Direct3D11CaptureFrame>,
    /// Set when the next frame must be taken whatever its dirty rects say (a region appeared or
    /// moved, so its pixels are needed even where nothing changed).
    want_any: bool,
    counts: Counts,
}

/// Frames a session saw, by fate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub delivered: u64,
    /// Rendered before the current exclusion list took effect.
    pub stale: u64,
    /// Dirty rects missed every region: no work at all.
    pub missed: u64,
    /// Handed to the engine's thread.
    pub taken: u64,
}

impl Counts {
    pub fn plus(self, other: Self) -> Self {
        Self {
            delivered: self.delivered + other.delivered,
            stale: self.stale + other.stale,
            missed: self.missed + other.missed,
            taken: self.taken + other.taken,
        }
    }

    pub fn minus(self, other: Self) -> Self {
        Self {
            delivered: self.delivered - other.delivered,
            stale: self.stale - other.stale,
            missed: self.missed - other.missed,
            taken: self.taken - other.taken,
        }
    }
}

/// One monitor's capture session.
pub struct Session {
    pub monitor: HMONITOR,
    /// The monitor in virtual-screen px.
    pub bounds: Rect,
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    exclusion: Option<Exclusion>,
    watch: Arc<Mutex<Watch>>,
    token: i64,
}

impl Session {
    /// Starts capturing `monitor`; each frame that matters posts `message` to `notify`.
    pub fn start(
        device: &CaptureDevice,
        monitor: HMONITOR,
        bounds: Rect,
        notify: HWND,
        message: u32,
    ) -> Result<Self> {
        borderless_answered();
        let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: a live monitor handle.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device.winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsCursorCaptureEnabled(false)?;
        session.SetDirtyRegionMode(GraphicsCaptureDirtyRegionMode::ReportOnly)?;
        // At most 60 updates a second, however fast what is behind changes (the monitors here run
        // at 239 Hz).
        session.SetMinUpdateInterval(MIN_INTERVAL)?;
        if let Err(e) = session.SetIsBorderRequired(false) {
            log::error!("glass overlays: capture border can't be turned off: {e}");
        }
        let exclusion = Exclusion::of(&session);
        let watch = Arc::new(Mutex::new(Watch {
            want_any: true,
            ..Default::default()
        }));
        let shared = watch.clone();
        let notify = notify.0 as usize;
        let token = pool.FrameArrived(&TypedEventHandler::new(
            move |pool: windows_core::Ref<Direct3D11CaptureFramePool>, _| {
                let Some(pool) = pool.as_ref() else {
                    return Ok(());
                };
                let frame = pool.TryGetNextFrame()?;
                if keep(&shared, &frame) {
                    // SAFETY: posting to the engine's window, which outlives the session.
                    let _ = unsafe {
                        PostMessageW(Some(HWND(notify as *mut _)), message, WPARAM(0), LPARAM(0))
                    };
                }
                Ok(())
            },
        ))?;
        session.StartCapture()?;
        Ok(Self {
            monitor,
            bounds,
            item,
            pool,
            session,
            exclusion,
            watch,
            token,
        })
    }

    /// Whether this Windows has the per-session exclusion list.
    pub fn excludes(&self) -> bool {
        self.exclusion.is_some()
    }

    /// Keeps `windows` out of the backdrop; frames rendered before that are dropped.
    pub fn exclude(&self, windows: &[HWND]) -> Result<()> {
        let Some(exclusion) = &self.exclusion else {
            return Ok(());
        };
        let iteration = exclusion.set(windows)?;
        self.lock().min_iteration = iteration;
        Ok(())
    }

    /// The overlays' frames on this monitor, in virtual px; with `fresh`, the next frame is
    /// taken whatever its dirty rects (a region appeared or moved and needs its pixels).
    pub fn set_regions(&self, regions: impl IntoIterator<Item = Rect>, fresh: bool) {
        let mut watch = self.lock();
        watch.regions = regions
            .into_iter()
            .map(|r| geometry::local(r, self.bounds))
            .collect();
        watch.want_any |= fresh;
    }

    /// The frame waiting for this thread, if one is.
    pub fn take(&self) -> Option<Direct3D11CaptureFrame> {
        self.lock().pending.take()
    }

    pub fn counts(&self) -> Counts {
        self.lock().counts
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Watch> {
        self.watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
        let _ = &self.item;
    }
}

/// The frame handler's decision, on WGC's thread: keeps `frame` for the engine if it is current
/// and touches a region (or any frame is wanted). Returns whether the engine must be woken.
fn keep(watch: &Mutex<Watch>, frame: &Direct3D11CaptureFrame) -> bool {
    let mut watch = watch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    watch.counts.delivered += 1;
    if abi::iteration(frame) < watch.min_iteration {
        watch.counts.stale += 1;
        return false;
    }
    let hit = watch.want_any
        || frame.DirtyRegions().is_ok_and(|dirty| {
            dirty.into_iter().any(|r| {
                let r = Rect {
                    x: r.X,
                    y: r.Y,
                    width: r.Width,
                    height: r.Height,
                };
                watch.regions.iter().any(|g| geometry::touches(r, *g))
            })
        });
    if !hit {
        watch.counts.missed += 1;
        return false;
    }
    watch.want_any = false;
    watch.counts.taken += 1;
    let woke = watch.pending.is_none();
    watch.pending = Some(frame.clone());
    woke
}

/// The captured frame's texture and its dirty rects (monitor-local).
pub fn open_frame(frame: &Direct3D11CaptureFrame) -> Result<(ID3D11Texture2D, Vec<Rect>)> {
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    // SAFETY: the frame's own texture, used while the frame is held.
    let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let dirty = frame
        .DirtyRegions()?
        .into_iter()
        .map(|r| Rect {
            x: r.X,
            y: r.Y,
            width: r.Width,
            height: r.Height,
        })
        .collect();
    Ok((texture, dirty))
}

/// The borderless-capture request, while it is being answered.
static ASKING: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Starts asking, once per process and without waiting, to capture without the yellow border
/// (Settings › Privacy & security › Screenshot borders governs it for unpackaged apps). Called
/// when the overlays are created, so the answer is in before the first one opens. Logged loudly
/// if not allowed: the border then shows while an overlay is open.
pub fn ask_borderless() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // On its own MTA thread: the request may wait on a consent prompt.
        let asking = std::thread::spawn(|| {
            let status = (|| -> windows_core::Result<AppCapabilityAccessStatus> {
                // SAFETY: per-thread COM init for this short-lived thread.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)?
                    .join()
            })();
            log_borderless(status);
        });
        *ASKING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(asking);
    });
}

/// Waits for the borderless answer, if it is still coming (a session started before it may
/// show the border).
fn borderless_answered() {
    ask_borderless();
    let asking = ASKING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(asking) = asking {
        let _ = asking.join();
    }
}

fn log_borderless(status: windows_core::Result<AppCapabilityAccessStatus>) {
    match status {
        Ok(AppCapabilityAccessStatus::Allowed) => {
            log::info!("glass overlays: borderless capture allowed");
        }
        other => log::error!(
            "glass overlays: borderless capture NOT allowed ({other:?}); the capture border \
             will show while an overlay is open"
        ),
    }
}
