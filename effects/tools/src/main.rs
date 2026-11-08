use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};
use wgpu::util::DeviceExt;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let args: Vec<String> = std::env::args().collect();
    let rendering = args.iter().any(|s| s == "--render");
    let update = args.iter().any(|s| s == "--update-costs");
    let option = |name: &str, default: &str| {
        args.windows(2)
            .find(|w| w[0] == name)
            .map_or(default.to_owned(), |w| w[1].clone())
    };
    let out = PathBuf::from(option("--out", "C:/mypaper-targets/effects-sheet"));
    let photo = PathBuf::from(option("--photo", "themes/rain-city-amber/media/city.jpg"));
    let lab = if rendering {
        Some(Lab::new(&photo)?)
    } else {
        None
    };
    if rendering {
        fs::create_dir_all(&out)?;
    }
    let mut report = Vec::new();
    let mut count = 0;
    let mut entries = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path().join("effect.json");
        if !path.exists() {
            continue;
        }
        let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let loaded = mypaper_theme::stack::LoadedEffect::load(entry.path())?;
        let mut sources = Vec::new();
        for (index, pass) in manifest["passes"].as_array().unwrap().iter().enumerate() {
            let source_path = entry.path().join(pass["source"].as_str().unwrap());
            let source = loaded.pass(index)?.code;
            validate(&source_path, &source)?;
            sources.push(source);
            count += 1;
        }
        // The engine owns its five reference packages; render and update only our packages.
        let id = manifest["id"].as_str().unwrap().to_owned();
        if ["rain", "blur", "color", "vignette", "grain"].contains(&id.as_str()) {
            continue;
        }
        if let Some(lab) = &lab {
            let (image, ms) = lab.render(&manifest, &sources, &id)?;
            let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
            rgb.save(out.join(format!("{id}.jpg")))?;
            image::imageops::resize(&rgb, 320, 180, image::imageops::FilterType::Lanczos3)
                .save(entry.path().join("preview.jpg"))?;
            if update {
                manifest["cost"] = serde_json::json!((ms * 1000.0).round() / 1000.0);
                manifest["cost_estimated"] = serde_json::json!(!lab.integrated);
                fs::write(&path, serde_json::to_string_pretty(&manifest)? + "\n")?;
            }
            println!("{id}: {ms:.3} ms");
            report.push(serde_json::json!({"id": id,"category": manifest["category"],"animated":manifest["animated"],"gpu_ms":ms}));
        }
    }
    println!("Validated {count} passes with naga 30.");
    if let Some(lab) = &lab {
        image::DynamicImage::ImageRgba8(lab.photo.clone())
            .to_rgb8()
            .save(out.join("before.jpg"))?;
        fs::write(
            out.join("timings.json"),
            serde_json::to_string_pretty(
                &serde_json::json!({"adapter":lab.adapter,"integrated":lab.integrated,"width":WIDTH,"height":HEIGHT,"frames":24,"photo":photo,"effects":report}),
            )?,
        )?;
    }
    Ok(())
}

