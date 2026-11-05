// Vendored from liquid-rust (https://github.com/al1h3n/liquid-rust, commit 2073cd9) by al1h3n,
// MIT OR Apache-2.0 (see LICENSE-MIT, LICENSE-APACHE). Local changes are marked "mypaper:".
//! wgpu renderer: packs a [`Scene`] into GPU buffers and draws it over your content.
//!
//! Per frame:
//! 1. copy `content` into `target`;
//! 2. build (or reuse) a blur pyramid of the content, only inside the region glass
//!    can sample — Apple samples its backdrop at quarter resolution, we go further;
//! 3. draw every glass group as one instanced quad, z-layer by z-layer;
//! 4. when a higher layer overlaps lower glass, re-render the lower glass into an
//!    offscreen backdrop for just that region and blur it into a second pyramid, so
//!    glass on glass refracts what is really below it. Non-overlapping layers cost
//!    nothing. With [`Scene::glass_on_glass`] off, every layer samples the content instead.
//!
//! Only pixels are cached, never motion: the blur of content that holds still is kept
//! from frame to frame, while every spring is solved and every shape drawn fresh, so
//! glass always refracts what is behind it right now.

use std::ops::{Bound, Range, RangeBounds};

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::scene::{Geometry, GlassId, Scene};

/// Inputs for one frame.
pub struct Frame<'a> {
    /// What is behind the glass. Needs `TEXTURE_BINDING`; same size as `target`.
    pub content: &'a wgpu::TextureView,
    /// Where content + glass are drawn. Must use the renderer's format. Must not be
    /// the same texture as `content`.
    pub target: &'a wgpu::TextureView,
    /// Size of both textures in pixels.
    pub size: (u32, u32),
    /// Pixels per point (device pixel ratio).
    pub scale: f32,
    /// Set when `content` differs from the previous frame. While it stays `false` the
    /// blur pyramid is reused, wherever the glass moves (zero blur cost over static
    /// wallpapers). `true` is always correct, only slower.
    pub content_changed: bool,
    /// mypaper: `true` (upstream) copies `content` into `target` first. `false` draws the glass
    /// over whatever `target` already holds: the wallpaper the engine restored, or a transparent
    /// clear for a premultiplied overlay. `content` may then be a different size than `target`.
    pub copy_content: bool,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 4],
    misc: [f32; 4],
    light: [f32; 4],
    touch: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct ShapeGpu {
    center_half: [f32; 4],
    params: [f32; 4],
    stretch: [f32; 4],
    extra: [f32; 4],
}

/// One instanced quad: a cluster of shapes drawn as one smooth union.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct GroupGpu {
    bounds: [f32; 4],
    /// First shape, shape count, 1 when the shapes' materials differ (blend them), unused.
    info: [u32; 4],
    /// Union radius, presence, press, unused.
    geo: [f32; 4],
}

/// One shape's material, resolved for its own size. Shapes in a group fuse their materials
/// across the liquid neck with the same weights as their outlines, so a tinted button keeps
/// its tint when it touches a plain one.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct MaterialGpu {
    optics: [f32; 4],
    lighting: [f32; 4],
    edge: [f32; 4],
    shadow: [f32; 4],
    light_a: [f32; 4],
    light_b: [f32; 4],
    dark_a: [f32; 4],
    dark_b: [f32; 4],
    tint: [f32; 4],
}

type Bounds = [f32; 4];

struct Layer {
    groups: Range<u32>,
    bounds: Bounds,
    z: i32,
}

#[derive(Default)]
struct Packed {
    shapes: Vec<ShapeGpu>,
    /// One per shape, same index: each shape's own material, resolved for its own size. The
    /// shader blends them per pixel as the shapes fuse, so fused glass keeps its own look
    /// (tint, thickness, frost) and nothing jumps when a group splits.
    materials: Vec<MaterialGpu>,
    groups: Vec<GroupGpu>,
    layers: Vec<Layer>,
    /// Farthest any group samples beyond its bounds (refraction + blur), px.
    reach: f32,
    /// mypaper: one per group, same index: what it reads from the content pyramid.
    reads: Vec<Reads>,
}

/// mypaper: what one glass group reads from the content's blur pyramid, so a rebuild only
/// redraws the texels glass will read (see [`Pyramid::build_reads`]).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reads {
    /// Where its frost samples: the group's bounds, px. The renderer adds its reach.
    bounds: Bounds,
    /// Its members' largest frost sigma (px, presence included): the highest level frost reads.
    sigma: f32,
    /// Bounding box of its members' centres, px, where the shader reads the backdrop's
    /// statistics (shadow adaptivity, and the light face of adaptive glass).
    centers: Bounds,
    /// Whether its face flips with the backdrop's brightness (it then reads the top level).
    adaptive: bool,
}

