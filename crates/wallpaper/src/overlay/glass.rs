//! An overlay's GPU work: the plate pass's own shader (`../glass.wgsl`, used unchanged) drawn
//! over a crop of the backdrop into the overlay's glass texture (which the window shows), and
//! the compare pass
//! (`compare.wgsl`) that says whether a fresh crop differs from the one on screen.
//!
//! The crops, the glass and the shader's coordinates are one space: the overlay's frame (the
//! surface grown by the glass's reach). The glass is cleared to transparent, so the clear
//! centre glass.wgsl never rasterizes shows the live window behind; only the rim (and a tint or
//! blur) is drawn, premultiplied, as DirectComposition wants.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use mypaper_desktop::Rect;

use super::capture::{CaptureDevice, Crop};
use super::geometry::MAX_BLUR;
use super::request::Material;

/// The crop and glass format (the capture's own).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// Vertices per plate: glass.wgsl's four rim quads and its centre's.
const VERTICES: u32 = 30;
/// The tint's opacity at clarity 0.
const MAX_TINT: f32 = 0.8;

/// Mirrors `Plate` in glass.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Instance {
    rect: [f32; 4],
    shape: [f32; 4],
    tint: [f32; 4],
    extra: [f32; 4],
}

/// Mirrors `Uniforms` in glass.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// The plate glass.wgsl draws for a surface at `plate` (frame-local px).
pub fn instance(plate: Rect, radius: f32, material: &Material) -> Instance {
    let unit = |v: f32| v.clamp(0.0, 1.0);
    let [r, g, b, _] = material.tint.map(|c| f32::from(c) / 255.0);
    Instance {
        rect: [
            plate.x as f32,
            plate.y as f32,
            plate.width as f32,
            plate.height as f32,
        ],
        shape: [
            radius.max(0.0),
            1.0,
            unit(material.refraction),
            unit(material.highlight),
        ],
        tint: [r, g, b, (1.0 - unit(material.clarity)) * MAX_TINT],
        // Full presence: the window's opacity does the fade (DirectComposition animates it).
        extra: [material.blur.clamp(0.0, MAX_BLUR), 1.0, 0.0, 0.0],
    }
}

/// The pipelines, shared by every overlay.
pub struct Pipelines {
    glass: wgpu::RenderPipeline,
    glass_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    compare: wgpu::ComputePipeline,
    compare_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("../glass.wgsl"));
        let entry = |binding: u32, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let texture = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let glass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass overlay"),
            entries: &[
                entry(
                    0,
                    both,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, both, texture),
                entry(
                    2,
                    both,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
        let glass = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glass overlay"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("glass overlay"),
                    bind_group_layouts: &[Some(&glass_layout)],
                    immediate_size: 0,
                }),
            ),
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
                    format: FORMAT,
                    // Over a transparent clear this leaves premultiplied colour.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass overlay"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

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
            glass,
            glass_layout,
            sampler,
            compare,
            compare_layout,
        }
    }
}

/// One overlay's crops and buffers. The two crops take turns: the glass samples the `shown`
/// one while the next frame is copied into the other and compared with it.
pub struct Targets {
    crops: [Crop; 2],
    shown: usize,
    /// What the glass pass draws into, and the window shows.
    target: Crop,
    uniforms: wgpu::Buffer,
    instance: wgpu::Buffer,
    /// Per crop: the glass sampling it.
    glass: [wgpu::BindGroup; 2],
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
        let glass_target = capture.crop(device, size, wgpu::TextureUsages::RENDER_ATTACHMENT)?;
        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let copy_dst = wgpu::BufferUsages::COPY_DST;
        let uniforms = buffer(
            "glass overlay",
            size_of::<Uniforms>() as u64,
            wgpu::BufferUsages::UNIFORM | copy_dst,
        );
        let instance = buffer(
            "glass overlay plate",
            size_of::<Instance>() as u64,
            wgpu::BufferUsages::VERTEX | copy_dst,
        );
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
        let glass = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass overlay"),
                layout: &pipes.glass_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&pipes.sampler),
                    },
                ],
            })
        });
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
        Ok(Self {
            crops,
            shown: 0,
            target: glass_target,
            uniforms,
            instance,
            glass,
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

    /// Writes this overlay's plate before a draw.
    pub fn set_plate(&self, queue: &wgpu::Queue, plate: &Instance) {
        let (w, h) = self.size();
        let uniforms = Uniforms {
            size: [w as f32, h as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        queue.write_buffer(&self.instance, 0, bytemuck::bytes_of(plate));
    }

    /// The glass texture, for the window to show.
    pub fn glass(&self) -> &Crop {
        &self.target
    }

    /// Records the glass over the shown crop (the fresh one with `fresh`) into the glass texture,
    /// cleared first.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipes: &Pipelines,
        fresh: bool,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let target = self.target.texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glass overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: timestamps,
            ..Default::default()
        });
        pass.set_pipeline(&pipes.glass);
        let crop = if fresh { 1 - self.shown } else { self.shown };
        pass.set_bind_group(0, &self.glass[crop], &[]);
        pass.set_vertex_buffer(0, self.instance.slice(..));
        pass.draw(0..VERTICES, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarity_sets_the_tints_opacity_and_the_rest_pass_through() {
        let plate = Rect {
            x: 9,
            y: 9,
            width: 280,
            height: 400,
        };
        let clear = instance(plate, 12.0, &Material::CLEAR);
        assert_eq!(clear.rect, [9.0, 9.0, 280.0, 400.0]);
        assert_eq!(clear.shape, [12.0, 1.0, 0.6, 0.55]);
        assert_eq!(clear.tint[3], 0.0, "clear glass has no wash");
        let tinted = instance(
            plate,
            12.0,
            &Material {
                clarity: 0.5,
                blur: 40.0,
                tint: [255, 0, 0, 0],
                ..Material::CLEAR
            },
        );
        assert_eq!(tinted.tint, [1.0, 0.0, 0.0, 0.4]);
        assert_eq!(tinted.extra[0], MAX_BLUR);
    }

    /// The plate pass's shader is used unchanged: it must still fit this module's layout (its
    /// bindings, its instance attributes and its entry points), checked without a GPU.
    #[test]
    fn glass_wgsl_still_fits_the_overlay_layout() {
        let source = include_str!("../glass.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("glass.wgsl parses");
        let entries: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            entries.contains(&"vs") && entries.contains(&"fs"),
            "{entries:?}"
        );
        let vs = module.entry_points.iter().find(|e| e.name == "vs").unwrap();
        // The vertex index plus one vec4 per Instance field.
        assert_eq!(vs.function.arguments.len(), 2);
        let bound: Vec<u32> = module
            .global_variables
            .iter()
            .filter_map(|(_, g)| g.binding.as_ref().map(|b| b.binding))
            .collect();
        assert_eq!(bound.len(), 3, "uniforms, the backdrop and its sampler");
        assert_eq!(size_of::<Instance>(), 64);
    }
}