fn validate(path: &Path, code: &str) -> Result<()> {
    let module = naga::front::wgsl::parse_str(code)
        .map_err(|e| anyhow::anyhow!("{}\n{}", path.display(), e.emit_to_string(code)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .with_context(|| path.display().to_string())?;
    Ok(())
}

struct Lab {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    original: wgpu::Texture,
    depth: wgpu::Texture,
    sampler: wgpu::Sampler,
    photo: image::RgbaImage,
    adapter: String,
    integrated: bool,
}

impl Lab {
    fn new(photo: &Path) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        adapters.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::IntegratedGpu => 0,
            wgpu::DeviceType::DiscreteGpu => 1,
            _ => 2,
        });
        let adapter = adapters.into_iter().next().context("no DX12 adapter")?;
        let info = adapter.get_info();
        println!("GPU: {} ({:?})", info.name, info.device_type);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("effects offline lab"),
                required_features: wgpu::Features::IMMEDIATES | wgpu::Features::TIMESTAMP_QUERY,
                required_limits: wgpu::Limits {
                    max_immediate_size: 32,
                    ..Default::default()
                },
                ..Default::default()
            }))?;
        let texture_entry = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect contract"),
            entries: &[
                texture_entry(0, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2, true),
                texture_entry(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 32,
        });
        let photo = image::open(photo)?
            .resize_to_fill(WIDTH, HEIGHT, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let original = texture(&device, WIDTH, HEIGHT, FORMAT);
        queue.write_texture(
            original.as_image_copy(),
            &photo,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
            original.size(),
        );
        let depth = texture(&device, 1, 1, wgpu::TextureFormat::R32Float);
        queue.write_texture(
            depth.as_image_copy(),
            bytemuck::bytes_of(&1.0f32),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            depth.size(),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            device,
            queue,
            layout,
            pipeline_layout,
            original,
            depth,
            sampler,
            photo,
            adapter: info.name,
            integrated: info.device_type == wgpu::DeviceType::IntegratedGpu,
        })
    }

    fn render(
        &self,
        manifest: &Value,
        sources: &[String],
        id: &str,
    ) -> Result<(image::RgbaImage, f64)> {
        let typed = mypaper_theme::stack::parse_effect(&serde_json::to_string(manifest)?)?;
        let uniform = mypaper_theme::stack::params_bytes(&typed.params, &BTreeMap::new());
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("default params"),
                contents: &uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let mut textures = Vec::new();
        let mut pipelines = Vec::new();
        let mut groups = Vec::new();
        for (i, source) in sources.iter().enumerate() {
            let scale = manifest["passes"][i]["scale"].as_f64().unwrap_or(1.0);
            textures.push(texture(
                &self.device,
                (f64::from(WIDTH) * scale).round() as u32,
                (f64::from(HEIGHT) * scale).round() as u32,
                FORMAT,
            ));
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(id),
                    source: wgpu::ShaderSource::Wgsl(source.clone().into()),
                });
            pipelines.push(
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some(id),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &shader,
                            entry_point: Some("mypaper_vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &shader,
                            entry_point: Some("mypaper_fx"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: FORMAT,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    }),
            );
            let input = if i == 0 {
                &self.original
            } else {
                &textures[i - 1]
            };
            groups.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(id),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &input.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            &self.original.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            &self.depth.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            }));
        }
        let passes = sources.len() as u32;
        let frames = 24;
        let queries = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some(id),
            ty: wgpu::QueryType::Timestamp,
            count: frames * passes * 2,
        });
        let query_bytes = u64::from(frames * passes * 2) * 8;
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: query_bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: query_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for frame in 0..frames + 3 {
            for i in 0..sources.len() {
                let target = &textures[i];
                let view = target.create_view(&Default::default());
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let timestamp_writes = (frame >= 3).then(|| {
                    let at = ((frame - 3) * passes + i as u32) * 2;
                    wgpu::RenderPassTimestampWrites {
                        query_set: &queries,
                        beginning_of_pass_write_index: Some(at),
                        end_of_pass_write_index: Some(at + 1),
                    }
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(id),
                    color_attachments: &attachments,
                    timestamp_writes,
                    ..Default::default()
                });
                pass.set_pipeline(&pipelines[i]);
                pass.set_bind_group(0, &groups[i], &[]);
                let w = target.width() as f32;
                let h = target.height() as f32;
                let paper: [f32; 8] = [
                    w,
                    h,
                    w * 0.63,
                    h * 0.42,
                    12.0 + frame as f32 / 30.0,
                    f32::from_bits(frame),
                    0.0,
                    0.0,
                ];
                pass.set_immediates(0, bytemuck::bytes_of(&paper));
                pass.draw(0..3, 0..1);
            }
        }
        encoder.resolve_query_set(&queries, 0..frames * passes * 2, &resolve, 0);
        encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, query_bytes);
        self.queue.submit([encoder.finish()]);
        let bytes = self.map(&read)?;
        let stamps: Vec<u64> = bytes
            .chunks_exact(8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let mut times: Vec<f64> = stamps
            .chunks_exact((passes * 2) as usize)
            .map(|frame| {
                frame
                    .chunks_exact(2)
                    .map(|s| (s[1] - s[0]) as f64)
                    .sum::<f64>()
                    * f64::from(self.queue.get_timestamp_period())
                    / 1_000_000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let ms = times[times.len() / 2];
        let image = self.pixels(textures.last().unwrap())?;
        Ok((image, ms))
    }

    fn map(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = sender.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let bytes = buffer.get_mapped_range(..)?.to_vec();
        buffer.unmap();
        Ok(bytes)
    }

    fn pixels(&self, texture: &wgpu::Texture) -> Result<image::RgbaImage> {
        let w = texture.width();
        let h = texture.height();
        let row = (w * 4).div_ceil(256) * 256;
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let bytes = self.map(&read)?;
        let packed: Vec<u8> = bytes
            .chunks_exact(row as usize)
            .flat_map(|row| row[..(w * 4) as usize].iter().copied())
            .collect();
        image::RgbaImage::from_raw(w, h, packed).context("image readback")
    }
}

fn texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offline texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
