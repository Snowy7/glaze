//! glTF import: flattens a file's default scene into one world-space
//! vertex buffer and one index buffer, split into a batch per base color
//! texture (so a scene costs a handful of draw calls, not one per node).
//!
//! Supported: triangle primitives, node hierarchies, positions, normals
//! (flat ones are generated when missing), `TEXCOORD_0`, `COLOR_0`, the base
//! color factor and texture, and emissive (with `KHR_materials_emissive_strength`).
//! Everything else (skins, morph targets, alpha blending, other maps) is ignored.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};

/// Mirrors `struct VsIn` in scene.wgsl. Material factors are baked into
/// `color` and `emissive` at load time, so draws need no per-material state.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Base color factor times `COLOR_0`, linear.
    pub color: [f32; 3],
    /// Emissive factor times emissive strength, linear (may exceed 1).
    pub emissive: [f32; 3],
}

/// A decoded base color texture (sRGB-encoded RGBA8).
pub struct Texture {
    pub size: (u32, u32),
    pub rgba: Vec<u8>,
}

/// A contiguous index range drawn with one texture (`None`: untextured).
pub struct Batch {
    pub texture: Option<usize>,
    pub indices: Range<u32>,
}

pub struct SceneData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<Batch>,
    pub textures: Vec<Texture>,
}

/// Loads a `.glb` / `.gltf` file.
pub fn load(path: &Path) -> Result<SceneData> {
    let (document, buffers, images) =
        gltf::import(path).with_context(|| format!("reading {}", path.display()))?;
    build(&document, &buffers, &images)
}

fn build(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Result<SceneData> {
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .context("glTF file has no scene")?;
    let mut builder = Builder::default();
    for node in scene.nodes() {
        builder.node(&node, Mat4::IDENTITY, buffers)?;
    }
    builder.finish(images)
}

#[derive(Default)]
struct Builder {
    vertices: Vec<Vertex>,
    /// Indices per glTF image index (`None`: untextured).
    buckets: BTreeMap<Option<usize>, Vec<u32>>,
}

impl Builder {
    fn node(
        &mut self,
        node: &gltf::Node,
        parent: Mat4,
        buffers: &[gltf::buffer::Data],
    ) -> Result<()> {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                self.primitive(&primitive, world, buffers)
                    .with_context(|| format!("mesh {:?}", mesh.name().unwrap_or("(unnamed)")))?;
            }
        }
        for child in node.children() {
            self.node(&child, world, buffers)?;
        }
        Ok(())
    }

    fn primitive(
        &mut self,
        primitive: &gltf::Primitive,
        world: Mat4,
        buffers: &[gltf::buffer::Data],
    ) -> Result<()> {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            log::warn!(
                "skipping a {:?} primitive (only triangles are drawn)",
                primitive.mode()
            );
            return Ok(());
        }
        let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
        let Some(positions) = reader.read_positions() else {
            return Ok(());
        };
        let positions: Vec<Vec3> = positions
            .map(|p| world.transform_point3(Vec3::from(p)))
            .collect();
        let mut indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        indices.truncate(indices.len() / 3 * 3);
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            bail!("index out of range");
        }
        // A mirroring transform reverses winding; swap back so front faces stay front.
        if world.determinant() < 0.0 {
            indices
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .for_each(|tri| tri.swap(1, 2));
        }

        let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
        let normals: Option<Vec<Vec3>> = reader.read_normals().map(|normals| {
            normals
                .map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero())
                .collect()
        });
        let uvs: Option<Vec<[f32; 2]>> = reader
            .read_tex_coords(0)
            .map(|uvs| uvs.into_f32().collect());
        let colors: Option<Vec<[f32; 4]>> = reader
            .read_colors(0)
            .map(|colors| colors.into_rgba_f32().collect());

        let material = primitive.material();
        let pbr = material.pbr_metallic_roughness();
        let factor = Vec3::from_slice(&pbr.base_color_factor());
        let emissive =
            Vec3::from(material.emissive_factor()) * material.emissive_strength().unwrap_or(1.0);
        let texture = pbr
            .base_color_texture()
            .filter(|info| info.tex_coord() == 0)
            .map(|info| info.texture().source().index());

        let vertex = |i: usize, normal: Vec3| {
            let vertex_color = colors
                .as_ref()
                .and_then(|c| c.get(i))
                .map_or(Vec3::ONE, |c| Vec3::from_slice(c));
            Vertex {
                position: positions[i].to_array(),
                normal: normal.to_array(),
                uv: uvs
                    .as_ref()
                    .and_then(|u| u.get(i))
                    .copied()
                    .unwrap_or_default(),
                color: (factor * vertex_color).to_array(),
                emissive: emissive.to_array(),
            }
        };

        let base = self.vertices.len() as u32;
        let bucket = self.buckets.entry(texture).or_default();
        if let Some(normals) = &normals {
            self.vertices.extend(
                (0..positions.len()).map(|i| vertex(i, normals.get(i).copied().unwrap_or(Vec3::Y))),
            );
            bucket.extend(indices.iter().map(|i| base + i));
        } else {
            // No normals: un-share vertices and give each triangle its face normal.
            for (t, tri) in indices.as_chunks::<3>().0.iter().enumerate() {
                let [a, b, c] = tri.map(|i| i as usize);
                let normal = (positions[b] - positions[a])
                    .cross(positions[c] - positions[a])
                    .normalize_or_zero();
                self.vertices.extend([a, b, c].map(|i| vertex(i, normal)));
                let first = base + 3 * t as u32;
                bucket.extend([first, first + 1, first + 2]);
            }
        }
        Ok(())
    }

    fn finish(self, images: &[gltf::image::Data]) -> Result<SceneData> {
        if self.vertices.is_empty() {
            bail!("glTF scene has no triangle meshes");
        }
        let mut indices = Vec::new();
        let mut batches = Vec::new();
        let mut textures = Vec::new();
        for (image, bucket) in self.buckets {
            let start = indices.len() as u32;
            indices.extend(bucket);
            let texture = image
                .map(|i| {
                    let data = images.get(i).context("texture image is missing")?;
                    textures.push(decode(data).with_context(|| format!("image {i}"))?);
                    Ok::<_, anyhow::Error>(textures.len() - 1)
                })
                .transpose()?;
            batches.push(Batch {
                texture,
                indices: start..indices.len() as u32,
            });
        }
        Ok(SceneData {
            vertices: self.vertices,
            indices,
            batches,
            textures,
        })
    }
}

