//! Zero-copy frames: the decoder's D3D11 device and wgpu's D3D12 device,
//! on the same GPU, share a small ring of NV12 textures ("slots").
//!
//! Per frame the decoder copies its output into a free slot and signals the
//! `written` fence. The engine has wgpu's next submission wait for that
//! value before sampling the slot. When a slot leaves the screen, wgpu
//! signals `released` after its next submission, and the decoder's GPU
//! queue waits for that before writing the slot again. Neither CPU thread
//! ever waits for the other's GPU work.
//!
//! wgpu sees each slot as two single-plane textures (R8 luma, RG8 chroma).
//! They are only ever sampled and are declared as already in that state,
//! so wgpu emits no barriers: D3D12 promotes them from COMMON for each
//! submission and decays them back after, the state the D3D11 writer needs.

use anyhow::{Context, Result};
use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, LUID};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, ID3D12Device, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIResource1,
};
use windows::core::{Interface, PCWSTR};

use crate::gpu::Gpu;

/// Slots in the ring: one per frame that can be in flight (two queued, one
/// due later, one on screen, one being written), so the decoder never waits.
pub const SLOTS: usize = 5;

/// An owned NT handle that may cross threads; closed on drop.
pub struct Handle(HANDLE);

// SAFETY: NT handles are process-wide; any thread may use or close one.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this handle is ours and closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// What the decoder hands the engine once its slots exist.
pub struct Slots {
    pub textures: Vec<Handle>,
    pub written: Handle,
    pub size: (u32, u32),
}

/// wgpu's adapter, plus a new shared `released` fence and a handle to it
/// for the decoder.
pub fn target(gpu: &Gpu) -> Result<(LUID, ID3D12Fence, Handle)> {
    // SAFETY: only uses the raw device to create new objects.
    let hal = unsafe { gpu.device.as_hal::<Dx12>() }.context("wgpu is not on D3D12")?;
    let device = hal.raw_device();
    // SAFETY: D3D12 calls on a live device with valid arguments.
    unsafe {
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_SHARED)?;
        let handle = device.CreateSharedHandle(&fence, None, GENERIC_ALL.0, PCWSTR::null())?;
        Ok((device.GetAdapterLuid(), fence, Handle(handle)))
    }
}

/// The decoder's half: copies decoded frames into the slots.
pub struct Writer {
    context: ID3D11DeviceContext4,
    textures: Vec<ID3D11Texture2D>,
    written: ID3D11Fence,
    value: u64,
    released: ID3D11Fence,
    /// The part of the decoder's (padded) output that is shown.
    region: D3D11_BOX,
    pub size: (u32, u32),
}

impl Writer {
    /// Creates slots for the `width × height` frame at `(x, y)` of the
    /// decoder's output (NV12 needs even coordinates, so odd ones round).
    pub fn new(
        device: &ID3D11Device,
        released: Handle,
        (x, y): (u32, u32),
        (width, height): (u32, u32),
    ) -> Result<(Self, Slots)> {
        let (x, y, width, height) = (
            x & !1,
            y & !1,
            width.next_multiple_of(2),
            height.next_multiple_of(2),
        );
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                as u32,
        };
        let access = (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0;
        // SAFETY: D3D11 calls on a live device; out-pointers are valid locals.
        unsafe {
            let device5: ID3D11Device5 = device.cast()?;
            let (mut opened, mut created) = (None, None);
            device5.OpenSharedFence(released.0, &mut opened)?;
            device5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut created)?;
            let released: ID3D11Fence = opened.context("no released fence")?;
            let written: ID3D11Fence = created.context("no written fence")?;
            let written_handle = written.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())?;
            let mut textures = Vec::with_capacity(SLOTS);
            let mut handles = Vec::with_capacity(SLOTS);
            for _ in 0..SLOTS {
                let mut texture = None;
                device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                let texture: ID3D11Texture2D = texture.context("no slot texture")?;
                let resource: IDXGIResource1 = texture.cast()?;
                handles.push(Handle(resource.CreateSharedHandle(
                    None,
                    access,
                    PCWSTR::null(),
                )?));
                textures.push(texture);
            }
            let writer = Self {
                context: device.GetImmediateContext()?.cast()?,
                textures,
                written,
                value: 0,
                released,
                region: D3D11_BOX {
                    left: x,
                    top: y,
                    front: 0,
                    right: x + width,
                    bottom: y + height,
                    back: 1,
                },
                size: (width, height),
            };
            let slots = Slots {
                textures: handles,
                written: Handle(written_handle),
                size: (width, height),
            };
            Ok((writer, slots))
        }
    }

    /// Whether frames of this area (as in [`Writer::new`]) fit these slots.
    pub fn fits(&self, (x, y): (u32, u32), (width, height): (u32, u32)) -> bool {
        (self.region.left, self.region.top) == (x & !1, y & !1)
            && self.size == (width.next_multiple_of(2), height.next_multiple_of(2))
    }

    /// Queues a copy of subresource `index` of `source` into `slot`, once
    /// wgpu has released the slot (`after`, 0 if never shown). Returns the
    /// `written` value that marks the frame ready.
    pub fn write(
        &mut self,
        source: &ID3D11Texture2D,
        index: u32,
        slot: usize,
        after: u64,
    ) -> Result<u64> {
        self.value += 1;
        // SAFETY: D3D11 calls on live objects; the box lies inside `source`
        // (the display aperture is inside the decoded frame).
        unsafe {
            if after > 0 {
                self.context.Wait(&self.released, after)?;
            }
            self.context.CopySubresourceRegion(
                &self.textures[slot],
                0,
                0,
                0,
                0,
                source,
                index,
                Some(&self.region),
            );
            self.context.Signal(&self.written, self.value)?;
            // Submit now: the engine may wait on this value any moment.
            self.context.Flush();
        }
        Ok(self.value)
    }
}

