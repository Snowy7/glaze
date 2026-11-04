// Liquid glass plates (glass_pass.rs), after Apple's Liquid Glass: a clear lens whose rim bends what
// is behind it (the wallpaper just outside the edge is pulled into the rim, split a little by
// color), one thin specular edge lit from the top left with a fainter one opposite, and nearly no
// tint or blur. Each plate is one instance of five quads: four make the ring of its rim band, the
// only part that samples `behind` (a copy of the frame around the rims); the fifth is the centre,
// drawn only for a tint (a flat wash, no sampling) or a blur. A clear centre is never rasterized:
// it is exactly what is behind.

struct Uniforms {
    size: vec2f,
    _pad: vec2f,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var behind: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Plate {
    @location(0) rect: vec4f,   // x, y, width, height: px of this screen
    @location(1) shape: vec4f,  // radius, intensity, refraction, highlight
    @location(2) tint: vec4f,   // a wash, 0..1, alpha last
    @location(3) extra: vec4f,  // blur px, fade (0..1, appearing and leaving), 0, 0
};
// (In `Out`, extra.w says which quad drew the pixel: 1 the rim band, 0 the centre.)

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) px: vec2f,
    @location(1) @interpolate(flat) rect: vec4f,
    @location(2) @interpolate(flat) shape: vec4f,
    @location(3) @interpolate(flat) tint: vec4f,
    @location(4) @interpolate(flat) extra: vec4f,
};

/// The rim band's width: where the glass bends what is behind it (glass_pass.rs mirrors it).
fn band_of(half: vec2f, r: f32) -> f32 {
    return min(r * 0.5 + 5.0, min(half.x, half.y) * 0.45);
}

/// How far the rim reaches out for what it shows, at most (glass_pass.rs copies that far out).
fn reach_of(band: f32, refraction: f32) -> f32 {
    return band * refraction;
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, p: Plate) -> Out {
    var corners = array<vec2f, 6>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0));
    let half = p.rect.zw * 0.5;
    let centre = p.rect.xy + half;
    let r = min(p.shape.x, min(half.x, half.y));
    // The ring from `outer` (half a pixel out, for the antialiased edge) in to `inner`.
    let outer = half + 0.5;
    let inner = max(half - band_of(half, r), vec2f(0.0));
    let quad = vi / 6u;
    var lo = -outer;
    var hi = outer;
    if quad == 0u {
        hi.y = -inner.y; // top
    } else if quad == 1u {
        lo.y = inner.y; // bottom
    } else if quad == 2u {
        lo.y = -inner.y;
        hi = vec2f(-inner.x, inner.y); // left
    } else if quad == 3u {
        lo = vec2f(inner.x, -inner.y);
        hi.y = inner.y; // right
    } else if p.extra.x >= 0.5 || p.tint.a >= 0.004 {
        lo = -inner; // the centre, for a tint or a blur
        hi = inner;
    } else {
        lo = vec2f(0.0); // no centre: a quad of no area
        hi = vec2f(0.0);
    }
    let px = centre + mix(lo, hi, corners[vi % 6u]);
    var o: Out;
    o.pos = vec4f(px.x / u.size.x * 2.0 - 1.0, 1.0 - px.y / u.size.y * 2.0, 0.0, 1.0);
    o.px = px;
    o.rect = p.rect;
    o.shape = p.shape;
    o.tint = p.tint;
    o.extra = vec4f(p.extra.xyz, select(0.0, 1.0, quad < 4u));
    return o;
}

/// Signed distance to a rounded box centred on the origin: negative inside.
fn sd_box(p: vec2f, half: vec2f, r: f32) -> f32 {
    let q = abs(p) - half + r;
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn tap(px: vec2f) -> vec3f {
    return textureSampleLevel(behind, samp, px / u.size, 0.0).rgb;
}

@fragment
fn fs(v: Out) -> @location(0) vec4f {
    let half = v.rect.zw * 0.5;
    let local = v.px - (v.rect.xy + half);
    let r = min(v.shape.x, min(half.x, half.y));
    let d = sd_box(local, half, r);
    let strength = v.shape.y * v.extra.y;
    let blur = v.extra.x;

    // A half-pixel antialiased edge.
    let cover = clamp(0.5 - d, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let depth = max(-d, 0.0);
    let band = band_of(half, r);
    let t = clamp(1.0 - depth / band, 0.0, 1.0);

    // The centre (its own quad, so it never samples what was not copied): a flat wash blended over
    // what is there, or a light blur.
    if v.extra.w < 0.5 {
        if blur < 0.5 {
            return vec4f(v.tint.rgb, v.tint.a * v.extra.y);
        }
        let k = blur * 0.7;
        var c = (tap(v.px) * 2.0 + tap(v.px + vec2f(k, k)) + tap(v.px + vec2f(-k, k)) + tap(v.px + vec2f(k, -k)) + tap(v.px + vec2f(-k, -k))) / 6.0;
        return vec4f(mix(c, v.tint.rgb, v.tint.a), v.extra.y);
    }

    // The rim's outward normal: the distance field's gradient, worked out (no extra lookups).
    let q = abs(local) - half + r;
    var g = select(vec2f(0.0, 1.0), vec2f(1.0, 0.0), q.x > q.y);
    if q.x > 0.0 && q.y > 0.0 {
        g = normalize(q);
    }
    let n = g * select(vec2f(-1.0), vec2f(1.0), local >= vec2f(0.0));

    // A convex lens edge: toward the rim, what is behind comes from further out, steeply at the very
    // edge (a quarter-circle profile), with a slight chromatic split (blue bends a little more).
    let edge = 1.0 - sqrt(max(1.0 - t * t, 0.0));
    let shift = edge * reach_of(band, v.shape.z) * strength;
    let at = v.px + n * shift;
    var col = vec3f(tap(at - n * shift * 0.06).r, tap(at).g, tap(at + n * shift * 0.06).b);
    if blur >= 0.5 {
        let k = blur * 0.7;
        col = (col * 2.0 + tap(at + vec2f(k, k)) + tap(at + vec2f(-k, k)) + tap(at + vec2f(k, -k)) + tap(at + vec2f(-k, -k))) / 6.0;
    }
    col = mix(col, v.tint.rgb, v.tint.a);

    // The specular edge: one crisp line along the rim, brightest where it faces the light (top
    // left), fainter on the opposite edge (the light leaving the glass), and barely elsewhere.
    let light = normalize(vec2f(-0.6, -0.8));
    let facing = max(dot(n, light), 0.0);
    let away = max(dot(n, -light), 0.0);
    let line = exp(-depth * depth / 1.2);
    let sheen = exp(-depth / max(band * 0.25, 1.0));
    let hl = v.shape.w * strength;
    col += vec3f(hl * (line * (0.12 + 0.95 * pow(facing, 2.0) + 0.5 * pow(away, 2.5)) + sheen * 0.06 * facing));
    // The rim's fade-in and out goes with the plate.
    return vec4f(col, cover * min(v.extra.y * 4.0, 1.0));
}