struct Member {
    id: GlassId,
    geo: Geometry,
    z: i32,
    container: u64,
    order: u64,
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn expand(b: Bounds, by: f32) -> Bounds {
    [b[0] - by, b[1] - by, b[2] + by, b[3] + by]
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Stacking orders to draw, as [`Renderer::render_layers`] takes them.
type Layers = (Bound<i32>, Bound<i32>);

/// Splits live glass on `layers` into render groups: one per container cluster whose
/// members are within `spacing` of each other, one per loose glass.
fn cluster(scene: &Scene, layers: Layers) -> Vec<Vec<Member>> {
    let mut members: Vec<Member> = scene
        .nodes
        .iter()
        .filter(|(_, n)| !scene.material_of(n).is_identity() && n.presence.value > 1e-3 && layers.contains(&scene.z_of(n)))
        .map(|(id, n)| Member {
            id,
            geo: scene.geometry(n),
            z: scene.z_of(n),
            container: n.glass.container.map_or(0, |c| slotmap::Key::data(&c).as_ffi()),
            order: n.order,
        })
        .collect();
    members.sort_by_key(|m| (m.z, m.container, m.order));

    let mut parent: Vec<usize> = (0..members.len()).collect();
    // ponytail: O(n^2) per container; fine for UI-sized clusters (tens of shapes).
    for (i, a) in members.iter().enumerate() {
        let Some(spacing) = scene.nodes[a.id].glass.container.and_then(|c| scene.containers.get(c)).map(|c| c.spacing) else {
            continue;
        };
        for (j, b) in members.iter().enumerate().skip(i + 1) {
            if b.container != a.container {
                break;
            }
            let gap = (a.geo.center - b.geo.center).abs() - (a.geo.reach() + b.geo.reach());
            if gap.max_element() < spacing {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }

    let mut groups: Vec<Vec<Member>> = Vec::new();
    let mut slot = vec![usize::MAX; members.len()];
    for (i, m) in members.into_iter().enumerate() {
        let root = find(&mut parent, i);
        if slot[root] == usize::MAX {
            slot[root] = groups.len();
            groups.push(Vec::new());
        }
        groups[slot[root]].push(m);
    }
    // Within a layer a group draws by its newest member: glass that just grew out of a group
    // (a menu out of its button) covers older glass it overlaps, not the other way round.
    groups.sort_by_key(|g| (g[0].z, g.iter().map(|m| m.order).max().unwrap_or(0)));
    groups
}

fn pack(scene: &Scene, layers: Layers, scale: f32, viewport: Vec2, out: &mut Packed) {
    out.shapes.clear();
    out.materials.clear();
    out.groups.clear();
    out.layers.clear();
    out.reach = 0.0;
    out.reads.clear();
    let s = scale;
    let screen = [0.0, 0.0, viewport.x, viewport.y];

    for members in cluster(scene, layers) {
        let lead = &scene.nodes[members[0].id];
        let spacing = lead.glass.container.and_then(|c| scene.containers.get(c)).map_or(0.0, |c| c.spacing);
        let presence = |m: &Member| scene.nodes[m.id].presence.value.clamp(0.0, 1.0);
        let p = members.iter().map(presence).fold(0.0, f32::max);
        let press = members.iter().map(|m| scene.nodes[m.id].press.value).fold(0.0, f32::max).clamp(0.0, 1.5);

        let first = out.shapes.len() as u32;
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut reads = Reads { bounds, sigma: 0.0, centers: bounds, adaptive: false };
        // How far the drop shadow reaches past the shapes, px.
        let mut margin: f32 = 2.0 * s;
        for member in &members {
            let g = member.geo;
            let pm = presence(member);
            let others = members.iter().any(|o| o.id != member.id && presence(o) > 0.5);
            let shrink = (1.0 - pm) * g.half.min_element() * if others { 1.0 } else { 0.15 };
            let along = 1.0 + g.stretch;
            // Each shape's own material at its own size: a 52 pt bar fused to a 400 pt
            // panel stays thin glass, a tinted button next to a plain one stays tinted.
            let node = &scene.nodes[member.id];
            // mypaper: each glass may bring its own appearance.
            let appearance = node.glass.appearance.as_ref().unwrap_or(&scene.appearance);
            let resolved = scene.material_of(node).resolve(g.half.min_element() * 2.0, appearance);
            let m = resolved.material;
            let depth = (m.depth * s).min(g.half.min_element() * s).max(1.0);
            out.shapes.push(ShapeGpu {
                center_half: [g.center.x * s, g.center.y * s, g.half.x * s, g.half.y * s],
                params: [g.radius * s, shrink * s, g.axis.x, g.axis.y],
                stretch: [along, 1.0 / along, g.exponent, 0.0],
                extra: [depth, 0.0, 0.0, 0.0],
            });
            let material = material_gpu(&resolved, p, press, s, appearance.dark);
            margin = margin.max(m.shadow.offset_y.abs() * s + 1.5 * m.shadow.blur * s);
            out.reach = out.reach.max(depth * 1.2 * m.refraction + 8.0 * m.frost * 0.5 * s * p + 8.0);
            let c = g.center * s;
            let r = g.reach() * s;
            bounds = union(bounds, [c.x - r.x, c.y - r.y, c.x + r.x, c.y + r.y]);
            reads.sigma = reads.sigma.max(material.optics[3]);
            reads.centers = union(reads.centers, [c.x, c.y, c.x, c.y]);
            reads.adaptive |= material.optics[1] > 0.0;
            out.materials.push(material);
        }

        let k = spacing * s;
        let bounds = clamp_bounds(expand(bounds, margin + k * 0.25 + 1.0), screen);
        out.reads.push(Reads { bounds, ..reads });
        // Members with the same material and size (a row of equal buttons) skip blending.
        let blend = out.materials[first as usize..].windows(2).any(|w| w[0] != w[1]);
        out.groups.push(GroupGpu { bounds, info: [first, members.len() as u32, u32::from(blend), 0], geo: [k, p, press, 0.0] });

        let index = out.groups.len() as u32 - 1;
        let z = members[0].z;
        match out.layers.last_mut() {
            Some(layer) if layer.z == z => {
                layer.groups.end = index + 1;
                layer.bounds = union(layer.bounds, bounds);
            }
            _ => out.layers.push(Layer { groups: index..index + 1, bounds, z }),
        }
    }
}

/// GPU form of a resolved material. Strengths scale with the group's `presence`, so glass
/// materializes by ramping its lensing in; the specular brightens with `press`.
fn material_gpu(resolved: &crate::material::Resolved, presence: f32, press: f32, scale: f32, dark: bool) -> MaterialGpu {
    let (m, p, s) = (resolved.material, presence, scale);
    let tone_a = |t: crate::Tone| [t.lift * p, t.luminance, t.luminance_mix * p, t.lighten * p];
    let tone_b = |t: crate::Tone| [t.darken_to, t.darken * p, 0.0, 0.0];
    let tint = m.tint.map_or([0.0; 4], |c| {
        let lab = c.to_oklab();
        // Stained glass, never a solid fill: full alpha keeps 15 % of the backdrop.
        [lab[0], lab[1], lab[2], c.a * 0.85 * p]
    });
    MaterialGpu {
        optics: [m.refraction * p, resolved.adapt, m.dispersion * p, m.frost * 0.5 * s * p],
        lighting: [m.splay, m.specular * p * (1.0 + 0.6 * press), m.rim * p, m.rim_shade * p],
        edge: [m.hairline * p, m.edge * p, resolved.contrast * p, m.dim * p],
        // mypaper: the scheme rides in the unused slot (1 light, 0 dark), per glass.
        shadow: [m.shadow.offset_y * s, m.shadow.blur * 0.5 * s, m.shadow.opacity * p, if dark { 0.0 } else { 1.0 }],
        light_a: tone_a(m.light_tone),
        light_b: tone_b(m.light_tone),
        dark_a: tone_a(m.dark_tone),
        dark_b: tone_b(m.dark_tone),
        tint,
    }
}

fn clamp_bounds(b: Bounds, screen: Bounds) -> Bounds {
    [b[0].max(screen[0]), b[1].max(screen[1]), b[2].min(screen[2]), b[3].min(screen[3])]
}

fn scissor(pass: &mut wgpu::RenderPass<'_>, b: Bounds, size: (u32, u32)) -> bool {
    let x0 = b[0].floor().max(0.0) as u32;
    let y0 = b[1].floor().max(0.0) as u32;
    let x1 = (b[2].ceil().max(0.0) as u32).min(size.0);
    let y1 = (b[3].ceil().max(0.0) as u32).min(size.1);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
    true
}

// mypaper: `Clone`, so [`Renderer::fork`] can share compiled pipelines.
#[derive(Clone)]
struct Pipelines {
    glass: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    down_first: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

struct Pyramid {
    size: (u32, u32),
    levels: u32,
    full: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    /// `chain[i]` reads mip `i` to produce mip `i + 1`.
    chain: Vec<wgpu::BindGroup>,
}

const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MAX_LEVELS: u32 = 7;
/// mypaper: `glass.wgsl`'s PYRAMID_SIGMA0, the blur sigma (px) of pyramid level 1.
const PYRAMID_SIGMA0: f32 = 1.9;
/// mypaper: rects a level's cache may hold before still content is blurred whole instead.
const CACHE_RECTS: usize = 16;

/// mypaper: rects of every pyramid level (`rects[level]`), in that level's texels.
type LevelRects = Vec<Vec<Bounds>>;

fn area(r: &Bounds) -> f32 {
    (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0)
}

/// mypaper: drops empty rects and merges any two whose bounding box costs no more texels than
/// the two drawn apart, so overlapping glass doesn't downsample the same texels twice.
fn merge(rects: &mut Vec<Bounds>) {
    rects.retain(|r| area(r) > 0.0);
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let u = union(rects[i], rects[j]);
                if area(&u) <= area(&rects[i]) + area(&rects[j]) {
                    rects[i] = u;
                    rects.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
}

/// mypaper: a pyramid's shape: the target size it blurs and its level count.
#[derive(Clone, Copy)]
struct Layout {
    size: (u32, u32),
    levels: u32,
}

impl Layout {
    fn mip_size(&self, level: usize) -> (u32, u32) {
        ((self.size.0.div_ceil(2) >> level).max(1), (self.size.1.div_ceil(2) >> level).max(1))
    }

    /// mypaper: the texels of each level that glass making `reads` samples (frost sampling up
    /// to `reach` px past a group's bounds), each with the halo the levels above need to
    /// compute theirs exactly, merged per level. Levels nothing reads are left out.
    fn rects(&self, reads: &[Reads], reach: f32) -> LevelRects {
        let top = self.levels as f32;
        let mut out: LevelRects = vec![Vec::new(); self.levels as usize];
        for r in reads {
            // The highest level `frost()` reads for this sigma (it also reads the one below).
            let level = (r.sigma.max(1e-4) / PYRAMID_SIGMA0).log2() + 1.0;
            if level > 0.0 {
                let k = if level < 1.0 { 0.0 } else { level.min(top).floor().min(top - 1.0) };
                self.add(&mut out, expand(r.bounds, reach), k as usize);
            }
            // fs_glass's backdrop statistics at the centres: shadow adaptivity, and the face of
            // glass that flips with the content (whose top level is otherwise never read).
            self.add(&mut out, r.centers, (top - 4.0).max(0.0) as usize);
            if r.adaptive {
                self.add(&mut out, r.centers, (top - 1.0).max(0.0) as usize);
            }
        }
        for level in &mut out {
            merge(level);
        }
        out
    }

    /// mypaper: adds `area` (level-0 px) at levels `0..=top`. A B-spline frost read or a
    /// bilinear tap reaches under 3 texels around its sample, and a downsampled texel reads
    /// its source 2 texels out (bilinear: 3) at the levels' size ratio.
    fn add(&self, out: &mut LevelRects, area: Bounds, top: usize) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let mut halo = [3.0f32, 3.0];
        for k in (0..=top).rev() {
            let (mw, mh) = self.mip_size(k);
            let (sx, sy) = (mw as f32 / w, mh as f32 / h);
            let rect = [area[0] * sx - halo[0], area[1] * sy - halo[1], area[2] * sx + halo[0], area[3] * sy + halo[1]];
            out[k].push(clamp_bounds(rect, [0.0, 0.0, mw as f32, mh as f32]));
            if k > 0 {
                let (pw, ph) = self.mip_size(k - 1);
                halo = [halo[0] * pw as f32 / mw as f32 + 3.0, halo[1] * ph as f32 / mh as f32 + 3.0];
            }
        }
    }

    /// mypaper: every texel of every level.
    fn whole(&self) -> LevelRects {
        (0..self.levels as usize)
            .map(|k| {
                let (w, h) = self.mip_size(k);
                vec![[0.0, 0.0, w as f32, h as f32]]
            })
            .collect()
    }
}

impl Pyramid {
    fn new(device: &wgpu::Device, pipelines: &Pipelines, size: (u32, u32)) -> Self {
        let min_side = size.0.min(size.1).max(2);
        let levels = (min_side.ilog2().saturating_sub(3)).clamp(1, MAX_LEVELS);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("liquid-rust pyramid"),
            size: wgpu::Extent3d { width: size.0.div_ceil(2), height: size.1.div_ceil(2), depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mips: Vec<_> = (0..levels).map(|i| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: i, mip_level_count: Some(1), ..Default::default() })).collect();
        let chain = mips[..mips.len() - 1].iter().map(|view| down_bind_group(device, &pipelines.down, &pipelines.sampler, view)).collect();
        Self { size, levels, full: texture.create_view(&wgpu::TextureViewDescriptor::default()), mips, chain }
    }

    /// mypaper: its shape, which is all the rect maths needs.
    fn layout(&self) -> Layout {
        Layout { size: self.size, levels: self.levels }
    }

    /// mypaper: [`Pyramid::build`] into `rects` of each level only.
    fn build_rects(&self, pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, first: &wgpu::BindGroup, rects: &LevelRects, profiler: &Option<Profiler>) {
        const LEVELS: [&str; MAX_LEVELS as usize] = ["pyramid 0", "pyramid 1", "pyramid 2", "pyramid 3", "pyramid 4", "pyramid 5", "pyramid 6"];
        for (i, mip) in self.mips.iter().enumerate() {
            if rects[i].is_empty() {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("liquid-rust downsample"),
                color_attachments: &[Some(attachment(mip))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if i == 0 {
                pass.set_pipeline(&pipelines.down_first);
                pass.set_bind_group(0, first, &[]);
            } else {
                pass.set_pipeline(&pipelines.down);
                pass.set_bind_group(0, &self.chain[i - 1], &[]);
            }
            for rect in &rects[i] {
                if scissor(&mut pass, *rect, self.layout().mip_size(i)) {
                    pass.draw(0..3, 0..1);
                }
            }
            drop(pass);
            stamp(profiler, encoder, LEVELS[i]);
        }
    }

    /// Downsamples the texture `first` reads (see [`down_bind_group`]) into every level, only
    /// inside `region` (px of level 0).
    fn build(&self, pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, first: &wgpu::BindGroup, region: Bounds, profiler: &Option<Profiler>) {
        const LEVELS: [&str; MAX_LEVELS as usize] = ["pyramid 0", "pyramid 1", "pyramid 2", "pyramid 3", "pyramid 4", "pyramid 5", "pyramid 6"];
        for (i, mip) in self.mips.iter().enumerate() {
            if i > 0 {
                stamp(profiler, encoder, LEVELS[i - 1]);
            }
            let f = (1u32 << (i + 1)) as f32;
            let mip_size = ((self.size.0.div_ceil(2) >> i).max(1), (self.size.1.div_ceil(2) >> i).max(1));
            let b = expand([region[0] / f, region[1] / f, region[2] / f, region[3] / f], 2.0);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("liquid-rust downsample"),
                color_attachments: &[Some(attachment(mip))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !scissor(&mut pass, b, mip_size) {
                continue;
            }
            if i == 0 {
                pass.set_pipeline(&pipelines.down_first);
                pass.set_bind_group(0, first, &[]);
            } else {
                pass.set_pipeline(&pipelines.down);
                pass.set_bind_group(0, &self.chain[i - 1], &[]);
            }
            pass.draw(0..3, 0..1);
        }
        stamp(profiler, encoder, LEVELS[self.mips.len() - 1]);
    }
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
    }
}

fn down_bind_group(device: &wgpu::Device, pipeline: &wgpu::RenderPipeline, sampler: &wgpu::Sampler, src: &wgpu::TextureView) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid-rust downsample"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(sampler) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src) }],
    })
}

fn blit_bind_group(device: &wgpu::Device, pipelines: &Pipelines, src: &wgpu::TextureView) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid-rust blit"),
        layout: &pipelines.blit.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src) }],
    })
}

