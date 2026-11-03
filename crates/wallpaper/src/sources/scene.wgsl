// Scene wallpaper: a sky gradient, then the world-space glTF mesh with one
// warm sun, hemisphere ambient, a soft rim, exponential fog and emissive.
// All lighting is linear; the swapchain is non-sRGB, so `encode` converts.

struct Globals {
    view_proj: mat4x4f,
    eye: vec4f,        // xyz; w = distance from the eye where fog starts
    sun_dir: vec4f,    // xyz: unit vector toward the sun
    sun_color: vec4f,
    sky: vec4f,        // hemisphere ambient, up
    ground: vec4f,     // hemisphere ambient, down
    fog: vec4f,        // rgb; w = density per world unit
    bg_top: vec4f,     // sky gradient, top of the screen
    bg_bottom: vec4f,  // sky gradient, bottom of the screen
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var base_tex: texture_2d<f32>;
@group(1) @binding(1) var base_samp: sampler;

// Linear -> sRGB, plus ~1/255 of dither to hide banding in dark gradients.
fn encode(c: vec3f, frag: vec2f) -> vec3f {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    let srgb = select(high, low, c <= vec3f(0.0031308));
    let noise = fract(52.9829189 * fract(dot(frag, vec2f(0.06711056, 0.00583715))));
    return srgb + (noise - 0.5) / 255.0;
}

// Identity below the knee, then a smooth shoulder toward 1 so emissive
// glows roll off instead of clipping.
fn tonemap(c: vec3f) -> vec3f {
    let knee = 0.6;
    let over = max(c - knee, vec3f(0.0));
    return min(c, vec3f(knee)) + (1.0 - knee) * (1.0 - exp(-over / (1.0 - knee)));
}

struct SkyOut {
    @builtin(position) pos: vec4f,
    @location(0) t: f32,
}

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: SkyOut;
    out.pos = vec4f(p * 2.0 - 1.0, 1.0, 1.0);
    out.t = 1.0 - p.y; // 0 at the top of the screen
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4f {
    let c = mix(g.bg_top.rgb, g.bg_bottom.rgb, smoothstep(0.0, 1.0, in.t));
    return vec4f(encode(c, in.pos.xy), 1.0);
}

struct VsIn {
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec3f,
    @location(4) emissive: vec3f,
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec3f,
    @location(4) emissive: vec3f,
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = g.view_proj * vec4f(in.pos, 1.0);
    out.world = in.pos;
    out.normal = in.normal;
    out.uv = in.uv;
    out.color = in.color;
    out.emissive = in.emissive;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let n = select(-normalize(in.normal), normalize(in.normal), front);
    let to_eye = g.eye.xyz - in.world;
    let dist = length(to_eye);
    let v = to_eye / dist;
    let albedo = in.color * textureSample(base_tex, base_samp, in.uv).rgb;

    // Squared half-Lambert: a soft terminator instead of a hard one.
    let ndl = dot(n, g.sun_dir.xyz);
    let wrap = clamp(ndl * 0.5 + 0.5, 0.0, 1.0);
    let sun = g.sun_color.rgb * (wrap * wrap);
    let up = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    let ambient = mix(g.ground.rgb, g.sky.rgb, up);

    // Cool rim along silhouettes, strongest on upward-facing edges, plus a
    // faint Blinn highlight on the sunlit side.
    let rim = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), 3.0) * g.sky.rgb * (0.8 + 1.6 * up);
    let h = normalize(g.sun_dir.xyz + v);
    let spec = pow(clamp(dot(n, h), 0.0, 1.0), 40.0) * 0.1 * step(0.0, ndl) * g.sun_color.rgb;

    let lit = albedo * (sun + ambient) + rim + spec + in.emissive;
    let amount = 1.0 - exp(-g.fog.w * max(dist - g.eye.w, 0.0));
    let color = mix(tonemap(lit), g.fog.rgb, amount);
    return vec4f(encode(color, in.clip.xy), 1.0);
}
