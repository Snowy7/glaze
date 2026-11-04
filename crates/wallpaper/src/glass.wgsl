// Liquid glass plates (glass_pass.rs). Each plate is drawn twice, as a ring of four quads: first
// every plate's soft shadow (the ring outside the plate), then every plate's glass (the ring of its
// rim band), so no shadow lands on a neighbour's glass and the clear centre is never rasterized
// (it is exactly what is behind). The glass samples `behind`, a copy of the rim strips of the
// frame, bending them inward with a chromatic split under a specular rim.

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
    @location(3) extra: vec4f,  // blur px, fade (0..1, appearing and leaving), pass (0 shadow, 1 glass), 0
};

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) px: vec2f,
    @location(1) @interpolate(flat) rect: vec4f,
    @location(2) @interpolate(flat) shape: vec4f,
    @location(3) @interpolate(flat) tint: vec4f,
    @location(4) @interpolate(flat) extra: vec4f,
};

/// How far the shadow reaches past the plate, px.
const MARGIN: f32 = 12.0;

/// The rim band's width: where the glass bends what is behind it (glass_pass.rs mirrors it).
fn band_of(half: vec2f, r: f32) -> f32 {
    return min(r * 0.8 + 6.0, min(half.x, half.y) * 0.5);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, p: Plate) -> Out {
    var corners = array<vec2f, 6>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0));
    let half = p.rect.zw * 0.5;
    let centre = p.rect.xy + half;
    let r = min(p.shape.x, min(half.x, half.y));
    // The ring: from `outer` in to `inner` (half-sizes around the centre).
    var outer = half;
    var inner = vec2f(0.0);
    if p.extra.z < 0.5 {
        outer = half + MARGIN;
        inner = max(half - r * 0.3, vec2f(0.0)); // the corners' rounding is outside the plate
    } else if p.extra.x < 0.5 && p.tint.a < 0.004 {
        inner = max(half - band_of(half, r), vec2f(0.0)); // a blur or a tint fills the whole plate
    }
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
    } else {
        lo = vec2f(inner.x, -inner.y);
        hi.y = inner.y; // right
    }
    let px = centre + mix(lo, hi, corners[vi % 6u]);
    var o: Out;
    o.pos = vec4f(px.x / u.size.x * 2.0 - 1.0, 1.0 - px.y / u.size.y * 2.0, 0.0, 1.0);
    o.px = px;
    o.rect = p.rect;
    o.shape = p.shape;
    o.tint = p.tint;
    o.extra = p.extra;
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

    if v.extra.z < 0.5 {
        // The shadow: soft, a little below the plate, only outside it.
        if d < 0.0 {
            discard;
        }
        let ds = max(sd_box(local - vec2f(0.0, 2.5), half, r), 0.0);
        let a = strength * 0.26 * exp(-ds / 6.0) * (1.0 - smoothstep(MARGIN * 0.6, MARGIN, ds));
        if a < 0.004 {
            discard;
        }
        return vec4f(0.0, 0.0, 0.0, a);
    }

    // The glass, with a half-pixel antialiased edge.
    let cover = clamp(0.5 - d, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let depth = max(-d, 0.0);
    let band = band_of(half, r);
    let t = clamp(1.0 - depth / band, 0.0, 1.0);
    let blur = v.extra.x;
    if t <= 0.0 && blur < 0.5 && v.tint.a < 0.004 {
        discard; // the clear centre: exactly what is behind
    }
    // The rim's outward normal: the distance field's gradient, worked out (no extra lookups).
    let q = abs(local) - half + r;
    var g = select(vec2f(0.0, 1.0), vec2f(1.0, 0.0), q.x > q.y);
    if q.x > 0.0 && q.y > 0.0 {
        g = normalize(q);
    }
    let n = g * select(vec2f(-1.0), vec2f(1.0), local >= vec2f(0.0));
    // A lens edge: what is behind is pulled toward the rim, more the closer to it, split a
    // little by color (red bends more than blue).
    let bend = t * t * (3.0 - 2.0 * t);
    let shift = bend * bend * v.shape.z * band * 0.8 * strength;
    let at = v.px - n * shift;
    var col = vec3f(tap(at - n * shift * 0.1).r, tap(at).g, tap(at + n * shift * 0.1).b);
    if blur >= 0.5 {
        let k = blur * 0.7;
        col = (col * 2.0 + tap(at + vec2f(k, k)) + tap(at + vec2f(-k, k)) + tap(at + vec2f(k, -k)) + tap(at + vec2f(-k, -k))) / 6.0;
    }
    col = mix(col, v.tint.rgb, v.tint.a);

    // Light from the top left: a crisp rim that brightens where it faces the light, a softer
    // glow just inside it, a faint rim on the far side and a touch of shade at the bottom.
    let light = normalize(vec2f(-0.55, -0.85));
    let facing = max(dot(n, light), 0.0);
    let away = max(dot(n, -light), 0.0);
    let rim = exp(-depth / 1.4);
    let glow = exp(-depth / max(band * 0.32, 1.0));
    let hl = v.shape.w * strength;
    col += vec3f(hl * (rim * (0.28 + 0.72 * pow(facing, 1.4)) + glow * 0.1 * facing + rim * 0.32 * pow(away, 2.0)));
    col *= 1.0 - 0.12 * strength * glow * pow(away, 1.5);
    return vec4f(col, cover);
}
