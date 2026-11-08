use super::*;

#[test]
fn render_scale_shrinks_targets() {
    assert_eq!(render_size((1920, 1080), 0.5), (960, 540));
    assert_eq!(render_size((1001, 1), 0.25), (251, 1));
    assert_eq!(size_of::<Globals>(), 208);
    assert_eq!(size_of::<load::MaterialUniform>(), 64);
    assert_eq!(size_of::<Vertex>(), 48);
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_buffer(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        tx.send(r).unwrap();
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = buffer.get_mapped_range(..).unwrap().to_vec();
    buffer.unmap();
    bytes
}
fn pixels(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let row = (texture.width() * 4).next_multiple_of(256);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene lab readback"),
        size: u64::from(row) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = gpu.device.create_command_encoder(&Default::default());
    e.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    gpu.queue.submit([e.finish()]);
    read_buffer(gpu, &buffer)
        .chunks_exact(row as usize)
        .flat_map(|r| r[..texture.width() as usize * 4].to_vec())
        .collect()
}

/// Offscreen only: never creates a desktop window or changes the current theme.
/// `MYPAPER_SCENE_MODEL` selects a repo-relative model; `MYPAPER_SCENE_CLIP` selects a clip.
#[test]
#[ignore = "GPU frame lab; writes C:/mypaper-targets/scene3d"]
fn lab_transparency_depth_and_cost() {
    let gpu = Gpu::headless().expect("DX12 GPU");
    let size = (1920, 1080);
    let path = root().join(
        std::env::var("MYPAPER_SCENE_MODEL")
            .unwrap_or_else(|_| "themes/examples/islands/islands.glb".into()),
    );
    let name = path.file_stem().unwrap().to_string_lossy();
    let output = std::path::Path::new("C:/mypaper-targets/scene3d");
    std::fs::create_dir_all(output).unwrap();
    let color = gpu.texture(
        "scene lab color",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let depth = gpu.texture(
        "scene lab depth",
        size,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let color_view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let options = SceneOptions {
        animation: std::env::var("MYPAPER_SCENE_CLIP").ok(),
        ..Default::default()
    };
    let mut renderer = SceneRenderer::load(&gpu, &path, &options).unwrap();
    for scale in [1.0, 0.5] {
        renderer.options.scale = scale;
        gpu.checked(|| {
            let mut e = gpu.device.create_command_encoder(&Default::default());
            renderer.render_into(
                &gpu,
                &mut e,
                &color_view,
                Some(&depth_view),
                size,
                Duration::from_secs_f32(0.3),
                [960.0, 540.0],
                true,
            );
            gpu.queue.submit([e.finish()]);
        })
        .unwrap();
        let rgba = pixels(&gpu, &color);
        let alpha: Vec<_> = rgba.chunks_exact(4).map(|p| p[3]).collect();
        assert!(
            alpha.iter().any(|&a| a > 0),
            "model alpha must not be empty"
        );
        assert!(alpha.contains(&0), "background must be clear");
        assert!(
            rgba.chunks_exact(4)
                .all(|p| p[..3].iter().all(|&c| c <= p[3])),
            "premultiplied alpha"
        );
        std::fs::write(
            output.join(format!("{name}-transparent-{scale}.bgra")),
            &rgba,
        )
        .unwrap();
        let bytes = pixels(&gpu, &depth);
        let values: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert!(
            values
                .iter()
                .all(|d| d.is_finite() && (0.0..=1.0).contains(d))
        );
        assert!(values.iter().any(|&d| d < 0.99));
        assert!(values.contains(&1.0));
        std::fs::write(output.join(format!("{name}-depth-{scale}.f32")), &bytes).unwrap();
    }
    renderer.options.scale = 1.0;
    for (i, t) in [0.0, 0.7, 1.4].into_iter().enumerate() {
        let mut e = gpu.device.create_command_encoder(&Default::default());
        renderer.render_into(
            &gpu,
            &mut e,
            &color_view,
            None,
            size,
            Duration::from_secs_f32(t),
            [960.0, 540.0],
            false,
        );
        gpu.queue.submit([e.finish()]);
        std::fs::write(
            output.join(format!("{name}-sky-{i}.bgra")),
            pixels(&gpu, &color),
        )
        .unwrap();
    }
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        eprintln!("GPU timestamps unavailable");
        return;
    }
    const FRAMES: u32 = 120;
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("scene lab timing"),
        ty: wgpu::QueryType::Timestamp,
        count: FRAMES * 2,
    });
    let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(FRAMES) * 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(FRAMES) * 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for scale in [1.0, 0.5] {
        renderer.options.scale = scale;
        // Warm resize and pipelines before timestamps.
        let mut e = gpu.device.create_command_encoder(&Default::default());
        renderer.render_into(
            &gpu,
            &mut e,
            &color_view,
            Some(&depth_view),
            size,
            Duration::from_secs(1),
            [960.0, 540.0],
            true,
        );
        gpu.queue.submit([e.finish()]);
        for i in 0..FRAMES {
            let mut e = gpu.device.create_command_encoder(&Default::default());
            e.write_timestamp(&queries, 2 * i);
            renderer.render_into(
                &gpu,
                &mut e,
                &color_view,
                Some(&depth_view),
                size,
                Duration::from_secs_f32(1.0 + i as f32 / 30.0),
                [960.0, 540.0],
                true,
            );
            e.write_timestamp(&queries, 2 * i + 1);
            gpu.queue.submit([e.finish()]);
        }
        let mut e = gpu.device.create_command_encoder(&Default::default());
        e.resolve_query_set(&queries, 0..FRAMES * 2, &resolve, 0);
        e.copy_buffer_to_buffer(&resolve, 0, &read, 0, u64::from(FRAMES) * 16);
        gpu.queue.submit([e.finish()]);
        let bytes = read_buffer(&gpu, &read);
        let mut ms: Vec<_> = bytes
            .chunks_exact(16)
            .map(|b| {
                let a = u64::from_le_bytes(b[..8].try_into().unwrap());
                let z = u64::from_le_bytes(b[8..].try_into().unwrap());
                (z - a) as f64 * f64::from(gpu.queue.get_timestamp_period()) / 1e6
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        eprintln!(
            "SCENE_GPU model={name} size=1920x1080 scale={scale} median={:.4}ms p95={:.4}ms mean={:.4}ms",
            ms[60],
            ms[114],
            ms.iter().sum::<f64>() / f64::from(FRAMES)
        );
    }
}
