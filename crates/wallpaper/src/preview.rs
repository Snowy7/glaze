//! Glass for Studio's previews (plans/native/studio-redesign.md, D3). Studio has no GPU device,
//! so it asks the engine: "draw the wallpaper with these glass plates at W×H into shared
//! memory". The engine answers once per request, from the wallpaper frame it already keeps for
//! the desktop's own glass, with the same glass shader.
//!
//! The protocol, shared with `crates/studio/src/glass.rs` (keep the two in step):
//! - Studio creates the section `Local\mypaper.preview` ([`Header`] then `width * height` BGRA
//!   pixels, premultiplied, opaque) and the auto-reset event `Local\mypaper.preview.done`;
//! - the engine creates the manual-reset event `Local\mypaper.preview.request` and waits on it
//!   with its other handles. Studio fills a request (bumping `request`) and sets it;
//! - the engine maps the section, draws, writes the pixels, `status` and then `done = request`,
//!   unmaps it, and sets the done event.
//!
//! While Studio is closed nothing is mapped and the request event never fires: the engine pays
//! one idle handle in its wait. Textures and buffers live only while a request is answered.

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW,
    UnmapViewOfFile,
};
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, OpenEventW, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows::core::w;

use crate::gpu::Gpu;

const MAGIC: u32 = 0x5650_504d; // "MPPV"
const VERSION: u32 = 1;
const MAX_PLATES: usize = 32;
/// The largest preview, in pixels.
const MAX_SIZE: (u32, u32) = (2048, 1280);
/// Pixels start here, after the header.
const PIXELS: usize = 4096;

/// A plate in preview pixels, with the glass material (clarity, blur, refraction, highlight,
/// and a straight-alpha tint).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Plate {
    pub rect: [f32; 4],
    pub radius: f32,
    pub clarity: f32,
    pub blur: f32,
    pub refraction: f32,
    pub highlight: f32,
    pub _pad: [f32; 3],
    pub tint: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    /// Bumped by Studio for every request.
    pub request: u64,
    /// The request the pixels answer, written last by the engine.
    pub done: u64,
    pub width: u32,
    pub height: u32,
    /// Which screen's wallpaper (0: the first).
    pub screen: u32,
    /// 0 drawn; 1 no frame to draw from (a web or app wallpaper, or none yet); 2 bad request.
    pub status: u32,
    pub count: u32,
    pub _pad: u32,
    pub plates: [Plate; MAX_PLATES],
}

const _: () = assert!(size_of::<Header>() <= PIXELS);

/// The engine's end: the request event and, while answering, the GPU objects.
pub struct Preview {
    event: Option<HANDLE>,
}

impl Preview {
    pub fn new() -> Self {
        // SAFETY: a named manual-reset event owned by this process until drop.
        let event =
            unsafe { CreateEventW(None, true, false, w!("Local\\mypaper.preview.request")) }
                .inspect_err(|e| log::warn!("Studio previews unavailable: {e}"))
                .ok();
        Self { event }
    }

    /// Wait on this with the engine's other handles.
    pub fn handle(&self) -> Option<HANDLE> {
        self.event
    }

    /// Answers a pending request from the screen's kept wallpaper frame (`None`: a source
    /// that draws itself, or no frame yet).
    pub fn serve(&mut self, gpu: &Gpu, kept: Option<&wgpu::Texture>) {
        let Some(event) = self.event else { return };
        // SAFETY: our own event; a zero-timeout poll and a reset.
        unsafe {
            if WaitForSingleObject(event, 0) != WAIT_OBJECT_0 {
                return;
            }
            let _ = ResetEvent(event);
        }
        if let Err(e) = answer(gpu, kept) {
            log::warn!("Studio preview: {e:#}");
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(event) = self.event.take() {
            // SAFETY: our own handle, closed once.
            let _ = unsafe { CloseHandle(event) };
        }
    }
}

fn answer(gpu: &Gpu, kept: Option<&wgpu::Texture>) -> Result<()> {
    // SAFETY: Studio's section, mapped whole and unmapped below; the header is checked before
    // the pixel range (inside the mapping Studio sized for MAX_SIZE) is written.
    unsafe {
        let mapping = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("Local\\mypaper.preview"))
            .context("no preview section")?;
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 0);
        let result = (|| -> Result<()> {
            anyhow::ensure!(!view.Value.is_null(), "mapping the preview section");
            let header = &mut *(view.Value as *mut Header);
            if header.magic != MAGIC || header.version != VERSION {
                bail!("preview protocol mismatch");
            }
            let (w, h) = (header.width, header.height);
            let count = header.count as usize;
            header.status =
                if w == 0 || h == 0 || w > MAX_SIZE.0 || h > MAX_SIZE.1 || count > MAX_PLATES {
                    2
                } else if let Some(kept) = kept {
                    let pixels = std::slice::from_raw_parts_mut(
                        (view.Value as *mut u8).add(PIXELS),
                        w as usize * h as usize * 4,
                    );
                    draw(gpu, kept, (w, h), &header.plates[..count], pixels)?;
                    0
                } else {
                    1
                };
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            header.done = header.request;
            Ok(())
        })();
        let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.Value });
        let _ = CloseHandle(mapping);
        if let Ok(done) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\mypaper.preview.done")) {
            let _ = SetEvent(done);
            let _ = CloseHandle(done);
        }
        result
    }
}