/// Copies the texture `group` reads (see [`blit_bind_group`]) into `dst`.
fn blit(pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, group: &wgpu::BindGroup, dst: &wgpu::TextureView, region: Option<Bounds>, size: (u32, u32)) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("liquid-rust blit"),
        color_attachments: &[Some(attachment(dst))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(region) = region
        && !scissor(&mut pass, region, size)
    {
        return;
    }
    pass.set_pipeline(&pipelines.blit);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

/// mypaper: GPU timestamps between the renderer's passes, for the frame lab's per-pass profile.
/// Needs `TIMESTAMP_QUERY_INSIDE_ENCODERS`. Timestamp 0 opens a frame; timestamp `i + 1` closes
/// the pass named `labels()[i]`.
pub struct Profiler {
    queries: wgpu::QuerySet,
    labels: std::cell::RefCell<Vec<&'static str>>,
}

impl Profiler {
    /// Profiles into `queries` (a timestamp set; passes past its count are not timed).
    pub fn new(queries: wgpu::QuerySet) -> Self {
        Self { queries, labels: std::cell::RefCell::new(Vec::new()) }
    }

    /// The query set, to resolve `0..=labels().len()`.
    pub fn queries(&self) -> &wgpu::QuerySet {
        &self.queries
    }

    /// The passes timed in the last frame, in order.
    pub fn labels(&self) -> Vec<&'static str> {
        self.labels.borrow().clone()
    }

    fn open(&self, encoder: &mut wgpu::CommandEncoder) {
        self.labels.borrow_mut().clear();
        encoder.write_timestamp(&self.queries, 0);
    }

    fn stamp(&self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        let mut labels = self.labels.borrow_mut();
        if (labels.len() as u32) + 1 < self.queries.count() {
            labels.push(label);
            encoder.write_timestamp(&self.queries, labels.len() as u32);
        }
    }
}

