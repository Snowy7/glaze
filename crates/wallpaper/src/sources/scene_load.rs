//! glTF meshes, PBR materials and reusable node/skin/morph animation storage.
use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::{ops::Range, path::Path};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
pub struct Texture {
    pub size: (u32, u32),
    pub rgba: Vec<u8>,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MaterialUniform {
    pub base: [f32; 4],
    pub emissive: [f32; 4],
    /// metallic, roughness, normal scale, occlusion strength
    pub surface: [f32; 4],
    /// alpha mode (opaque/mask/blend), cutoff, double sided, unused
    pub alpha: [f32; 4],
}
pub struct Material {
    pub uniform: MaterialUniform,
    pub textures: [Option<usize>; 5],
}
pub struct Batch {
    pub material: usize,
    pub indices: Range<u32>,
    pub center: Vec3,
}
#[derive(Clone, Copy)]
struct LocalVertex {
    vertex: Vertex,
    joints: [u16; 4],
    weights: [f32; 4],
}
struct Morph {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}
struct Primitive {
    node: usize,
    vertices: Range<usize>,
    morphs: Vec<Morph>,
}
struct Skin {
    joints: Vec<usize>,
    inverse_bind: Vec<Mat4>,
}
#[derive(Clone)]
struct Pose {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
    weights: Vec<f32>,
}
struct Node {
    base: Pose,
    pose: Pose,
    matrix: Option<Mat4>,
    parent: Option<usize>,
    skin: Option<usize>,
}
#[derive(Clone, Copy)]
enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}
struct Channel {
    node: usize,
    property: Property,
    times: Vec<f32>,
    values: Vec<f32>,
    width: usize,
    interpolation: gltf::animation::Interpolation,
}
pub struct Clip {
    pub name: String,
    pub duration: f32,
    channels: Vec<Channel>,
}
pub struct SceneData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<Batch>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub clips: Vec<Clip>,
    local: Vec<LocalVertex>,
    primitives: Vec<Primitive>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    worlds: Vec<Mat4>,
    skins: Vec<Skin>,
}