/// The engine's half: wgpu views of the slots and the fences that order
/// access to them.
pub struct Reader {
    /// One bind group (luma + chroma) per slot.
    pub bind_groups: Vec<wgpu::BindGroup>,
    queue: wgpu::Queue,
    written: ID3D12Fence,
    released: ID3D12Fence,
    value: u64,
}

impl Reader {
    /// Opens the decoder's slots in wgpu. `bind` makes a bind group from
    /// the luma and chroma views of one slot.
    pub fn import(
        gpu: &Gpu,
        released: ID3D12Fence,
        slots: &Slots,
        bind: impl Fn(&wgpu::TextureView, &wgpu::TextureView) -> wgpu::BindGroup,
    ) -> Result<Self> {
        let (written, resources) = {
            // SAFETY: only uses the raw device to open shared objects.
            let hal = unsafe { gpu.device.as_hal::<Dx12>() }.context("wgpu is not on D3D12")?;
            let device = hal.raw_device();
            let written: ID3D12Fence = open(device, &slots.written)?;
            let resources = (slots.textures.iter())
                .map(|handle| open::<ID3D12Resource>(device, handle))
                .collect::<Result<Vec<_>>>()?;
            (written, resources)
        };
        let (width, height) = slots.size;
        let bind_groups = resources
            .iter()
            .map(|resource| {
                let luma = plane(
                    gpu,
                    resource,
                    0,
                    wgpu::TextureFormat::R8Unorm,
                    (width, height),
                );
                let chroma = plane(
                    gpu,
                    resource,
                    1,
                    wgpu::TextureFormat::Rg8Unorm,
                    (width / 2, height / 2),
                );
                bind(&luma, &chroma)
            })
            .collect();
        Ok(Self {
            bind_groups,
            queue: gpu.queue.clone(),
            written,
            released,
            value: 0,
        })
    }

    /// Makes wgpu's next submission wait until the decoder has written `ready`.
    pub fn show(&self, ready: u64) {
        // SAFETY: only stages a wait on wgpu's queue.
        if let Some(queue) = unsafe { self.queue.as_hal::<Dx12>() } {
            queue.add_wait_fence(self.written.clone(), ready);
        }
    }

    /// Frees the slot just taken off screen once wgpu's next submission (and
    /// so every earlier one that sampled it) completes. Returns the value
    /// the decoder waits for before reusing it.
    pub fn release(&mut self) -> u64 {
        self.value += 1;
        // SAFETY: only stages a signal on wgpu's queue.
        if let Some(queue) = unsafe { self.queue.as_hal::<Dx12>() } {
            queue.add_signal_fence(self.released.clone(), self.value);
        }
        self.value
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: only edits wgpu's staged fence lists and signals our own fence.
        unsafe {
            if let Some(queue) = self.queue.as_hal::<Dx12>() {
                queue.remove_wait_fence(&self.written);
                queue.remove_signal_fence(&self.released);
            }
            // Release every slot: a decoder GPU queue waiting on a release
            // that will now never be staged would otherwise never drain.
            let _ = self.released.Signal(u64::MAX);
        }
    }
}

fn open<T: Interface>(device: &ID3D12Device, handle: &Handle) -> Result<T> {
    let mut object = None;
    // SAFETY: `handle` is a live NT handle to a shared object of type `T`.
    unsafe { device.OpenSharedHandle(handle.0, &mut object) }?;
    object.context("OpenSharedHandle returned nothing")
}

/// One plane of a shared NV12 slot, as a sample-only wgpu texture view.
fn plane(
    gpu: &Gpu,
    resource: &ID3D12Resource,
    plane: u32,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let desc = wgpu::TextureDescriptor {
        label: Some("video slot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    // SAFETY: `resource` is a live NV12 texture on wgpu's adapter, and
    // `plane`/`format`/`size` describe one of its planes. It is only ever
    // sampled, a state D3D12 enters implicitly from COMMON, hence the
    // `RESOURCE` initial state (no barriers; see the module docs).
    let texture = unsafe {
        let raw = wgpu::hal::dx12::Device::texture_from_raw(
            resource.clone(),
            format,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        )
        .with_plane_slice(plane);
        gpu.device
            .create_texture_from_hal::<Dx12>(raw, &desc, wgpu::TextureUses::RESOURCE)
    };
    texture.create_view(&Default::default())
}
