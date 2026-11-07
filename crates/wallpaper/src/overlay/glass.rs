//! An overlay's GPU work: Liquid Glass (liquid-rust, the plates' renderer, see
//! `crate::glass_pass`) drawn over a crop of the backdrop into the overlay's glass texture (which
//! the window shows), and the compare pass (`compare.wgsl`) that says whether a fresh crop
//! differs from the one on screen.
//!
//! The crops, the glass and liquid-rust's coordinates are one space: the overlay's frame (the
//! surface grown by the glass's outset). The glass is drawn over a transparent clear, so the
//! window shows only the glass, its rim light and its drop shadow, premultiplied, as
//! DirectComposition wants.
//!
//! liquid-rust reads and writes linear colour through sRGB views. The crops can't have one (they
//! are D3D12 resources shared with the capture and DirectComposition), so each draw copies the
//! crop into an sRGB texture and the glass back out of one: two copies of a small texture.

use anyhow::Result;
use liquid_rust as lr;
use mypaper_desktop::Rect;

use super::capture::{CaptureDevice, Crop};
use super::request::{Material, Style};

/// The crop and glass format (the capture's own).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// What liquid-rust reads and draws: the crops' format, as sRGB.
const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The overlay material as the plates' controls (`glass_pass::liquid`). The tint's colour sets
/// the colour scheme; clarity tints the glass through the transparency slider. The fourth byte
/// is a stained tint's strength: the shell sends 0, a plate drawn as an overlay (a web or app
/// wallpaper's, [`super::PlateOverlay`]) keeps its own.
pub fn controls(material: &Material) -> mypaper_glass_ipc::Material {
    let tint = material.tint.map(|c| f32::from(c) / 255.0);
    mypaper_glass_ipc::Material {
        clarity: material.clarity,
        blur: material.blur,
        refraction: material.refraction,
        highlight: material.highlight,
        tint,
    }
}

/// The pipelines, shared by every overlay.
pub struct Pipelines {
    /// Compiled once; each overlay forks it.
    glass: lr::Renderer,
    compare: wgpu::ComputePipeline,
    compare_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let entry = |binding: u32, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let unfiltered = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let compute = wgpu::ShaderStages::COMPUTE;
        let compare_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass overlay compare"),
            entries: &[
                entry(0, compute, unfiltered),
                entry(1, compute, unfiltered),
                entry(
                    2,
                    compute,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let compare = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("glass overlay compare"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("glass overlay compare"),
                    bind_group_layouts: &[Some(&compare_layout)],
                    immediate_size: 0,
                }),
            ),
            module: &device.create_shader_module(wgpu::include_wgsl!("compare.wgsl")),
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            glass: lr::Renderer::new(device, SRGB),
            compare,
            compare_layout,
        }
    }
}

