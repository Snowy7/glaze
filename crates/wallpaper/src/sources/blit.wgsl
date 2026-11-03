// Draws a texture over the whole target. `scale`/`offset` map screen UV to
// texture UV (see fit_transform in blit.rs); outside the texture is black.

struct Blit {
    scale: vec2f,
    offset: vec2f,
    opacity: f32,
    // Three scalars, not a vec3f (which would align to 16 and grow the struct
    // to 48 bytes); this keeps it at the 32 bytes of `Params` in blit.rs.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var<uniform> blit: Blit;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(p.x, 1.0 - p.y);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4f {
    let uv = in.uv * blit.scale + blit.offset;
    let inside = all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0));
    let color = textureSampleLevel(tex, samp, uv, 0.0).rgb;
    return vec4f(select(vec3f(0.0), color, inside), blit.opacity);
}
