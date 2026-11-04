//! Plates drawn directly from the shell's latest shared-memory snapshot. No motion policy,
//! backdrop copies or full-frame scratch live here: all samples use the kept wallpaper.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use mypaper_desktop::Rect;
use mypaper_glass_ipc::{MAX_PLATES, Plate, Reader};
use windows::Win32::Foundation::HANDLE;

use crate::gpu::Gpu;

const VERTICES: u32 = 30;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    shape: [f32; 4], // radius, clarity, refraction, highlight
    tint: [f32; 4],
    extra: [f32; 4], // blur, unused
}
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    size: [f32; 2],
    _pad: [f32; 2],
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
}

pub struct GlassPass {
    monitors: Vec<Rect>,
    plates: Vec<Plate>,
    dirty: Vec<bool>,
    reader: Reader,
    pipeline: Option<Pipeline>,
    bindings: Vec<Option<(wgpu::Texture, wgpu::BindGroup)>>,
    instances: Vec<Instance>,
    pub sequence: u64,
    pub published_qpc: u64,
}

fn overlaps(p: &Plate, m: &Rect) -> bool {
    let [x, y, w, h] = p.rect;
    x < (m.x + m.width) as f32
        && x + w > m.x as f32
        && y < (m.y + m.height) as f32
        && y + h > m.y as f32
}

impl GlassPass {
    pub fn new(monitors: Option<&[Rect]>) -> Result<Self> {
        let monitors = monitors.unwrap_or_default().to_vec();
        let count = monitors.len();
        Ok(Self {
            monitors,
            plates: Vec::new(),
            dirty: vec![false; count],
            reader: Reader::open()?,
            pipeline: None,
            bindings: vec![None; count],
            instances: Vec::with_capacity(MAX_PLATES),
            sequence: 0,
            published_qpc: 0,
        })
    }

    pub fn wait_handles(&self) -> Vec<HANDLE> {
        std::iter::once(self.reader.wake_event())
            .chain(self.reader.writer_process())
            .collect()
    }

    pub fn reload(&mut self) {
        match self.reader.read_changed() {
            Ok(Some(snapshot)) => {
                self.sequence = snapshot.sequence;
                self.published_qpc = snapshot.published_qpc;
                self.set_plates(snapshot.plates);
            }
            Ok(None) => {}
            Err(e) => log::error!("glass IPC: {e:#}"),
        }
    }

    pub(crate) fn set_plates(&mut self, plates: Vec<Plate>) {
        for (i, monitor) in self.monitors.iter().enumerate() {
            // Compare only this monitor's plates; identical publications cause zero drawing.
            let before = self.plates.iter().filter(|p| overlaps(p, monitor));
            let after = plates.iter().filter(|p| overlaps(p, monitor));
            self.dirty[i] |= !before.eq(after);
        }
        self.plates = plates;
    }

    pub fn take_dirty(&mut self, index: usize) -> bool {
        self.dirty.get_mut(index).is_some_and(std::mem::take)
    }

    /// Draw into the restored swapchain image, sampling the unmodified kept wallpaper.
    /// Its resolution can differ: sampling uses normalized final-screen coordinates.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        index: usize,
        size: (u32, u32),
        behind: &wgpu::Texture,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let Some(monitor) = self.monitors.get(index) else {
            return false;
        };
        self.instances.clear();
        self.instances.extend(
            self.plates
                .iter()
                .filter(|p| overlaps(p, monitor))
                .map(|p| {
                    let m = p.material;
                    Instance {
                        rect: [
                            p.rect[0] - monitor.x as f32,
                            p.rect[1] - monitor.y as f32,
                            p.rect[2],
                            p.rect[3],
                        ],
                        shape: [p.radius, m.clarity, m.refraction, m.highlight],
                        tint: m.tint,
                        extra: [m.blur, 0.0, 0.0, 0.0],
                    }
                }),
        );
        if self.instances.is_empty() {
            return false;
        }
        let pipeline = self.pipeline.get_or_insert_with(|| Pipeline::new(gpu));
        if self.bindings[index]
            .as_ref()
            .is_none_or(|(texture, _)| texture != behind)
        {
            let view = behind.create_view(&Default::default());
            let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass kept frame"),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: pipeline.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                    },
                ],
            });
            self.bindings[index] = Some((behind.clone(), binding));
        }
        gpu.queue.write_buffer(
            &pipeline.instances,
            0,
            bytemuck::cast_slice(&self.instances),
        );
        gpu.queue.write_buffer(
            &pipeline.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                size: [size.0 as f32, size.1 as f32],
                _pad: [0.0; 2],
            }),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &self.bindings[index].as_ref().unwrap().1, &[]);
        pass.set_vertex_buffer(0, pipeline.instances.slice(..));
        pass.draw(0..VERTICES, 0..self.instances.len() as u32);
        true
    }

    /// Release old source textures on theme switches, even while a screen stays hidden.
    pub fn reset(&mut self) {
        self.bindings.fill(None);
    }
}

impl Pipeline {
    fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::include_wgsl!("glass.wgsl"));
        let entry = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
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
            label: Some("glass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glass"),
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
            label: Some("glass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let buffer = |label: &str, size: usize, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            pipeline,
            layout,
            sampler,
            uniforms: buffer("glass", size_of::<Uniforms>(), wgpu::BufferUsages::UNIFORM),
            instances: buffer(
                "glass plates",
                size_of::<Instance>() * MAX_PLATES,
                wgpu::BufferUsages::VERTEX,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_glass_ipc::Material;

    #[test]
    fn geometry_is_used_as_published_and_only_affected_screens_redraw() {
        let monitors = [
            Rect {
                x: -2560,
                y: 9,
                width: 2560,
                height: 1440,
            },
            Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ];
        let mut pass = GlassPass::new(Some(&monitors)).unwrap();
        let mut p = Plate {
            id: 7,
            rect: [-2544.25, 77.0, 309.0, 307.0],
            radius: 22.0,
            material: Material::default(),
        };
        pass.set_plates(vec![p]);
        assert!(pass.take_dirty(0));
        assert!(!pass.take_dirty(1));
        assert_eq!(pass.plates[0], p);
        pass.set_plates(vec![p]);
        assert!(!pass.take_dirty(0));
        p.rect[0] = 100.0;
        pass.set_plates(vec![p]);
        assert!(pass.take_dirty(0));
        assert!(pass.take_dirty(1));
        p.material.clarity = 0.0;
        pass.set_plates(vec![p]);
        assert!(!pass.take_dirty(0));
        assert!(pass.take_dirty(1));
        pass.set_plates(vec![]);
        assert!(pass.take_dirty(1));
        assert!(!pass.take_dirty(1));
    }
}
