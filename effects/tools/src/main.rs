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
    let missing_previews = args.iter().any(|s| s == "--missing-previews");
    let include_reference = args.iter().any(|s| s == "--include-reference");
    let rendering = missing_previews || args.iter().any(|s| s == "--render");
    let finite = args.iter().any(|s| s == "--check-finite");
    let bloom_identity = args.iter().any(|s| s == "--check-bloom-identity");
    let previews = !args.iter().any(|s| s == "--no-previews");
    let update = args.iter().any(|s| s == "--update-costs");
    let event_peak = args.iter().any(|s| s == "--event-peak");
    let option = |name: &str, default: &str| {
        args.windows(2)
            .find(|w| w[0] == name)
            .map_or(default.to_owned(), |w| w[1].clone())
    };
    let out = PathBuf::from(option("--out", "C:/mypaper-targets/effects-sheet"));
    let photo = PathBuf::from(option("--photo", "themes/rain-city-amber/media/city.jpg"));
    let only = option("--only", "");
    let mouse = option("--mouse", "0.63,0.42");
    let (mouse_x, mouse_y) = mouse.split_once(',').context("--mouse is X,Y in 0..1")?;
    let mouse = [mouse_x.parse::<f32>()?, mouse_y.parse::<f32>()?];
    anyhow::ensure!(
        mouse
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "--mouse is X,Y in 0..1"
    );
    let values: BTreeMap<String, Value> = serde_json::from_str(&option("--params", "{}"))?;
    anyhow::ensure!(
        !update || (values.is_empty() && !event_peak),
        "costs must be measured at defaults"
    );
    let lab = if rendering || finite || bloom_identity {
        Some(Lab::new(
            &photo,
            args.iter().any(|s| s == "--depth-ramp"),
            option("--clock-hours", "17.5")
                .parse::<f32>()?
                .clamp(0.0, 24.0),
            bloom_identity,
            args.windows(2)
                .find(|w| w[0] == "--time")
                .map(|w| w[1].parse())
                .transpose()?,
            mouse,
            event_peak,
        )?)
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
        // These reference packages remain owned elsewhere; grain belongs to the effect lab.
        let id = manifest["id"].as_str().unwrap().to_owned();
        let reference = ["rain", "blur", "color", "vignette"].contains(&id.as_str());
        if reference && !missing_previews && !include_reference {
            continue;
        }
        if missing_previews && entry.path().join("preview.jpg").exists() {
            continue;
        }
        if !only.is_empty() && !only.split(',').any(|name| name == id) {
            continue;
        }
        if let Some(lab) = &lab {
            if bloom_identity {
                if id == "bloom" {
                    let (image, _) = lab.render(
                        &manifest,
                        &sources,
                        &id,
                        &BTreeMap::from([("amount".to_owned(), serde_json::json!(0))]),
                        false,
                    )?;
                    let error = image
                        .as_raw()
                        .iter()
                        .zip(lab.photo.as_raw())
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or(0);
                    anyhow::ensure!(
                        error <= 1,
                        "zero-strength bloom changed the sharp layer input by {error} byte levels"
                    );
                    println!(
                        "bloom: zero strength preserves sharp layer input (max error {error}/255), with a different original base"
                    );
                }
                continue;
            }
            if finite {
                let mut cases = vec![BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
                for p in &loaded.manifest.params {
                    use mypaper_theme::stack::ParamKind;
                    match &p.kind {
                        ParamKind::Float { min, max, .. } => {
                            cases[1].insert(p.id.clone(), serde_json::json!(min));
                            cases[2].insert(p.id.clone(), serde_json::json!(max));
                        }
                        ParamKind::Bool { .. } => {
                            cases[1].insert(p.id.clone(), serde_json::json!(false));
                            cases[2].insert(p.id.clone(), serde_json::json!(true));
                        }
                        ParamKind::Enum { options, .. } => {
                            for option in options {
                                cases.push(BTreeMap::from([(
                                    p.id.clone(),
                                    serde_json::json!(option),
                                )]));
                            }
                        }
                        ParamKind::Color { .. } => {
                            cases[1].insert(p.id.clone(), serde_json::json!("#000000"));
                            cases[2].insert(p.id.clone(), serde_json::json!("#ffffff"));
                        }
                    }
                }
                for values in &cases {
                    lab.render(&manifest, &sources, &id, values, true)
                        .with_context(|| format!("{id}: {values:?}"))?;
                }
                println!("{id}: {} finite RGBA16F cases", cases.len());
                continue;
            }
            let (image, ms) = lab.render(&manifest, &sources, &id, &values, false)?;
            let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
            save_jpeg(&rgb, &out.join(format!("{id}.jpg")))?;
            if previews {
                save_jpeg(
                    &image::imageops::resize(&rgb, 320, 180, image::imageops::FilterType::Lanczos3),
                    &entry.path().join("preview.jpg"),
                )?;
            }
            if update && !reference {
                manifest["cost"] = serde_json::json!((ms * 1000.0).round() / 1000.0);
                manifest["cost_estimated"] = serde_json::json!(!lab.integrated);
                fs::write(&path, serde_json::to_string_pretty(&manifest)? + "\n")?;
            }
            println!("{id}: {ms:.3} ms");
            report.push(serde_json::json!({"id": id,"category": manifest["category"],"animated":manifest["animated"],"gpu_ms":ms}));
        }
    }
    println!("Validated {count} passes with naga 30.");
    if rendering && !finite {
        let lab = lab.as_ref().unwrap();
        save_jpeg(
            &image::DynamicImage::ImageRgba8(lab.photo.clone()).to_rgb8(),
            &out.join("before.jpg"),
        )?;
        fs::write(
            out.join("timings.json"),
            serde_json::to_string_pretty(
                &serde_json::json!({"adapter":lab.adapter,"integrated":lab.integrated,"width":WIDTH,"height":HEIGHT,"frames":24,"photo":photo,"effects":report}),
            )?,
        )?;
    }
    Ok(())
}

