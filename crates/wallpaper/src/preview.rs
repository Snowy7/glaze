//! Glass for Studio's previews (plans/native/studio-redesign.md, D3). Studio has no GPU device,
//! so it asks the engine: "draw the wallpaper with these glass plates at W×H into shared
//! memory". The engine answers once per request, from the wallpaper frame it already keeps for
//! the desktop's own glass, with the same Liquid Glass (liquid-rust, see `glass_pass`).
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
//!
//! Any screen's drawn frame answers (the first screen's when it has one): a covered screen has
//! none until it's uncovered. With no frame at all the engine answers `status` 1 once; the same
//! request asked again waits for a frame instead of another refusal. Studio asks again as soon as
//! it's refused, so refusing at once kept both processes spinning (6,700 round trips a second,
//! 2026-11-06).

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use liquid_rust as lr;
use mypaper_glass_ipc::Material;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW,
    UnmapViewOfFile,
};
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, OpenEventW, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows::core::w;

use crate::glass_pass::GlassPass;
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
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
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

/// What a request asks for, to tell a repeat from a change.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    size: (u32, u32),
    screen: u32,
    plates: Vec<Plate>,
}

/// The engine's end: the request event and, while answering, the GPU objects.
pub struct Preview {
    event: Option<HANDLE>,
    /// The last request answered "no frame".
    refused: Option<Asked>,
    /// That request came again unchanged and waits, unanswered, for a frame to draw it from.
    parked: bool,
}

impl Preview {
    pub fn new() -> Self {
        // SAFETY: a named manual-reset event owned by this process until drop.
        let event =
            unsafe { CreateEventW(None, true, false, w!("Local\\mypaper.preview.request")) }
                .inspect_err(|e| log::warn!("Studio previews unavailable: {e}"))
                .ok();
        Self {
            event,
            refused: None,
            parked: false,
        }
    }

    /// Wait on this with the engine's other handles.
    pub fn handle(&self) -> Option<HANDLE> {
        self.event
    }

    /// Answers a new request, or the parked one once there's a frame, from `kept`: a drawn
    /// wallpaper frame (`None`: a source that draws itself, or nothing drawn yet). The glass is
    /// the desktop's own (`pass` lends its compiled pipelines).
    pub fn serve(&mut self, gpu: &Gpu, kept: Option<&wgpu::Texture>, pass: &mut GlassPass) {
        let Some(event) = self.event else { return };
        // SAFETY: our own event; a zero-timeout poll and a reset.
        let asked = unsafe {
            let set = WaitForSingleObject(event, 0) == WAIT_OBJECT_0;
            if set {
                let _ = ResetEvent(event);
            }
            set
        };
        if !asked && !(self.parked && kept.is_some()) {
            return;
        }
        self.parked = false;
        match answer(gpu, kept, &mut self.refused, pass) {
            Ok(parked) => self.parked = parked,
            Err(e) => log::warn!("Studio preview: {e:#}"),
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

/// Answers the request in Studio's section, or returns `true` without answering when it repeats
/// the request last answered "no frame" (`refused`) and there's still no frame.
fn answer(
    gpu: &Gpu,
    kept: Option<&wgpu::Texture>,
    refused: &mut Option<Asked>,
    pass: &mut GlassPass,
) -> Result<bool> {
    // SAFETY: Studio's section, mapped whole and unmapped below; the header is checked before
    // the pixel range (inside the mapping Studio sized for MAX_SIZE) is written.
    unsafe {
        let mapping = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("Local\\mypaper.preview"))
            .context("no preview section")?;
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 0);
        let result = (|| -> Result<bool> {
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
                    draw(gpu, kept, (w, h), &header.plates[..count], pixels, pass)?;
                    *refused = None;
                    0
                } else {
                    let asked = Asked {
                        size: (w, h),
                        screen: header.screen,
                        plates: header.plates[..count].to_vec(),
                    };
                    if refused.as_ref() == Some(&asked) {
                        return Ok(true);
                    }
                    *refused = Some(asked);
                    1
                };
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            header.done = header.request;
            Ok(false)
        })();
        let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.Value });
        let _ = CloseHandle(mapping);
        if matches!(result, Ok(true)) {
            return result;
        }
        if let Ok(done) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\mypaper.preview.done")) {
            let _ = SetEvent(done);
            let _ = CloseHandle(done);
        }
        result
    }
}

/// The wallpaper stretched to `size`, the plates over it, read back into `out` (BGRA).
fn draw(
    gpu: &Gpu,
    kept: &wgpu::Texture,
    size: (u32, u32),
    plates: &[Plate],
    out: &mut [u8],
    desktop: &mut GlassPass,
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
        glass(gpu, kept, size, plates, &target, &mut encoder, desktop.fork(gpu));
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
        for (d, s) in line.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
            let (r, b) = if swap { (s[0], s[2]) } else { (s[2], s[0]) };
            d.copy_from_slice(&[b, s[1], r, 255]);
        }
    }
    drop(mapped);
    readback.unmap();
    Ok(())
}

/// The desktop's glass (liquid-rust, as `glass_pass` draws it) over the preview target, at one
/// pixel per point, with `renderer` (a fork of the desktop's): Studio sends the plates in
/// preview pixels.
fn glass(
    gpu: &Gpu,
    kept: &wgpu::Texture,
    size: (u32, u32),
    plates: &[Plate],
    target: &wgpu::Texture,
    encoder: &mut wgpu::CommandEncoder,
    mut renderer: lr::Renderer,
) {
    let mut scene = lr::Scene::new();
    scene.physics = false;
    for p in plates {
        let material = Material {
            clarity: p.clarity,
            blur: p.blur,
            refraction: p.refraction,
            highlight: p.highlight,
            tint: p.tint,
        };
        let (material, appearance) = crate::material::liquid(&material, 1.0);
        let [x, y, w, h] = p.rect;
        let glass = lr::Glass::new(lr::Rect::new(x, y, w, h))
            .shape(lr::Shape::Circular(p.radius.max(0.0)))
            .material(material)
            .appearance(appearance);
        scene.add(glass);
    }
    scene.update(0.0);
    let srgb = gpu.format.add_srgb_suffix();
    let view = |t: &wgpu::Texture| {
        t.create_view(&wgpu::TextureViewDescriptor {
            format: Some(srgb),
            ..Default::default()
        })
    };
    renderer.render(
        &gpu.device,
        &gpu.queue,
        encoder,
        &lr::Frame {
            content: &view(kept),
            target: &view(target),
            size,
            scale: 1.0,
            content_changed: true,
            copy_content: false,
        },
        &scene,
    );
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