/// Expands an 8-bit glTF image to RGBA8.
fn decode(image: &gltf::image::Data) -> Result<Texture> {
    use gltf::image::Format;
    let px = &image.pixels;
    let rgba = match image.format {
        Format::R8G8B8A8 => px.clone(),
        Format::R8G8B8 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        Format::R8G8 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        Format::R8 => px.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        other => bail!("unsupported texture format {other:?} (use 8-bit PNG or JPEG)"),
    };
    Ok(Texture {
        size: (image.width, image.height),
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps JSON and a binary buffer into a GLB container.
    fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
        fn chunk(kind: &[u8; 4], data: &[u8], pad: u8) -> Vec<u8> {
            let mut data = data.to_vec();
            data.resize(data.len().next_multiple_of(4), pad);
            let mut out = (data.len() as u32).to_le_bytes().to_vec();
            out.extend(kind);
            out.extend(data);
            out
        }
        let body = [
            chunk(b"JSON", json.as_bytes(), b' '),
            chunk(b"BIN\0", bin, 0),
        ]
        .concat();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((12 + body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    /// One normal-less triangle (CCW seen from +Z) used by three nodes: under
    /// a translated, scaled hierarchy, and under a mirroring node.
    fn scene() -> SceneData {
        let json = r#"{
          "asset": { "version": "2.0" },
          "extensionsUsed": ["KHR_materials_emissive_strength"],
          "scene": 0, "scenes": [{ "nodes": [0, 2] }],
          "nodes": [
            { "translation": [10, 0, 0], "children": [1] },
            { "scale": [2, 2, 2], "mesh": 0 },
            { "scale": [-1, 1, 1], "mesh": 0 }
          ],
          "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
          "materials": [{
            "pbrMetallicRoughness": { "baseColorFactor": [1, 0.5, 0.25, 1] },
            "emissiveFactor": [0, 1, 0],
            "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 3 } }
          }],
          "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                          "min": [0, 0, 0], "max": [1, 1, 0] }],
          "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
          "buffers": [{ "byteLength": 36 }]
        }"#;
        let corners = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let bin: Vec<u8> = corners
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let (document, buffers, images) = gltf::import_slice(glb(json, &bin)).unwrap();
        build(&document, &buffers, &images).unwrap()
    }

    #[test]
    fn walks_the_hierarchy_and_bakes_materials() {
        let scene = scene();
        // Un-shared vertices: three per triangle, two triangles.
        assert_eq!(scene.vertices.len(), 6);
        let positions: Vec<_> = scene.vertices[..3].iter().map(|v| v.position).collect();
        assert_eq!(
            positions,
            [[10.0, 0.0, 0.0], [12.0, 0.0, 0.0], [10.0, 2.0, 0.0]]
        );
        for v in &scene.vertices {
            assert_eq!(v.color, [1.0, 0.5, 0.25]);
            assert_eq!(v.emissive, [0.0, 3.0, 0.0]);
        }
        assert!(scene.textures.is_empty());
        assert_eq!(scene.batches.len(), 1);
        assert_eq!(scene.batches[0].indices, 0..6);
    }

    #[test]
    fn generates_outward_flat_normals_even_when_mirrored() {
        // The mirrored copy has its winding swapped back, so both face +Z.
        for v in scene().vertices {
            assert_eq!(v.normal, [0.0, 0.0, 1.0]);
        }
    }
}
