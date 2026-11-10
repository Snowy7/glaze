use super::*;

fn tiny_scene() -> SceneData {
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"translation":[10,0,0],"children":[1]},{"scale":[2,2,2],"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[1,0.5,0.25,0.6],"metallicFactor":0.7,"roughnessFactor":0.3},"alphaMode":"MASK","alphaCutoff":0.4,"emissiveFactor":[0,1,0]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"bufferViews":[{"buffer":0,"byteLength":36}],"buffers":[{"byteLength":36}]}"#;
    let mut js = json.as_bytes().to_vec();
    js.resize(js.len().next_multiple_of(4), b' ');
    let corners = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
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
    let (doc, buffers, images) = gltf::import_slice(glb).unwrap();
    build(&doc, &buffers, &images).unwrap()
}

#[test]
fn tiny_glb_preserves_hierarchy_pbr_and_alpha() {
    let data = tiny_scene();
    assert_eq!(data.vertices.len(), 3);
    assert_eq!(data.indices, [0, 1, 2]);
    assert_eq!(
        data.vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
        [[10.0, 0.0, 0.0], [12.0, 0.0, 0.0], [10.0, 2.0, 0.0]]
    );
    assert!(data.vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    let m = &data.materials[0].uniform;
    assert_eq!(m.base, [1.0, 0.5, 0.25, 0.6]);
    assert_eq!(m.surface[..2], [0.7, 0.3]);
    assert_eq!(m.surface[2], 0.0, "absent normal maps must not perturb UV seams");
    assert_eq!(m.alpha[..2], [1.0, 0.4]);
    assert!(data.clips.is_empty());
}

fn sample(path: &str) -> SceneData {
    load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../themes/examples")
            .join(path),
    )
    .unwrap()
}
#[test]
fn named_node_and_morph_clips_change_geometry_without_reallocating() {
    let mut data = sample("ringed-planet/planet.glb");
    let original: Vec<_> = data.vertices.iter().map(|v| v.position).collect();
    let ptr = data.vertices.as_ptr();
    let cap = data.vertices.capacity();
    let turn = data
        .clips
        .iter()
        .position(|c| c.name == "Turntable")
        .unwrap();
    data.animate(turn, 2.0);
    assert!(
        data.vertices
            .iter()
            .zip(&original)
            .any(|(a, b)| Vec3::from(a.position).distance(Vec3::from(*b)) > 0.5)
    );
    let breathe = data.clips.iter().position(|c| c.name == "Breathe").unwrap();
    data.animate(breathe, 2.0);
    assert!(
        data.vertices
            .iter()
            .zip(&original)
            .any(|(a, b)| Vec3::from(a.position).distance(Vec3::from(*b)) > 0.05)
    );
    assert_eq!(data.vertices.as_ptr(), ptr);
    assert_eq!(data.vertices.capacity(), cap);
    data.animate(turn, 8.0);
    assert!(
        data.vertices
            .iter()
            .zip(&original)
            .all(|(a, b)| Vec3::from(a.position).distance(Vec3::from(*b)) < 1e-5)
    );
}
#[test]
fn skeletal_clips_deform_vertices_and_loop() {
    for (path, name) in [
        ("simple-skin/SimpleSkin.gltf", "Animation 1"),
        ("fox-model/Fox.glb", "Walk"),
    ] {
        let mut data = sample(path);
        let clip = data.clips.iter().position(|c| c.name == name).unwrap();
        data.animate(clip, 0.0);
        let first: Vec<_> = data.vertices.iter().map(|v| v.position).collect();
        data.animate(clip, 0.7);
        assert!(
            data.vertices
                .iter()
                .zip(&first)
                .any(|(a, b)| Vec3::from(a.position).distance(Vec3::from(*b)) > 0.01),
            "{path}"
        );
        assert!(
            data.vertices
                .iter()
                .all(|v| Vec3::from(v.position).is_finite() && Vec3::from(v.normal).is_finite())
        );
        data.animate(clip, data.clips[clip].duration);
        assert!(
            data.vertices
                .iter()
                .zip(first)
                .all(|(a, b)| Vec3::from(a.position).distance(Vec3::from(b)) < 1e-4)
        );
    }
}

#[test]
fn step_and_cubic_spline_respect_sampler_interpolation() {
    use gltf::animation::Interpolation;
    let mut data = tiny_scene();
    let channel = Channel {
        node: 1,
        property: Property::Translation,
        times: vec![0.0, 2.0],
        values: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        width: 3,
        interpolation: Interpolation::Step,
    };
    data.clips.push(Clip {
        name: "Step".into(),
        duration: 2.0,
        channels: vec![channel],
    });
    data.animate(0, 1.0);
    assert_eq!(data.vertices[0].position, [10.0, 0.0, 0.0]);
    let c = &mut data.clips[0].channels[0];
    c.interpolation = Interpolation::CubicSpline;
    c.values = vec![
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];
    data.animate(0, 0.5);
    assert!((data.vertices[0].position[0] - 10.3125).abs() < 1e-5);
}
