use super::*;

#[test]
fn render_scale_shrinks_targets() {
    assert_eq!(render_size((1920, 1080), 0.5), (960, 540));
    assert_eq!(render_size((1001, 1), 0.25), (251, 1));
    assert_eq!(size_of::<Globals>(), 208);
    assert_eq!(size_of::<load::MaterialUniform>(), 96);
    assert_eq!(size_of::<Vertex>(), 56);
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

fn alpha_fixture(mode: &str, alpha: f32) -> std::path::PathBuf {
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.8,0.2,0.1,$ALPHA],"metallicFactor":0,"roughnessFactor":0.5},"alphaMode":"$MODE","alphaCutoff":0.5}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}],"bufferViews":[{"buffer":0,"byteLength":36}],"buffers":[{"byteLength":36}]}"#;
    let mut js = json
        .replace("$MODE", mode)
        .replace("$ALPHA", &alpha.to_string())
        .into_bytes();
    js.resize(js.len().next_multiple_of(4), b' ');
    let corners = [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
    let bin: Vec<u8> = corners
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    glb.extend((28 + js.len() as u32 + bin.len() as u32).to_le_bytes());
    glb.extend((js.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(js);
    glb.extend((bin.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(bin);
    let dir = std::path::Path::new("C:/glaze-targets/scene3d");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("alpha-{mode}-{alpha}.glb"));
    std::fs::write(&path, glb).unwrap();
    path
}

#[test]
#[ignore = "DX12 stack model composition lab"]
fn lab_model_layer_in_still_stack() {
    let gpu = Gpu::headless().expect("DX12 GPU");
    let size = (1920, 1080);
    let dir = std::path::Path::new("C:/glaze-targets/scene3d/stack-fixture");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::copy(
        root().join("themes/examples/ringed-planet/planet.glb"),
        dir.join("planet.glb"),
    )
    .unwrap();
    std::fs::write(dir.join("theme.json"),r##"{"name":"Scene stack lab","wallpaper":{"kind":"stack","base":{"kind":"color","color":"#16243c"},"layers":[{"id":"planet","kind":"model","source":"planet.glb","rect":[0.18,0.08,0.64,0.84],"scale":0.5,"rotation":[0,25,0],"orbit":0,"parallax":0,"light":[0.4,0.8,0.5],"animation":null}]}}"##).unwrap();
    let theme = LoadedTheme::load(dir).unwrap();
    let mut source = super::super::load(&gpu, &theme, &[]).unwrap();
    assert_eq!(source.redraw(Duration::ZERO), Redraw::OnChange);
    let target = gpu.texture(
        "model stack lab",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let view = target.create_view(&Default::default());
    gpu.checked(|| {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.render(
            &gpu,
            Frame {
                screen: 0,
                size,
                mouse: [960.0, 540.0],
                clock: Duration::ZERO,
                frame: 0,
                view: &view,
                encoder: &mut encoder,
            },
        );
        gpu.queue.submit([encoder.finish()]);
    })
    .unwrap();
    let rgba = pixels(&gpu, &target);
    let background = &rgba[..4];
    assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
    let changed = rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| *p != background)
        .count();
    assert!(changed > 10000, "composited model is visible");
    for y in 0..size.1 {
        for x in 0..size.0 {
            if !(345..1575).contains(&x) || !(86..994).contains(&y) {
                let i = ((y * size.0 + x) * 4) as usize;
                assert_eq!(&rgba[i..i + 4], background, "model stays inside its rect");
            }
        }
    }
    std::fs::write("C:/glaze-targets/scene3d/stack-planet.bgra", rgba).unwrap();
}

#[test]
#[ignore = "DX12 alpha/environment and multi-screen regression lab"]
fn lab_material_alpha_environment_and_view_slots() {
    let gpu = Gpu::headless().expect("DX12 GPU");
    let size = (256, 256);
    let color = gpu.texture(
        "alpha lab color",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let depth = gpu.texture(
        "alpha lab depth",
        size,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let cv = color.create_view(&Default::default());
    let dv = depth.create_view(&Default::default());
    for (mode, alpha, expected) in [
        ("OPAQUE", 0.0, 255),
        ("MASK", 0.3, 0),
        ("MASK", 0.8, 255),
        ("BLEND", 0.4, 102),
    ] {
        let mut scene =
            SceneRenderer::load(&gpu, &alpha_fixture(mode, alpha), &SceneOptions::default())
                .unwrap();
        assert!(!scene.animated());
        gpu.checked(|| {
            let mut e = gpu.device.create_command_encoder(&Default::default());
            scene.render_into(
                &gpu,
                &mut e,
                &cv,
                Some(&dv),
                size,
                Duration::ZERO,
                [128.0; 2],
                true,
            );
            gpu.queue.submit([e.finish()]);
        })
        .unwrap();
        let rgba = pixels(&gpu, &color);
        assert_eq!(
            rgba.chunks_exact(4).map(|p| p[3]).max().unwrap(),
            expected,
            "{mode}/{alpha}"
        );
        assert!(
            rgba.chunks_exact(4)
                .all(|p| p[..3].iter().all(|&c| c <= p[3]))
        );
        let bytes = pixels(&gpu, &depth);
        let has_depth = bytes
            .chunks_exact(4)
            .any(|p| f32::from_le_bytes(p.try_into().unwrap()) < 1.0);
        assert_eq!(
            has_depth,
            expected > 0,
            "visible {mode} contributes linear depth"
        );
    }
    let mut scene = SceneRenderer::load(
        &gpu,
        &alpha_fixture("OPAQUE", 1.0),
        &SceneOptions {
            environment: Environment::Color,
            background: [0.0, 0.25, 0.0],
            exposure: 0.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut e = gpu.device.create_command_encoder(&Default::default());
    scene.render_into(
        &gpu,
        &mut e,
        &cv,
        None,
        size,
        Duration::ZERO,
        [128.0; 2],
        false,
    );
    gpu.queue.submit([e.finish()]);
    let rgba = pixels(&gpu, &color);
    assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
    assert_eq!(&rgba[..4], &[0, 137, 0, 255]);
    assert!(rgba.chunks_exact(4).any(|p| p[..3] == [0, 0, 0]));
    scene.options.environment = Environment::Transparent;
    scene.options.exposure = 1.0;
    scene.options.parallax = 0.3;
    let second = gpu.texture(
        "second scene view",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let sv = second.create_view(&Default::default());
    let mut e = gpu.device.create_command_encoder(&Default::default());
    scene.render_into_for_screen(
        &gpu,
        &mut e,
        &cv,
        None,
        size,
        Duration::from_secs(1),
        [0.0, 128.0],
        false,
        0,
    );
    scene.render_into_for_screen(
        &gpu,
        &mut e,
        &sv,
        None,
        size,
        Duration::from_secs(1),
        [256.0, 128.0],
        false,
        1,
    );
    gpu.queue.submit([e.finish()]);
    let left = pixels(&gpu, &color);
    let right = pixels(&gpu, &second);
    assert_ne!(
        left, right,
        "distinct view uniforms survive a shared submit"
    );
    assert!(left.chunks_exact(4).any(|p| p[3] == 0));
}

/// Offscreen only: never creates a desktop window or changes the current theme.
/// `GLAZE_SCENE_MODEL` selects a repo-relative model; `GLAZE_SCENE_CLIP` selects a clip.
#[test]
#[ignore = "GPU frame lab; writes C:/glaze-targets/scene3d"]
fn lab_transparency_depth_and_cost() {
    let gpu = Gpu::headless().expect("DX12 GPU");
    let size = (1920, 1080);
    let path = root().join(
        std::env::var("GLAZE_SCENE_MODEL")
            .unwrap_or_else(|_| "themes/examples/islands/islands.glb".into()),
    );
    let name = path.file_stem().unwrap().to_string_lossy();
    let output = std::path::Path::new("C:/glaze-targets/scene3d");
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
        animation: std::env::var("GLAZE_SCENE_CLIP").ok(),
        rotation: [
            0.0,
            std::env::var("GLAZE_SCENE_YAW")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            0.0,
        ],
        ..Default::default()
    };
    let mut renderer = SceneRenderer::load(&gpu, &path, &options).unwrap();
    assert_eq!(renderer.animated(), options.animation.is_some());
    renderer.options.orbit = 1.0;
    assert!(renderer.animated());
    renderer.options.orbit = 0.0;
    renderer.options.parallax = 0.1;
    assert!(renderer.animated());
    renderer.options.parallax = 0.0;
    if options.animation.is_none() {
        assert!(!renderer.animated());
    }
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
    // Capture distinct phases rather than accidentally sampling one walk-cycle period.
    let duration = renderer
        .clip
        .map_or(2.8, |i| renderer.data.clips[i].duration);
    for (i, t) in [0.0, duration * 0.25, duration * 0.5]
        .into_iter()
        .enumerate()
    {
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