pub fn load(path: &Path) -> Result<SceneData> {
    let (doc, buffers, images) =
        gltf::import(path).with_context(|| format!("reading {}", path.display()))?;
    build(&doc, &buffers, &images)
}
fn material(m: gltf::Material<'_>) -> Material {
    let p = m.pbr_metallic_roughness();
    let base = p.base_color_texture();
    let mr = p.metallic_roughness_texture();
    let normal = m.normal_texture();
    let emissive = m.emissive_texture();
    let occlusion = m.occlusion_texture();
    let e = Vec3::from(m.emissive_factor()) * m.emissive_strength().unwrap_or(1.0);
    Material {
        uniform: MaterialUniform {
            base: p.base_color_factor(),
            emissive: e.extend(0.0).to_array(),
            surface: [
                p.metallic_factor(),
                p.roughness_factor(),
                normal.as_ref().map_or(1.0, |t| t.scale()),
                occlusion.as_ref().map_or(1.0, |t| t.strength()),
            ],
            alpha: [
                match m.alpha_mode() {
                    gltf::material::AlphaMode::Opaque => 0.0,
                    gltf::material::AlphaMode::Mask => 1.0,
                    gltf::material::AlphaMode::Blend => 2.0,
                },
                m.alpha_cutoff().unwrap_or(0.5),
                if m.double_sided() { 1.0 } else { 0.0 },
                0.0,
            ],
        },
        textures: [
            base.map(|t| t.texture().source().index()),
            mr.map(|t| t.texture().source().index()),
            normal.map(|t| t.texture().source().index()),
            emissive.map(|t| t.texture().source().index()),
            occlusion.map(|t| t.texture().source().index()),
        ],
    }
}
fn build(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Result<SceneData> {
    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .context("glTF has no scene")?;
    let mut nodes: Vec<_> = doc
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            let weights = n
                .weights()
                .or_else(|| n.mesh().and_then(|m| m.weights()))
                .unwrap_or(&[])
                .to_vec();
            let pose = Pose {
                translation: t.into(),
                rotation: Quat::from_array(r),
                scale: s.into(),
                weights,
            };
            Node {
                base: pose.clone(),
                pose,
                matrix: match n.transform() {
                    gltf::scene::Transform::Matrix { matrix } => {
                        Some(Mat4::from_cols_array_2d(&matrix))
                    }
                    _ => None,
                },
                parent: None,
                skin: n.skin().map(|s| s.index()),
            }
        })
        .collect();
    for n in doc.nodes() {
        for c in n.children() {
            nodes[c.index()].parent = Some(n.index());
        }
    }
    fn visit(n: gltf::Node<'_>, order: &mut Vec<usize>) {
        order.push(n.index());
        for c in n.children() {
            visit(c, order);
        }
    }
    let mut order = Vec::new();
    // Evaluate every root: joints can have ancestors outside the selected scene.
    for n in doc.nodes().filter(|n| nodes[n.index()].parent.is_none()) {
        visit(n, &mut order);
    }
    let skins = doc
        .skins()
        .map(|s| {
            let joints: Vec<_> = s.joints().map(|j| j.index()).collect();
            let inverse_bind = s
                .reader(|b| Some(&buffers[b.index()]))
                .read_inverse_bind_matrices()
                .map(|m| m.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
            Skin {
                joints,
                inverse_bind,
            }
        })
        .collect();
    let mut materials: Vec<_> = doc.materials().map(material).collect();
    let default_material = materials.len();
    materials.push(Material {
        uniform: MaterialUniform {
            base: [1.0; 4],
            emissive: [0.0; 4],
            surface: [1.0, 1.0, 1.0, 1.0],
            alpha: [0.0, 0.5, 0.0, 0.0],
        },
        textures: [None; 5],
    });
    let mut data = SceneData {
        vertices: Vec::new(),
        indices: Vec::new(),
        batches: Vec::new(),
        materials,
        textures: images.iter().map(decode).collect::<Result<_>>()?,
        clips: Vec::new(),
        local: Vec::new(),
        primitives: Vec::new(),
        worlds: vec![Mat4::IDENTITY; nodes.len()],
        nodes,
        order,
        skins,
    };
    fn mesh(
        data: &mut SceneData,
        n: gltf::Node<'_>,
        buffers: &[gltf::buffer::Data],
        default_material: usize,
    ) -> Result<()> {
        if let Some(mesh) = n.mesh() {
            for p in mesh.primitives() {
                if p.mode() != gltf::mesh::Mode::Triangles {
                    log::warn!("scene: skipping non-triangle primitive");
                    continue;
                }
                let r = p.reader(|b| Some(&buffers[b.index()]));
                let positions: Vec<_> = r
                    .read_positions()
                    .context("primitive has no positions")?
                    .collect();
                let mut indices: Vec<_> = r
                    .read_indices()
                    .map(|i| i.into_u32().collect())
                    .unwrap_or_else(|| (0..positions.len() as u32).collect());
                indices.truncate(indices.len() / 3 * 3);
                if indices.iter().any(|&i| i as usize >= positions.len()) {
                    bail!("index out of range");
                }
                let normals = r.read_normals().map(Iterator::collect::<Vec<_>>);
                let uv = r
                    .read_tex_coords(0)
                    .map(|v| v.into_f32().collect::<Vec<_>>());
                let colors = r
                    .read_colors(0)
                    .map(|v| v.into_rgba_f32().collect::<Vec<_>>());
                let joints = r.read_joints(0).map(|v| v.into_u16().collect::<Vec<_>>());
                let weights = r.read_weights(0).map(|v| v.into_f32().collect::<Vec<_>>());
                let morphs: Vec<_> = r
                    .read_morph_targets()
                    .map(|(p, n, _)| Morph {
                        positions: p.map(Iterator::collect).unwrap_or_default(),
                        normals: n.map(Iterator::collect).unwrap_or_default(),
                    })
                    .collect();
                let start = data.local.len();
                let sources: Vec<usize> = if normals.is_none() {
                    indices.iter().map(|&i| i as usize).collect()
                } else {
                    (0..positions.len()).collect()
                };
                let mut flat = Vec3::Y;
                for (j, &i) in sources.iter().enumerate() {
                    if normals.is_none() && j % 3 == 0 {
                        let a = Vec3::from(positions[sources[j]]);
                        let b = Vec3::from(positions[sources[j + 1]]);
                        let c = Vec3::from(positions[sources[j + 2]]);
                        flat = (b - a).cross(c - a).normalize_or_zero();
                    }
                    data.local.push(LocalVertex {
                        vertex: Vertex {
                            position: positions[i],
                            normal: normals
                                .as_ref()
                                .and_then(|v| v.get(i))
                                .copied()
                                .unwrap_or(flat.to_array()),
                            uv: uv
                                .as_ref()
                                .and_then(|v| v.get(i))
                                .copied()
                                .unwrap_or_default(),
                            color: colors
                                .as_ref()
                                .and_then(|v| v.get(i))
                                .copied()
                                .unwrap_or([1.0; 4]),
                        },
                        joints: joints
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .copied()
                            .unwrap_or([0; 4]),
                        weights: weights
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .copied()
                            .unwrap_or([0.0; 4]),
                    });
                }
                let morphs = morphs
                    .into_iter()
                    .map(|m| Morph {
                        positions: if m.positions.is_empty() {
                            Vec::new()
                        } else {
                            sources.iter().map(|&i| m.positions[i]).collect()
                        },
                        normals: if m.normals.is_empty() {
                            Vec::new()
                        } else {
                            sources.iter().map(|&i| m.normals[i]).collect()
                        },
                    })
                    .collect();
                let index_start = data.indices.len() as u32;
                if normals.is_none() {
                    data.indices.extend((start as u32)..data.local.len() as u32);
                } else {
                    data.indices
                        .extend(indices.iter().map(|&i| start as u32 + i));
                }
                data.batches.push(Batch {
                    material: p.material().index().unwrap_or(default_material),
                    indices: index_start..data.indices.len() as u32,
                    center: Vec3::ZERO,
                });
                data.primitives.push(Primitive {
                    node: n.index(),
                    vertices: start..data.local.len(),
                    morphs,
                });
            }
        }
        for c in n.children() {
            mesh(data, c, buffers, default_material)?;
        }
        Ok(())
    }
    for n in scene.nodes() {
        mesh(&mut data, n, buffers, default_material)?;
    }
    if data.local.is_empty() {
        bail!("glTF scene has no triangle meshes");
    }
    data.vertices = data.local.iter().map(|v| v.vertex).collect();
    for a in doc.animations() {
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for c in a.channels() {
            use gltf::animation::util::ReadOutputs;
            let reader = c.reader(|b| Some(&buffers[b.index()]));
            let times: Vec<_> = reader
                .read_inputs()
                .context("animation has no times")?
                .collect();
            if times.windows(2).any(|t| t[0] >= t[1]) || times.iter().any(|t| !t.is_finite()) {
                bail!("invalid animation key times");
            }
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            let (property, width, values) = match reader
                .read_outputs()
                .context("animation has no values")?
            {
                ReadOutputs::Translations(v) => (Property::Translation, 3, v.flatten().collect()),
                ReadOutputs::Scales(v) => (Property::Scale, 3, v.flatten().collect()),
                ReadOutputs::Rotations(v) => {
                    (Property::Rotation, 4, v.into_f32().flatten().collect())
                }
                ReadOutputs::MorphTargetWeights(v) => {
                    let values: Vec<_> = v.into_f32().collect();
                    let keys = times.len()
                        * if c.sampler().interpolation()
                            == gltf::animation::Interpolation::CubicSpline
                        {
                            3
                        } else {
                            1
                        };
                    let width = values.len() / keys.max(1);
                    (Property::Weights, width, values)
                }
            };
            let node = c.target().node().index();
            if matches!(property, Property::Weights) {
                data.nodes[node].base.weights.resize(width, 0.0);
                data.nodes[node].pose.weights.resize(width, 0.0);
            }
            channels.push(Channel {
                node,
                property,
                times,
                values,
                width,
                interpolation: c.sampler().interpolation(),
            });
        }
        data.clips.push(Clip {
            name: a
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Animation {}", a.index() + 1)),
            duration,
            channels,
        });
    }
    data.deform();
    Ok(data)
}