/// One glass plate as `glass.wgsl` reads it (the desktop pass's instance layout).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    shape: [f32; 4],
    tint: [f32; 4],
    extra: [f32; 4],
}

/// The wallpaper stretched to `size`, the plates over it, read back into `out` (BGRA).
fn draw(
    gpu: &Gpu,
    kept: &wgpu::Texture,
    size: (u32, u32),
    plates: &[Plate],
    out: &mut [u8],
) -> Result<()> {
    let device = &gpu.device;
    let target = gpu.texture(
        "preview",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let view = target.create_view(&Default::default());
    let kept_view = kept.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let quad = gpu.blit.quad(device, &kept_view);
    quad.set(
        &gpu.queue,
        (kept.width(), kept.height()),
        size,
        mypaper_theme::Fit::Stretch,
        1.0,
    );
    {
        let mut pass =
            crate::sources::begin(&mut encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        quad.draw(&gpu.blit, &mut pass);
    }
    if !plates.is_empty() {
        glass(gpu, &kept_view, size, plates, &view, &mut encoder);
    }
    let row = (size.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview readback"),
        size: u64::from(row) * u64::from(size.1),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size.1),
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = sender.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv()??;
    let mapped = readback.slice(..).get_mapped_range()?;
    let swap = matches!(
        gpu.format,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
    );
    let width = size.0 as usize * 4;
    for (y, line) in out.chunks_exact_mut(width).enumerate() {
        let src = &mapped[y * row as usize..y * row as usize + width];
        for (d, s) in line.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let (r, b) = if swap { (s[0], s[2]) } else { (s[2], s[0]) };
            d.copy_from_slice(&[b, s[1], r, 255]);
        }
    }
    drop(mapped);
    readback.unmap();
    Ok(())
}

/// The desktop's glass shader, drawn over the preview target.
fn glass(
    gpu: &Gpu,
    behind: &wgpu::TextureView,
    size: (u32, u32),
    plates: &[Plate],
    view: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) {
    let device = &gpu.device;
    let module = device.create_shader_module(wgpu::include_wgsl!("glass.wgsl"));
    let entry = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("preview glass"),
        entries: &[
            entry(
                0,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            entry(
                1,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(
                2,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("preview glass"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let attributes =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("preview glass"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: gpu.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("preview glass"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let instances: Vec<Instance> = plates
        .iter()
        .map(|p| Instance {
            rect: p.rect,
            shape: [p.radius, p.clarity, p.refraction, p.highlight],
            tint: p.tint,
            extra: [p.blur, 0.0, 0.0, 0.0],
        })
        .collect();
    let buffer = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
        let b = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&b, 0, bytes);
        b
    };
    let uniforms = buffer(
        "preview glass",
        bytemuck::cast_slice(&[size.0 as f32, size.1 as f32, 0.0, 0.0]),
        wgpu::BufferUsages::UNIFORM,
    );
    let vertices = buffer(
        "preview plates",
        bytemuck::cast_slice(&instances),
        wgpu::BufferUsages::VERTEX,
    );
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("preview glass"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(behind),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let mut pass = crate::sources::begin(encoder, view, wgpu::LoadOp::Load);
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &bind, &[]);
    pass.set_vertex_buffer(0, vertices.slice(..));
    // 30 vertices per plate: the rim's four bands and the face (glass_pass's VERTICES).
    pass.draw(0..30, 0..instances.len() as u32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_layout_is_what_studio_writes() {
        // Studio's copy of these structs (crates/studio/src/glass.rs) asserts the same numbers.
        assert_eq!(size_of::<Plate>(), 64);
        assert_eq!(size_of::<Header>(), 48 + 64 * MAX_PLATES);
        assert_eq!(std::mem::offset_of!(Header, plates), 48);
    }
}