fn stamp(profiler: &Option<Profiler>, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
    if let Some(p) = profiler {
        p.stamp(encoder, label);
    }
}

/// Bind groups kept across frames, keyed by what they read. Making one is a driver call (and,
/// on the web, a JavaScript object for the garbage collector), and its inputs (the content
/// view, the two layer textures) rarely change between frames, so building them afresh every
/// frame was pure overhead. A few entries cover every case; the oldest goes first, so a host
/// that hands in a new view each frame keeps at most [`BindCache::CAPACITY`] alive.
struct BindCache<K> {
    entries: Vec<(K, wgpu::BindGroup)>,
}

impl<K: PartialEq> BindCache<K> {
    const CAPACITY: usize = 4;

    const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// The bind group for `key`, made by `make` the first time.
    fn get(&mut self, key: K, make: impl FnOnce(&K) -> wgpu::BindGroup) -> wgpu::BindGroup {
        if let Some((_, group)) = self.entries.iter().find(|(k, _)| *k == key) {
            return group.clone();
        }
        if self.entries.len() >= Self::CAPACITY {
            self.entries.remove(0);
        }
        let group = make(&key);
        self.entries.push((key, group.clone()));
        group
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Every [`BindCache`] of a renderer.
struct Binds {
    /// Reads a texture for [`blit`].
    blit: BindCache<wgpu::TextureView>,
    /// Reads a texture into the first pyramid level.
    down_first: BindCache<wgpu::TextureView>,
    /// Glass pass: the backdrop it refracts, and whether it blurs from the layer pyramid.
    /// Holds the storage buffers too, so it is cleared whenever they are reallocated.
    glass: BindCache<(wgpu::TextureView, bool)>,
}

/// Draws [`Scene`]s with Liquid Glass. Create one per target format and reuse it.
pub struct Renderer {
    format: wgpu::TextureFormat,
    pipelines: Pipelines,
    globals: wgpu::Buffer,
    shapes: wgpu::Buffer,
    materials: wgpu::Buffer,
    groups: wgpu::Buffer,
    /// Blur of the content alone.
    pyramid: Option<Pyramid>,
    /// Blur of a layer backdrop (content plus the glass below it). Made, with
    /// `layer_textures`, the first time glass overlaps glass.
    layered: Option<Pyramid>,
    layer_textures: Vec<wgpu::TextureView>,
    /// mypaper: the texels of each `pyramid` level that hold the current content's blur
    /// (upstream: one region for all levels).
    cached: Option<LevelRects>,
    packed: Packed,
    /// What the GPU buffers hold now, so a frame where nothing moved uploads nothing.
    uploaded: Uploaded,
    binds: Binds,
    /// Per layer: whether it needs a backdrop that includes the glass below it. Kept to reuse
    /// its allocation.
    rebuild: Vec<bool>,
    /// mypaper: per-pass GPU timestamps, when the frame lab asks.
    pub profiler: Option<Profiler>,
}

/// Last contents written to the renderer's buffers.
#[derive(Default)]
struct Uploaded {
    globals: Option<Globals>,
    shapes: Vec<ShapeGpu>,
    materials: Vec<MaterialGpu>,
    groups: Vec<GroupGpu>,
}

fn storage(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: None,
        vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Renderer {
    /// Creates pipelines for drawing into textures of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let glass = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid-rust glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glass.wgsl").into()),
        });
        let pyramid = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid-rust pyramid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/pyramid.wgsl").into()),
        });
        let strip = wgpu::PrimitiveTopology::TriangleStrip;
        let list = wgpu::PrimitiveTopology::TriangleList;
        let pipelines = Pipelines {
            glass: pipeline(device, &glass, "vs_glass", "fs_glass", format, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), strip),
            blit: pipeline(device, &pyramid, "vs_full", "fs_blit", format, None, list),
            down_first: pipeline(device, &pyramid, "vs_full", "fs_down_first", PYRAMID_FORMAT, None, list),
            down: pipeline(device, &pyramid, "vs_full", "fs_down", PYRAMID_FORMAT, None, list),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("liquid-rust linear clamp"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        };
        Self::with_pipelines(device, format, pipelines)
    }

    /// mypaper: [`Renderer::new`]'s body after the pipelines, shared with [`Renderer::fork`].
    fn with_pipelines(device: &wgpu::Device, format: wgpu::TextureFormat, pipelines: Pipelines) -> Self {
        Self {
            format,
            globals: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquid-rust globals"),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            shapes: storage(device, "liquid-rust shapes", size_of::<ShapeGpu>() as u64 * 16),
            materials: storage(device, "liquid-rust materials", size_of::<MaterialGpu>() as u64 * 16),
            groups: storage(device, "liquid-rust groups", size_of::<GroupGpu>() as u64 * 16),
            pipelines,
            pyramid: None,
            layered: None,
            layer_textures: Vec::new(),
            cached: None,
            packed: Packed::default(),
            uploaded: Uploaded::default(),
            binds: Binds { blit: BindCache::new(), down_first: BindCache::new(), glass: BindCache::new() },
            rebuild: Vec::new(),
            profiler: None,
        }
    }

    /// mypaper: a new renderer (own buffers, blur pyramid and caches) that shares this one's
    /// compiled pipelines, so a short-lived one (an overlay) costs no shader compile.
    pub fn fork(&self, device: &wgpu::Device) -> Self {
        Self::with_pipelines(device, self.format, self.pipelines.clone())
    }

    /// Texture format this renderer draws into.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Records commands that draw `frame.content` plus all glass of `scene` into
    /// `frame.target`. Does nothing for an empty (`0 x 0`) frame, such as a minimized window.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame<'_>, scene: &Scene) {
        self.render_layers(device, queue, encoder, frame, scene, ..);
    }

    /// Like [`Renderer::render`], but draws only the glass whose stacking order
    /// ([`Glass::z`](crate::Glass::z), or its container's) is in `layers`.
    ///
    /// This is how glass refracts the labels of the glass beneath it. Labels are drawn on
    /// top after `render`, so in one pass nothing can bend them. Split the scene instead:
    /// draw the lower layers into a texture, draw their labels onto it, then hand that
    /// texture to a second renderer as the `content` of the upper layers.
    ///
    /// ```no_run
    /// # fn demo(device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, scene: &liquid_rust::Scene,
    /// #     lower: &mut liquid_rust::Renderer, upper: &mut liquid_rust::Renderer,
    /// #     content: &wgpu::TextureView, middle: &wgpu::TextureView, target: &wgpu::TextureView) {
    /// # use liquid_rust::Frame;
    /// # let (size, scale) = ((1206, 2622), 3.0);
    /// lower.render_layers(device, queue, encoder, &Frame { content, target: middle, size, scale, content_changed: false, copy_content: true }, scene, ..2);
    /// // ... draw the labels of layers 0 and 1 into `middle` ...
    /// upper.render_layers(device, queue, encoder, &Frame { content: middle, target, size, scale, content_changed: true, copy_content: true }, scene, 2..);
    /// # }
    /// ```
    ///
    /// Use one renderer per pass: each keeps the blur of its own content.
    pub fn render_layers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame<'_>, scene: &Scene, layers: impl RangeBounds<i32>) {
        let size = frame.size;
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let layers = (layers.start_bound().cloned(), layers.end_bound().cloned());
        if let Some(p) = &self.profiler {
            p.open(encoder);
        }
        pack(scene, layers, frame.scale, Vec2::new(size.0 as f32, size.1 as f32), &mut self.packed);
        if self.pyramid.as_ref().is_none_or(|p| p.size != size) {
            self.pyramid = Some(Pyramid::new(device, &self.pipelines, size));
            self.layered = None;
            self.layer_textures.clear();
            self.cached = None;
            // Old layer textures and pyramids: their bind groups would keep them alive.
            self.binds.blit.clear();
            self.binds.down_first.clear();
            self.binds.glass.clear();
        }
        self.upload(device, queue, frame, scene);
        let pipelines = &self.pipelines;
        if frame.copy_content {
            let copy = self.binds.blit.get(frame.content.clone(), |src| blit_bind_group(device, pipelines, src));
            blit(pipelines, encoder, &copy, frame.target, None, size);
            stamp(&self.profiler, encoder, "copy content");
        }
        if self.packed.layers.is_empty() {
            return;
        }

        // Which layers need a backdrop that includes the glass below them.
        let screen = [0.0, 0.0, size.0 as f32, size.1 as f32];
        let reach = self.packed.reach;
        let layers = &self.packed.layers;
        let tail = |i: usize| clamp_bounds(expand(layers[i..].iter().fold(layers[i].bounds, |a, l| union(a, l.bounds)), reach), screen);
        let rebuild = &mut self.rebuild;
        rebuild.clear();
        rebuild.resize(layers.len(), false);
        let mut below = layers[0].bounds;
        for (i, layer) in layers.iter().enumerate().skip(1) {
            rebuild[i] = scene.glass_on_glass && intersects(expand(layer.bounds, reach), below);
            below = union(below, layer.bounds);
        }
        if rebuild.contains(&true) && self.layer_textures.is_empty() {
            self.layer_textures = (0..2).map(|_| layer_texture(device, self.format, size)).collect();
            self.layered = Some(Pyramid::new(device, &self.pipelines, size));
        }

        let Some(pyramid) = &self.pyramid else { return };
        let pipelines = &self.pipelines;
        let binds = &mut self.binds;
        let profiler = &self.profiler;
        // mypaper: rebuild only the texels each group reads (upstream: the bounding box of all
        // of them, here nearly the whole screen), and keep what still content already has.
        let needed = pyramid.layout().rects(&self.packed.reads, reach);
        let build = match &mut self.cached {
            Some(cached) if !frame.content_changed => {
                let missing: LevelRects = needed.iter().zip(cached.iter()).map(|(need, have)| need.iter().filter(|r| !have.iter().any(|h| contains(*h, **r))).copied().collect()).collect();
                if missing.iter().all(Vec::is_empty) {
                    None
                } else if cached.iter().zip(&missing).any(|(have, m)| have.len() + m.len() > CACHE_RECTS) {
                    // Content that holds still under glass that keeps moving is blurred whole,
                    // once: the glass can then move anywhere over it without another build.
                    *cached = pyramid.layout().whole();
                    Some(cached.clone())
                } else {
                    for (have, m) in cached.iter_mut().zip(&missing) {
                        have.extend_from_slice(m);
                    }
                    Some(missing)
                }
            }
            _ => {
                self.cached = Some(needed.clone());
                Some(needed)
            }
        };
        if let Some(rects) = build {
            let first = binds.down_first.get(frame.content.clone(), |src| down_bind_group(device, &pipelines.down_first, &pipelines.sampler, src));
            pyramid.build_rects(pipelines, encoder, &first, &rects, profiler);
        }

        // Glass pass inputs: the shared buffers, the backdrop `src` and its blur pyramid.
        let (globals, shapes, materials, groups) = (&self.globals, &self.shapes, &self.materials, &self.groups);
        let mut glass_group = |src: &wgpu::TextureView, blur: &Pyramid, layered: bool| {
            binds.glass.get((src.clone(), layered), |(src, _)| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("liquid-rust glass"),
                    layout: &pipelines.glass.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: shapes.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: groups.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pipelines.sampler) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(src) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&blur.full) },
                        wgpu::BindGroupEntry { binding: 6, resource: materials.as_entire_binding() },
                    ],
                })
            })
        };

        // `src` is the backdrop the next layer refracts, `group` the glass bind group reading
        // it and its blur, and `baked` the number of layers already drawn into it.
        let mut group = glass_group(frame.content, pyramid, false);
        let (mut src, mut baked, mut slot) = (frame.content, 0, 0);
        for (i, layer) in layers.iter().enumerate() {
            if let (true, Some(layered)) = (rebuild[i], &self.layered) {
                let region = tail(i);
                // Alternate by rebuild, not by layer index: with a layer in between that
                // needs no rebuild, the index would read and write the same texture.
                let dst = &self.layer_textures[slot];
                slot ^= 1;
                let copy = binds.blit.get(src.clone(), |src| blit_bind_group(device, pipelines, src));
                blit(pipelines, encoder, &copy, dst, Some(region), size);
                // Every layer since the last rebuild, not just the one below: all of them
                // are under this layer.
                for lower in &layers[baked..i] {
                    draw_layer(pipelines, encoder, &group, lower, dst, Some(region), size);
                }
                // A pyramid of its own, so the blur of the content stays cached.
                let first = binds.down_first.get(dst.clone(), |src| down_bind_group(device, &pipelines.down_first, &pipelines.sampler, src));
                layered.build(pipelines, encoder, &first, region, profiler);
                group = glass_group(dst, layered, true);
                (src, baked) = (dst, i);
            }
            draw_layer(pipelines, encoder, &group, layer, frame.target, None, size);
            stamp(profiler, encoder, "glass layer");
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame<'_>, scene: &Scene) {
        let (w, h) = (frame.size.0 as f32, frame.size.1 as f32);
        let angle = scene.light_angle.value.to_radians();
        let touch = &scene.touch;
        let globals = Globals {
            viewport: [w, h, 1.0 / w, 1.0 / h],
            misc: [frame.scale, self.pyramid.as_ref().map_or(1, |p| p.levels) as f32, if scene.appearance.dark { 0.0 } else { 1.0 }, 0.0],
            light: [angle.sin(), -angle.cos(), 0.0, 0.0],
            touch: [touch.pos.x * frame.scale, touch.pos.y * frame.scale, touch.radius.value.max(0.0) * frame.scale, touch.intensity.value.max(0.0)],
        };
        // Rendering again with nothing moved (only the content changed, or the host redrew)
        // uploads nothing.
        if self.uploaded.globals != Some(globals) {
            queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
            self.uploaded.globals = Some(globals);
        }
        let grown = write_storage(device, queue, &mut self.shapes, "liquid-rust shapes", &self.packed.shapes, &mut self.uploaded.shapes)
            | write_storage(device, queue, &mut self.materials, "liquid-rust materials", &self.packed.materials, &mut self.uploaded.materials)
            | write_storage(device, queue, &mut self.groups, "liquid-rust groups", &self.packed.groups, &mut self.uploaded.groups);
        if grown {
            // The glass bind groups point at the old buffers.
            self.binds.glass.clear();
        }
    }
}