impl SceneData {
    /// Samples one looping clip without allocating pose or vertex arrays.
    pub fn animate(&mut self, clip: usize, time: f32) {
        let clip = &self.clips[clip];
        let t = if clip.duration > 0.0 {
            time.rem_euclid(clip.duration)
        } else {
            0.0
        };
        for n in &mut self.nodes {
            n.pose.translation = n.base.translation;
            n.pose.rotation = n.base.rotation;
            n.pose.scale = n.base.scale;
            n.pose.weights.copy_from_slice(&n.base.weights);
        }
        for c in &clip.channels {
            if c.times.is_empty() {
                continue;
            }
            let hi = c.times.partition_point(|&k| k <= t).min(c.times.len() - 1);
            let lo = hi.saturating_sub(1);
            let dt = c.times[hi] - c.times[lo];
            let u = if dt > 0.0 {
                ((t - c.times[lo]) / dt).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let cubic = c.interpolation == gltf::animation::Interpolation::CubicSpline;
            let value = |component: usize| {
                let at = |key: usize, part: usize| {
                    c.values[(key * if cubic { 3 } else { 1 } + part) * c.width + component]
                };
                let a = at(lo, usize::from(cubic));
                let b = at(hi, usize::from(cubic));
                match c.interpolation {
                    gltf::animation::Interpolation::Step => {
                        if t >= c.times[hi] {
                            b
                        } else {
                            a
                        }
                    }
                    gltf::animation::Interpolation::Linear => a + (b - a) * u,
                    gltf::animation::Interpolation::CubicSpline => {
                        let u2 = u * u;
                        let u3 = u2 * u;
                        (2.0 * u3 - 3.0 * u2 + 1.0) * a
                            + (u3 - 2.0 * u2 + u) * dt * at(lo, 2)
                            + (-2.0 * u3 + 3.0 * u2) * b
                            + (u3 - u2) * dt * at(hi, 0)
                    }
                }
            };
            let pose = &mut self.nodes[c.node].pose;
            match c.property {
                Property::Translation => pose.translation = Vec3::new(value(0), value(1), value(2)),
                Property::Scale => pose.scale = Vec3::new(value(0), value(1), value(2)),
                Property::Rotation => {
                    if c.interpolation == gltf::animation::Interpolation::Linear {
                        let read = |key: usize| {
                            Quat::from_array(std::array::from_fn(|i| c.values[key * 4 + i]))
                        };
                        pose.rotation = read(lo).slerp(read(hi), u).normalize();
                    } else {
                        pose.rotation =
                            Quat::from_xyzw(value(0), value(1), value(2), value(3)).normalize();
                    }
                }
                Property::Weights => {
                    for (i, w) in pose.weights.iter_mut().enumerate() {
                        *w = value(i);
                    }
                }
            }
        }
        self.deform();
    }
    fn deform(&mut self) {
        for &i in &self.order {
            let n = &self.nodes[i];
            let local = n.matrix.unwrap_or_else(|| {
                Mat4::from_scale_rotation_translation(
                    n.pose.scale,
                    n.pose.rotation,
                    n.pose.translation,
                )
            });
            self.worlds[i] = n.parent.map_or(local, |p| self.worlds[p] * local);
        }
        for (p, batch) in self.primitives.iter().zip(&mut self.batches) {
            let node = &self.nodes[p.node];
            let world = self.worlds[p.node];
            let normal = Mat3::from_mat4(world).inverse().transpose();
            let mut center = Vec3::ZERO;
            for (j, i) in p.vertices.clone().enumerate() {
                let local = &self.local[i];
                let mut position = Vec3::from(local.vertex.position);
                let mut n = Vec3::from(local.vertex.normal);
                for (m, &w) in p.morphs.iter().zip(&node.pose.weights) {
                    if let Some(v) = m.positions.get(j) {
                        position += Vec3::from(*v) * w;
                    }
                    if let Some(v) = m.normals.get(j) {
                        n += Vec3::from(*v) * w;
                    }
                }
                let (position, n) = if let Some(skin) = node
                    .skin
                    .filter(|_| local.weights.iter().sum::<f32>() > 0.0)
                {
                    let skin = &self.skins[skin];
                    let mut transform = Mat4::ZERO;
                    let total = local.weights.iter().sum::<f32>();
                    for (&joint, &weight) in local.joints.iter().zip(&local.weights) {
                        if weight == 0.0 {
                            continue;
                        }
                        if let Some((&joint, bind)) = skin
                            .joints
                            .get(joint as usize)
                            .zip(skin.inverse_bind.get(joint as usize))
                        {
                            transform += (self.worlds[joint] * *bind) * (weight / total);
                        }
                    }
                    (
                        transform.transform_point3(position),
                        Mat3::from_mat4(transform).inverse().transpose() * n,
                    )
                } else {
                    (world.transform_point3(position), normal * n)
                };
                let mut v = local.vertex;
                v.position = position.to_array();
                v.normal = n.normalize_or_zero().to_array();
                self.vertices[i] = v;
                center += position;
            }
            batch.center = center / p.vertices.len().max(1) as f32;
        }
    }
}
fn decode(image: &gltf::image::Data) -> Result<Texture> {
    use gltf::image::Format;
    let px = &image.pixels;
    let rgba = match image.format {
        Format::R8G8B8A8 => px.clone(),
        Format::R8G8B8 => px
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        Format::R8G8 => px
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        Format::R8 => px.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        other => bail!("unsupported texture format {other:?}; use 8-bit PNG/JPEG"),
    };
    Ok(Texture {
        size: (image.width, image.height),
        rgba,
    })
}
