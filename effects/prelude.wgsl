// mypaper effect prelude (docs/wallpaper-builder.md, "WGSL contract").
// The engine builds each pass as: this file, then the generated `struct Params`, then the pass.
// A pass only has to define:
//
//     fn effect(frag: vec2f, uv: vec2f) -> vec4f
//
// `frag` is the pixel position in the pass's target (top-left origin) and `uv = frag /
// paper.resolution`. A pass at a lower `scale` sees its own smaller resolution, so work in
// `uv` when a size must look the same at every scale.

struct Paper {
    resolution: vec2f, // this pass's target size in pixels
    mouse: vec2f,      // cursor position in pixels (same space as frag)
    time: f32,         // seconds; stops while the wallpaper is hidden
    frame: u32,
    _pad: vec2f,
}

// Immediate data (DX12 root constants): set in the draw itself, so a frame uploads no buffer.
var<immediate> paper: Paper;

// The input: the previous layer, or the previous pass of this effect.
@group(0) @binding(0) var src: texture_2d<f32>;
// Linear filtering, clamped to the edge.
@group(0) @binding(1) var src_sampler: sampler;
// The base before any layer.
@group(0) @binding(2) var original: texture_2d<f32>;
// Scene depth in .r: 0 is near, 1 is far; flat 1.0 when the base has none. Not filterable on
// every GPU: read it with `sample_depth`, never with `src_sampler`.
@group(0) @binding(3) var depth: texture_2d<f32>;
// This effect's params, in declared order (`struct Params` is generated from effect.json).
@group(0) @binding(4) var<uniform> params: Params;

fn sample_src(uv: vec2f) -> vec4f {
    return textureSampleLevel(src, src_sampler, uv, 0.0);
}

fn sample_original(uv: vec2f) -> vec4f {
    return textureSampleLevel(original, src_sampler, uv, 0.0);
}

// Nearest depth at `uv`.
fn sample_depth(uv: vec2f) -> f32 {
    let size = vec2i(textureDimensions(depth));
    let at = clamp(vec2i(uv * vec2f(size)), vec2i(0), size - 1);
    return textureLoad(depth, at, 0).r;
}

// A cheap stable hash: 0..1 per integer cell.
fn hash21(p: vec2f) -> f32 {
    var q = fract(p * vec2f(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

// Smooth value noise, 0..1.
fn noise2(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2f(1.0, 0.0));
    let c = hash21(i + vec2f(0.0, 1.0));
    let d = hash21(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Five octaves of `noise2`, 0..1.
fn fbm2(p: vec2f) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 5; i++) {
        sum += amp * noise2(q);
        q = q * 2.02 + vec2f(17.0, 9.0);
        amp *= 0.5;
    }
    return sum / 0.96875;
}

// Rec. 709 luma of a color.
fn luma(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// `src` and `original` hold display (sRGB-encoded) values, while `color` params are linear.
// Convert with these when the math needs light, e.g. tinting or blooming.
fn to_linear(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(2.2));
}

fn to_display(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.2));
}

@vertex
fn mypaper_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // One triangle that covers the whole target.
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn mypaper_fx(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    return effect(pos.xy, pos.xy / paper.resolution);
}