/// One overlay's crops, glass and buffers. The two crops take turns: the glass samples the
/// `shown` one while the next frame is copied into the other and compared with it.
pub struct Targets {
    crops: [Crop; 2],
    shown: usize,
    /// What the window shows: the glass, copied out of `drawn`.
    target: Crop,
    /// The crop being refracted, and the glass drawn over a clear (both sRGB).
    content: wgpu::TextureView,
    drawn: wgpu::Texture,
    renderer: lr::Renderer,
    /// One glass, physics off: the window's opacity does the fade.
    scene: lr::Scene,
    glass: lr::GlassId,
    scale: f32,
    /// What the scene's glass was last set to (`set_plate`): the same again resolves nothing.
    plate: Option<(Rect, f32, Material, Style, f32)>,
    /// Per crop: that crop compared against the other.
    compare: [wgpu::BindGroup; 2],
    differ: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl Targets {
    pub fn new(
        device: &wgpu::Device,
        pipes: &Pipelines,
        capture: &CaptureDevice,
        size: (u32, u32),
    ) -> Result<Self> {
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
        let crops = [
            capture.crop(device, size, sampled)?,
            capture.crop(device, size, sampled)?,
        ];
        let glass_target = capture.crop(device, size, wgpu::TextureUsages::COPY_DST)?;
        let texture = |label: &str, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SRGB,
                usage,
                view_formats: &[],
            })
        };
        let content = texture(
            "glass overlay content",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        )
        .create_view(&Default::default());
        let drawn = texture(
            "glass overlay drawn",
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let copy_dst = wgpu::BufferUsages::COPY_DST;
        let differ = buffer(
            "glass overlay differ",
            4,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | copy_dst,
        );
        let readback = buffer(
            "glass overlay differ readback",
            4,
            wgpu::BufferUsages::MAP_READ | copy_dst,
        );
        let views = crops
            .each_ref()
            .map(|c| c.texture.create_view(&Default::default()));
        let compare = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass overlay compare"),
                layout: &pipes.compare_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[1 - i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: differ.as_entire_binding(),
                    },
                ],
            })
        });
        let mut scene = lr::Scene::new();
        scene.physics = false;
        let glass = scene.add(lr::Glass::new(lr::Rect::new(0.0, 0.0, 1.0, 1.0)));
        Ok(Self {
            crops,
            shown: 0,
            target: glass_target,
            content,
            drawn,
            renderer: pipes.glass.fork(device),
            scene,
            glass,
            scale: 1.0,
            plate: None,
            compare,
            differ,
            readback,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.crops[0].size
    }

    /// The crop the next frame is copied into.
    pub fn fresh(&self) -> &Crop {
        &self.crops[1 - self.shown]
    }

    /// Records counting the fresh crop's pixels that differ from the shown one's.
    pub fn compare(&self, pass: &mut wgpu::ComputePass, pipes: &Pipelines) {
        let (w, h) = self.size();
        pass.set_pipeline(&pipes.compare);
        pass.set_bind_group(0, &self.compare[1 - self.shown], &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
    }

    /// Clears the count before a compare pass (in the same encoder).
    pub fn clear_count(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.differ, 0, None);
    }

    /// Records copying the count where it can be read (after the compare pass).
    pub fn read_count(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_buffer_to_buffer(&self.differ, 0, &self.readback, 0, 4);
    }

    /// Starts mapping the count; read it with [`count`](Self::count) once the device is polled.
    pub fn map_count(&self) {
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |_| {});
    }

    /// The mapped count (0 if it could not be read), unmapped again.
    pub fn count(&self) -> u32 {
        let value = self
            .readback
            .slice(..)
            .get_mapped_range()
            .ok()
            .map_or(0, |bytes| bytemuck::pod_read_unaligned::<u32>(&bytes[..4]));
        self.readback.unmap();
        value
    }

    /// The fresh crop becomes the shown one.
    pub fn accept(&mut self) {
        self.shown = 1 - self.shown;
    }

    /// The shown crop's pixels, tightly packed BGRA (waits for the GPU; for checks only).
    pub fn read_shown(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let (w, h) = self.size();
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass overlay crop readback"),
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let texture = &self.crops[self.shown].texture;
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let Ok(bytes) = buffer.slice(..).get_mapped_range() else {
            return Vec::new();
        };
        bytes
            .chunks(row as usize)
            .flat_map(|r| r[..w as usize * 4].iter().copied())
            .collect()
    }

    /// Sets this overlay's glass before a draw: the surface at `plate` (frame-local px), its
    /// corner radius (px), its material and style, on a screen of `scale` px per point. Resolved
    /// only when one of them changed since the last draw.
    pub fn set_plate(
        &mut self,
        plate: Rect,
        radius: f32,
        material: &Material,
        style: &Style,
        scale: f32,
    ) {
        let key = (plate, radius, *material, *style, scale);
        if self.plate == Some(key) {
            return;
        }
        self.plate = Some(key);
        let s = scale.max(0.5);
        let frame = lr::Rect::new(
            plate.x as f32 / s,
            plate.y as f32 / s,
            plate.width as f32 / s,
            plate.height as f32 / s,
        );
        let (material, appearance) = crate::material::liquid(&controls(material), s);
        let set = |v: f32| (v >= 0.0).then_some(v);
        let mut material = crate::material::styled(
            material,
            style.brightness,
            style.saturation,
            set(style.sheen),
            set(style.depth),
        );
        material.inner_shadow = style.inner_shadow;
        // DComp only has circular rectangle clips. Smoothed overlays use the shader's exact
        // continuous coverage and a rectangular DComp bound; circular defaults stay as before.
        material.clip_to_shape = style.smoothing > 0.0;
        self.scene.place(self.glass, frame);
        // Exactly the surface's own corners: quarter circles unless the style smooths them.
        self.scene.set_shape(
            self.glass,
            crate::material::shape(radius / s, style.smoothing),
        );
        self.scene.set_material(self.glass, material);
        self.scene.set_appearance(self.glass, Some(appearance));
        self.scene.update(0.0);
        self.scale = s;
    }

    /// The glass texture, for the window to show.
    pub fn glass(&self) -> &Crop {
        &self.target
    }

    /// Records the glass over the shown crop (the fresh one with `fresh`) into the glass
    /// texture. `timestamps` time everything from the clear to the last glass pass.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        fresh: bool,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let crop = if fresh { 1 - self.shown } else { self.shown };
        let size = self.size();
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        encoder.copy_texture_to_texture(
            self.crops[crop].texture.as_image_copy(),
            self.content.texture().as_image_copy(),
            extent,
        );
        let drawn = self.drawn.create_view(&Default::default());
        let (begin, end) = match &timestamps {
            Some(t) => (
                Some(wgpu::RenderPassTimestampWrites {
                    end_of_pass_write_index: None,
                    ..t.clone()
                }),
                Some(wgpu::RenderPassTimestampWrites {
                    beginning_of_pass_write_index: None,
                    ..t.clone()
                }),
            ),
            None => (None, None),
        };
        let pass = |encoder: &mut wgpu::CommandEncoder, load, timestamps| {
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &drawn,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: timestamps,
                ..Default::default()
            }));
        };
        pass(
            encoder,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            begin,
        );
        self.renderer.render(
            device,
            queue,
            encoder,
            &lr::Frame {
                content: &self.content,
                target: &drawn,
                size,
                scale: self.scale,
                content_changed: true,
                copy_content: false,
            },
            &self.scene,
        );
        if end.is_some() {
            pass(encoder, wgpu::LoadOp::Load, end);
        }
        encoder.copy_texture_to_texture(
            self.drawn.as_image_copy(),
            self.target.texture.as_image_copy(),
            extent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_take_the_plates_controls_and_clarity_alone_tints() {
        let m = Material {
            clarity: 0.4,
            blur: 30.0,
            refraction: 0.6,
            highlight: 0.75,
            tint: [255, 255, 255, 0],
        };
        let c = controls(&m);
        assert_eq!((c.clarity, c.blur, c.refraction, c.highlight), (0.4, 30.0, 0.6, 0.75));
        assert_eq!(c.tint, [1.0, 1.0, 1.0, 0.0], "a scheme, no stained tint");
        let (_, appearance) = crate::material::liquid(&c, 1.5);
        assert!(!appearance.dark && (appearance.transparency - 0.6).abs() < 1e-6);
    }
}
