//! Shared textured-quad pipeline: draws a texture onto a screen with a
//! [`Fit`] mode and an opacity (for cross-fades). Images, slideshows,
//! videos and down-scaled renders all present through this.

use std::cell::Cell;

use bytemuck::{Pod, Zeroable};
use mypaper_theme::Fit;

/// Mirrors `struct Blit` in blit.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct Params {
    scale: [f32; 2],
    offset: [f32; 2],
    opacity: f32,
    _pad: [f32; 3],
}

pub struct Blitter {
    pipeline: wgpu::RenderPipeline,
    /// The same draw without blending, for a full-target opaque cover (see `BlitQuad::cover`).
    opaque: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// A texture bound for blitting. One per texture; reusable across screens
/// because every screen's frame is its own queue submission.
pub struct BlitQuad {
    params: wgpu::Buffer,
    /// What `params` was last given: a draw like the last one uploads nothing. Every
    /// `write_buffer` costs a staging buffer and an extra command list at the next submit.
    written: Cell<Option<Params>>,
    bind_group: wgpu::BindGroup,
}

impl Blitter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("blit.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: Some(&pipeline_layout),
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
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline: pipeline(Some(wgpu::BlendState::ALPHA_BLENDING)),
            opaque: pipeline(None),
            layout,
            sampler,
        }
    }

    pub fn quad(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> BlitQuad {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blit"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        BlitQuad {
            params,
            written: Cell::new(None),
            bind_group,
        }
    }
}

impl BlitQuad {
    /// Sets how the next draw maps a `src`-sized texture onto a `dst`-sized screen.
    pub fn set(
        &self,
        queue: &wgpu::Queue,
        src: (u32, u32),
        dst: (u32, u32),
        fit: Fit,
        opacity: f32,
    ) {
        let (scale, offset) = fit_transform(src, dst, fit);
        let params = Params {
            scale,
            offset,
            opacity,
            _pad: [0.0; 3],
        };
        if self.written.replace(Some(params)) != Some(params) {
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        }
    }

    pub fn draw(&self, blitter: &Blitter, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&blitter.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Like [`draw`](Self::draw) without blending. With a `Stretch` fit at opacity 1 it writes
    /// every target pixel exactly as `draw` over any background would, so the pass needs no
    /// clear: it may begin with `LoadOp::DontCare`.
    pub fn cover(&self, blitter: &Blitter, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&blitter.opaque);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Maps screen UV (0..1) to texture UV: `tex = screen * scale + offset`.
/// Texture UVs outside 0..1 are drawn black (letterbox).
pub fn fit_transform(src: (u32, u32), dst: (u32, u32), fit: Fit) -> ([f32; 2], [f32; 2]) {
    let (sw, sh) = (src.0.max(1) as f32, src.1.max(1) as f32);
    let (dw, dh) = (dst.0.max(1) as f32, dst.1.max(1) as f32);
    let (src_aspect, dst_aspect) = (sw / sh, dw / dh);
    let scale = match fit {
        Fit::Stretch => [1.0, 1.0],
        Fit::Center => [dw / sw, dh / sh],
        Fit::Cover if src_aspect > dst_aspect => [dst_aspect / src_aspect, 1.0],
        Fit::Cover => [1.0, src_aspect / dst_aspect],
        Fit::Contain if src_aspect > dst_aspect => [1.0, src_aspect / dst_aspect],
        Fit::Contain => [dst_aspect / src_aspect, 1.0],
    };
    let offset = [(1.0 - scale[0]) / 2.0, (1.0 - scale[1]) / 2.0];
    (scale, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    #[test]
    fn cover_crops_the_long_side() {
        // 2:1 image on a 1:1 screen: show the middle half horizontally.
        let (scale, offset) = fit_transform((2000, 1000), (1000, 1000), Fit::Cover);
        assert!(close(scale, [0.5, 1.0]) && close(offset, [0.25, 0.0]));
    }

    #[test]
    fn contain_letterboxes() {
        // 2:1 image on a 1:1 screen: full width, bars top and bottom.
        let (scale, offset) = fit_transform((2000, 1000), (1000, 1000), Fit::Contain);
        assert!(close(scale, [1.0, 2.0]) && close(offset, [0.0, -0.5]));
    }

    #[test]
    fn center_is_pixel_exact_and_stretch_is_identity() {
        let (scale, offset) = fit_transform((500, 500), (1000, 1000), Fit::Center);
        assert!(close(scale, [2.0, 2.0]) && close(offset, [-0.5, -0.5]));
        let (scale, offset) = fit_transform((123, 456), (1920, 1080), Fit::Stretch);
        assert!(close(scale, [1.0, 1.0]) && close(offset, [0.0, 0.0]));
    }
}