/// Draws the glass groups of `layer` with the glass bind group `group` into `dst`.
fn draw_layer(pipelines: &Pipelines, encoder: &mut wgpu::CommandEncoder, group: &wgpu::BindGroup, layer: &Layer, dst: &wgpu::TextureView, region: Option<Bounds>, size: (u32, u32)) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("liquid-rust glass"),
        color_attachments: &[Some(attachment(dst))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(region) = region
        && !scissor(&mut pass, region, size)
    {
        return;
    }
    pass.set_pipeline(&pipelines.glass);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..4, layer.groups.clone());
}

fn layer_texture(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("liquid-rust layer backdrop"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Writes `data` into `buffer` unless it already holds exactly that (`last`), growing it to
/// fit first. Returns `true` when the buffer was reallocated.
fn write_storage<T: Pod + PartialEq>(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &mut wgpu::Buffer, label: &str, data: &[T], last: &mut Vec<T>) -> bool {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    let grown = bytes.len() as u64 > buffer.size();
    if grown {
        *buffer = storage(device, label, (bytes.len() as u64).next_power_of_two());
    }
    if grown || data != last.as_slice() {
        if !bytes.is_empty() {
            queue.write_buffer(buffer, 0, bytes);
        }
        last.clear();
        last.extend_from_slice(data);
    }
    grown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Glass, Rect};

    const ALL_LAYERS: Layers = (Bound::Unbounded, Bound::Unbounded);

    fn groups_for_gap(gap: f32) -> usize {
        let mut scene = Scene::new();
        let c = scene.add_container(20.0, 0);
        scene.add(Glass::new(Rect::new(0.0, 0.0, 50.0, 50.0)).container(c));
        scene.add(Glass::new(Rect::new(50.0 + gap, 0.0, 50.0, 50.0)).container(c));
        while scene.update(1.0 / 60.0) {}
        cluster(&scene, ALL_LAYERS).len()
    }

    #[test]
    fn glass_within_spacing_shares_one_group() {
        assert_eq!(groups_for_gap(10.0), 1);
    }

    #[test]
    fn glass_beyond_spacing_renders_separately() {
        assert_eq!(groups_for_gap(30.0), 2);
    }

    #[test]
    fn container_spacing_changes_at_runtime() {
        let mut scene = Scene::new();
        let c = scene.add_container(20.0, 0);
        scene.add(Glass::new(Rect::new(0.0, 0.0, 50.0, 50.0)).container(c));
        scene.add(Glass::new(Rect::new(80.0, 0.0, 50.0, 50.0)).container(c));
        while scene.update(1.0 / 60.0) {}
        assert_eq!(cluster(&scene, ALL_LAYERS).len(), 2);
        scene.set_container_spacing(c, 40.0);
        assert_eq!(cluster(&scene, ALL_LAYERS).len(), 1);
    }

    #[test]
    fn fused_glass_keeps_its_own_thickness() {
        let mut scene = Scene::new();
        let c = scene.add_container(20.0, 0);
        scene.add(Glass::new(Rect::new(0.0, 0.0, 300.0, 52.0)).container(c));
        scene.add(Glass::new(Rect::new(0.0, 60.0, 300.0, 400.0)).container(c));
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 1.0, Vec2::new(400.0, 500.0), &mut packed);
        assert_eq!((packed.layers[0].groups.clone(), packed.groups.len(), packed.materials.len()), (0..1, 1, 2), "one group, a material per shape");
        assert!(packed.materials[1].shadow[1] > packed.materials[0].shadow[1], "the thick panel casts the softer shadow");
        assert_eq!(packed.groups[0].info[2], 1, "different materials are blended");
    }

    #[test]
    fn fused_glass_keeps_its_own_tint() {
        let mut scene = Scene::new();
        let c = scene.add_container(20.0, 0);
        scene.add(Glass::new(Rect::new(0.0, 0.0, 44.0, 44.0)).material(crate::Material::tinted(crate::Color::BLUE)).container(c));
        scene.add(Glass::new(Rect::new(50.0, 0.0, 44.0, 44.0)).container(c));
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 1.0, Vec2::new(200.0, 100.0), &mut packed);
        assert_eq!(packed.groups.len(), 1, "fused");
        assert!(packed.materials[0].tint[3] > 0.0 && packed.materials[1].tint[3] == 0.0, "each keeps its own tint");
    }

    #[test]
    fn a_row_of_equal_buttons_skips_material_blending() {
        let mut scene = Scene::new();
        let c = scene.add_container(20.0, 0);
        for i in 0..3 {
            scene.add(Glass::new(Rect::new(i as f32 * 50.0, 0.0, 44.0, 44.0)).interactive().container(c));
        }
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 2.0, Vec2::new(400.0, 200.0), &mut packed);
        assert_eq!((packed.groups.len(), packed.groups[0].info[1], packed.groups[0].info[2]), (1, 3, 0));
    }

    #[test]
    fn a_group_that_grew_new_glass_draws_last() {
        let mut scene = Scene::new();
        let bar = scene.add_container(16.0, 0);
        let button = scene.add(Glass::new(Rect::new(0.0, 0.0, 44.0, 44.0)).container(bar));
        let pill = scene.add(Glass::new(Rect::new(0.0, 300.0, 140.0, 44.0)));
        scene.expand_from(button, Glass::new(Rect::new(0.0, 0.0, 300.0, 400.0)).shape(crate::Shape::Rounded(28.0)), crate::Spring::SNAPPY);
        while scene.update(1.0 / 60.0) {}
        let groups = cluster(&scene, ALL_LAYERS);
        assert_eq!(groups.last().map(|g| g.len()), Some(2), "the menu's group is drawn after the pill");
        assert!(groups.iter().any(|g| g.iter().any(|m| m.id == pill)));
    }

    // mypaper: per-glass appearance reaches the material (scheme and transparency).
    #[test]
    fn a_glass_brings_its_own_appearance() {
        let mut scene = Scene::new();
        let dark = crate::Appearance { dark: true, transparency: 1.0, ..Default::default() };
        scene.add(Glass::new(Rect::new(0.0, 0.0, 200.0, 200.0)).appearance(dark));
        scene.add(Glass::new(Rect::new(300.0, 0.0, 200.0, 200.0)));
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 1.0, Vec2::new(600.0, 300.0), &mut packed);
        let [own, scene_wide] = [&packed.materials[0], &packed.materials[1]];
        assert_eq!((own.shadow[3], scene_wide.shadow[3]), (0.0, 1.0), "dark glass in a light scene");
        assert!(own.dark_b[1] > scene_wide.dark_b[1], "its slider tints it further");
    }

    // mypaper: a pyramid rebuild covers what each group reads, not their bounding box.
    #[test]
    fn a_rebuild_covers_only_the_texels_glass_reads() {
        let mut scene = Scene::new();
        scene.appearance = crate::Appearance { adapt_to_content: false, ..Default::default() };
        scene.add(Glass::new(Rect::new(8.0, 8.0, 1690.0, 27.0)));
        scene.add(Glass::new(Rect::new(16.0, 59.0, 203.0, 147.0)));
        scene.add(Glass::new(Rect::new(653.0, 896.0, 400.0, 51.0)));
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 1.5, Vec2::new(2560.0, 1440.0), &mut packed);
        let layout = Layout { size: (2560, 1440), levels: 7 };
        let rects = layout.rects(&packed.reads, packed.reach);
        let texels = |level: &[Bounds]| level.iter().map(area).sum::<f32>();
        let whole = 1280.0 * 720.0;
        assert!(texels(&rects[0]) < 0.6 * whole, "level 0 is mostly skipped: {}", texels(&rects[0]) / whole);
        let read = |r: &Reads| layout.rects(std::slice::from_ref(r), packed.reach);
        for (group, reads) in packed.reads.iter().enumerate() {
            for (level, need) in read(reads).iter().enumerate() {
                for r in need {
                    assert!(rects[level].iter().any(|h| contains(*h, *r)), "group {group} level {level}");
                }
            }
        }
        assert!(rects[4..].iter().all(Vec::is_empty), "nothing reads the top levels of glass that follows the scheme");
    }

    #[test]
    fn higher_z_starts_a_new_layer() {
        let mut scene = Scene::new();
        scene.add(Glass::new(Rect::new(0.0, 0.0, 50.0, 50.0)));
        scene.add(Glass::new(Rect::new(10.0, 10.0, 50.0, 50.0)).z(1));
        while scene.update(1.0 / 60.0) {}
        let mut packed = Packed::default();
        pack(&scene, ALL_LAYERS, 2.0, Vec2::new(200.0, 200.0), &mut packed);
        assert_eq!(packed.layers.len(), 2);
    }

    #[test]
    fn a_layer_range_packs_only_its_glass() {
        let mut scene = Scene::new();
        let menus = scene.add_container(16.0, 1);
        scene.add(Glass::new(Rect::new(0.0, 0.0, 50.0, 50.0)));
        scene.add(Glass::new(Rect::new(60.0, 0.0, 50.0, 50.0)).container(menus));
        scene.add(Glass::new(Rect::new(10.0, 10.0, 50.0, 50.0)).z(2));
        while scene.update(1.0 / 60.0) {}
        let z = |layers: Layers| {
            let mut packed = Packed::default();
            pack(&scene, layers, 1.0, Vec2::new(200.0, 200.0), &mut packed);
            packed.layers.iter().map(|l| l.z).collect::<Vec<_>>()
        };
        assert_eq!(z(ALL_LAYERS), [0, 1, 2]);
        assert_eq!(z((Bound::Unbounded, Bound::Excluded(2))), [0, 1], "a container counts by its own z");
        assert_eq!(z((Bound::Included(2), Bound::Unbounded)), [2]);
    }
}