/// Keep unchanged thumbnails intact, including when a gallery has their files mapped open.
fn save_jpeg(image: &image::RgbImage, path: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95).encode(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    if fs::read(path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
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
    base_original: wgpu::Texture,
    depth: wgpu::Texture,
    sampler: wgpu::Sampler,
    photo: image::RgbaImage,
    adapter: String,
    integrated: bool,
    clock_hours: f32,
    time: Option<f32>,
    mouse: [f32; 2],
    event_peak: bool,
}

impl Lab {
    fn new(
        photo: &Path,
        depth_ramp: bool,
        clock_hours: f32,
        different_original: bool,
        time: Option<f32>,
        mouse: [f32; 2],
        event_peak: bool,
    ) -> Result<Self> {
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
                texture_entry(5, true),
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
        let base_original = if different_original {
            let black = texture(&device, 1, 1, FORMAT);
            queue.write_texture(
                black.as_image_copy(),
                &[0, 0, 0, 255],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                black.size(),
            );
            black
        } else {
            original.clone()
        };
        let depth_width = if depth_ramp { WIDTH } else { 1 };
        let depth_height = if depth_ramp { HEIGHT } else { 1 };
        let depth = texture(
            &device,
            depth_width,
            depth_height,
            wgpu::TextureFormat::R32Float,
        );
        let depth_data: Vec<f32> = (0..depth_width * depth_height)
            .map(|i| {
                if depth_ramp {
                    1.0 - (i % depth_width) as f32 / (depth_width - 1) as f32
                } else {
                    1.0
                }
            })
            .collect();
        queue.write_texture(
            depth.as_image_copy(),
            bytemuck::cast_slice(&depth_data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(depth_width * 4),
                rows_per_image: Some(depth_height),
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
            base_original,
            depth,
            sampler,
            photo,
            adapter: info.name,
            integrated: info.device_type == wgpu::DeviceType::IntegratedGpu,
            clock_hours,
            time,
            mouse,
            event_peak,
        })
    }

    fn render(
        &self,
        manifest: &Value,
        sources: &[String],
        id: &str,
        values: &BTreeMap<String, Value>,
        finite: bool,
    ) -> Result<(image::RgbaImage, f64)> {
        let typed = mypaper_theme::stack::parse_effect(&serde_json::to_string(manifest)?)?;
        let uniform = mypaper_theme::stack::params_bytes(&typed.params, values);
        let format = if finite {
            wgpu::TextureFormat::Rgba16Float
        } else {
            FORMAT
        };
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
                format,
            ));
            // Compute the representative flash time with the shader's own hash. CPU/GPU
            // floating-point contraction can shift this short flash outside a CPU-picked frame.
            let source = if self.event_peak && id == "lightning" {
                source.replace("paper.time", "(hash21(vec2f(0.0, 17.0)) * 0.5 * 60.0 / clamp(params.frequency, 0.5, 15.0) + 0.01)")
            } else {
                source.clone()
            };
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(id),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
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
                                format,
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
                            &self.base_original.create_view(&Default::default()),
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
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(
                            &self.original.create_view(&Default::default()),
                        ),
                    },
                ],
            }));
        }
        let passes = sources.len() as u32;
        let frames = if finite { 1 } else { 24 };
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
                    w * self.mouse[0],
                    h * self.mouse[1],
                    self.time
                        .unwrap_or_else(|| capture_time(manifest, id, values, frame)),
                    f32::from_bits(frame),
                    self.clock_hours,
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
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        let mut times: Vec<f64> = stamps
            .chunks_exact((passes * 2) as usize)
            .map(|frame| {
                frame
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| (s[1] - s[0]) as f64)
                    .sum::<f64>()
                    * f64::from(self.queue.get_timestamp_period())
                    / 1_000_000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let ms = times[times.len() / 2];
        if finite {
            for target in &textures {
                for word in self.read_texture(target, 8)?.as_chunks::<2>().0 {
                    let bits = u16::from_le_bytes(*word);
                    anyhow::ensure!(bits & 0x7c00 != 0x7c00, "non-finite output in {id}");
                }
            }
            return Ok((image::RgbaImage::new(1, 1), ms));
        }
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
        image::RgbaImage::from_raw(
            texture.width(),
            texture.height(),
            self.read_texture(texture, 4)?,
        )
        .context("image readback")
    }

    fn read_texture(&self, texture: &wgpu::Texture, bytes_per_pixel: u32) -> Result<Vec<u8>> {
        let w = texture.width();
        let h = texture.height();
        let row = (w * bytes_per_pixel).div_ceil(256) * 256;
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
            .flat_map(|row| row[..(w * bytes_per_pixel) as usize].iter().copied())
            .collect();
        Ok(packed)
    }
}

/// Capture rare effects during an event so their gallery thumbnail shows what they do.
fn capture_time(manifest: &Value, id: &str, values: &BTreeMap<String, Value>, frame: u32) -> f32 {
    let freq = values
        .get("frequency")
        .and_then(Value::as_f64)
        .or_else(|| {
            manifest["params"]
                .as_array()?
                .iter()
                .find(|p| p["id"] == "frequency")?["default"]
                .as_f64()
        })
        .unwrap_or(4.0) as f32;
    match id {
        "lightning" => hash21([0.0, 17.0]) * 0.5 * 60.0 / freq.clamp(0.5, 15.0) + 0.025,
        "glitch" => hash21([0.0, 31.0]) * 0.65 * 60.0 / freq.clamp(0.5, 20.0) + 0.02,
        _ => 12.0 + frame as f32 / 30.0,
    }
}

fn hash21(p: [f32; 2]) -> f32 {
    let mut x = (p[0] * 123.34).fract();
    let mut y = (p[1] * 456.21).fract();
    let dot = x * (x + 45.32) + y * (y + 45.32);
    x += dot;
    y += dot;
    (x * y).fract()
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
