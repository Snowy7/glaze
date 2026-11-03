// Midnight: a calm sea under a full moon.
// A deep-blue sky with sparse twinkling stars and slow, moonlit clouds, a distant
// headland on the horizon, and a broken path of moonlight on the water.
//
// Cheap on purpose: every layer is smooth, so theme.json renders it at 30 fps and
// scale 0.6 (about a third of the pixels) and lets the GPU upscale it. The deepest
// cost is two 4-octave fbm calls for the clouds and one for the waves.
// Inputs: see `paper` in crates/wallpaper/src/prelude.wgsl.

const HORIZON = -0.12;            // y of the horizon, in the aspect-corrected y-up space of `uv`
const MOON = vec2f(0.38, 0.26);   // moon centre in the same space
const MOON_R = 0.042;
const MOON_LIGHT = vec3f(0.86, 0.92, 1.0);

fn hash(p: vec2f) -> f32 {
    var q = fract(p * vec2f(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x),
        mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x),
        u.y,
    );
}

fn fbm(start: vec2f) -> f32 {
    var p = start;
    var value = 0.0;
    var amplitude = 0.5;
    for (var i = 0; i < 4; i++) {
        value += amplitude * noise(p);
        p = p * 2.07 + vec2f(5.3, 1.7);
        amplitude *= 0.5;
    }
    return value;
}

// Sky colour by height above the horizon (0 at the horizon, 1 at the top edge):
// a luminous blue band that sinks into near-black at the zenith.
fn sky_gradient(h: f32) -> vec3f {
    let low = vec3f(0.075, 0.215, 0.43);
    let mid = vec3f(0.016, 0.062, 0.20);
    let top = vec3f(0.004, 0.011, 0.058);
    let c = mix(low, mid, smoothstep(0.0, 0.32, h));
    return mix(c, top, smoothstep(0.22, 1.0, h));
}

// Two layers of tiny stars. Each is a jittered point per grid cell; a few cells
// get one, and every star twinkles at its own slow rate.
fn stars(uv: vec2f) -> f32 {
    // The whole field turns very slowly around a point below the horizon.
    let a = paper.time * 0.0007;
    let c = cos(a);
    let s = sin(a);
    let rel = uv - vec2f(-0.3, -0.9);
    let p = vec2f(c * rel.x - s * rel.y, s * rel.x + c * rel.y);

    var total = 0.0;
    for (var layer = 0; layer < 2; layer++) {
        let scale = select(62.0, 118.0, layer == 1);
        let g = p * scale + f32(layer) * 19.0;
        let id = floor(g);
        let r = hash(id);
        let jitter = vec2f(hash(id + 7.1), hash(id + 3.7)) * 0.6 + 0.2;
        let d = length(fract(g) - jitter);
        let density = select(0.045, 0.06, layer == 1);
        let present = step(1.0 - density, r);
        let size = select(150.0, 260.0, layer == 1);
        let strength = select(1.0, 0.45, layer == 1);
        let rate = 0.6 + 2.2 * hash(id + 1.3);
        let twinkle = 0.78 + 0.22 * sin(paper.time * rate + r * 80.0);
        let magnitude = pow(hash(id + 9.9), 2.5);
        total += present * exp(-d * d * size) * (0.25 + magnitude) * strength * twinkle;
    }
    return total;
}

// The moon: a softly shaded disc with faint seas, a halo, and a thin ice-crystal ring.
fn moon(uv: vec2f) -> vec3f {
    let to = uv - MOON;
    let d = length(to);
    let local = to / MOON_R;
    let disc = smoothstep(MOON_R, MOON_R - 0.0012, d);
    let seas = smoothstep(0.38, 0.66, fbm(local * 1.7 + vec2f(3.1, 1.4)));
    let sphere = sqrt(max(1.0 - dot(local, local), 0.0));
    let surface = MOON_LIGHT * mix(0.70, 1.02, seas) * (0.80 + 0.30 * sphere);
    let halo = vec3f(0.34, 0.50, 0.95) * (exp(-d * 11.0) * 0.55 + exp(-d * 3.2) * 0.17)
        + vec3f(0.55, 0.65, 0.95) * exp(-d * 28.0) * 0.30;
    let ring = vec3f(0.30, 0.45, 0.80) * exp(-pow((d - 0.215) / 0.022, 2.0)) * 0.030;
    return surface * disc * 1.15 + halo + ring;
}

fn clouds(uv: vec2f, base: vec3f) -> vec3f {
    let h = uv.y - HORIZON;
    // Project the cloud deck onto a plane overhead, so it compresses toward the
    // horizon, then drift it sideways.
    let k = 1.0 / (h + 0.13);
    let cp = vec2f(uv.x * k * 0.9 + paper.time * 0.010, k * 0.55);
    let shape = fbm(cp * vec2f(1.0, 1.7) + vec2f(0.0, 4.0));
    let cover = smoothstep(0.46, 0.78, shape);
    // Moon-facing edges are brighter: compare the field with itself nudged to the moon.
    let toward = normalize(MOON - uv) * vec2f(1.0, 1.7);
    let nudged = fbm(cp * vec2f(1.0, 1.7) + vec2f(0.0, 4.0) + toward * 0.10);
    let rim = clamp((shape - nudged) * 5.0 + 0.45, 0.0, 1.0);
    let near_moon = 1.0 / (1.0 + dot(uv - MOON, uv - MOON) * 14.0);
    let dark = vec3f(0.020, 0.050, 0.135);
    let lit = vec3f(0.42, 0.56, 0.86);
    let cloud = mix(dark, lit, rim * (0.12 + 0.88 * near_moon));
    // Thin out toward the horizon haze and the top of the sky.
    let alpha = cover * smoothstep(0.0, 0.09, h) * 0.85;
    return mix(base, cloud, alpha);
}

// The sea. `depth` is how far below the horizon the pixel is.
fn sea(uv: vec2f) -> vec3f {
    let depth = HORIZON - uv.y;
    let z = 1.0 / (depth + 0.02);
    let ground = vec2f(uv.x * z, z);
    let t = paper.time;

    // Slow swell slopes (stretched sideways, like real wind-chop) from one fbm.
    let swell = fbm(vec2f(ground.x * 0.55, ground.y * 1.5 - t * 0.22));
    let ripple = noise(vec2f(ground.x * 2.2 + t * 0.12, ground.y * 5.0 - t * 0.5));

    // Water colour: dark close up, luminous blue where it reflects the horizon band.
    let far = exp(-depth * 4.0);
    var color = mix(vec3f(0.006, 0.020, 0.075), vec3f(0.060, 0.170, 0.340), far);
    color *= 0.82 + 0.36 * swell;

    // The moon's reflection: a column under the moon that widens toward us, jittered
    // sideways by the wave slope so it breaks up into dashes.
    let spread = 0.02 + depth * 0.30;
    let off = (uv.x - MOON.x) + (swell - 0.5) * (0.05 + depth * 0.55);
    let column = exp(-off * off / (spread * spread));
    let broad = exp(-pow((uv.x - MOON.x) / (spread * 4.5 + 0.04), 2.0));
    let glint = smoothstep(0.50, 0.90, ripple);
    let fade = smoothstep(0.0, 0.015, depth) * exp(-depth * 1.3);
    color += MOON_LIGHT * column * (0.10 + 1.15 * glint) * fade;
    color += vec3f(0.14, 0.28, 0.55) * broad * fade * 0.55;
    return color;
}

fn wallpaper(frag: vec2f) -> vec4f {
    let res = paper.resolution;
    // Centered, aspect-correct, y up.
    let uv = (frag - 0.5 * res) / res.y * vec2f(1.0, -1.0);

    let h = clamp((uv.y - HORIZON) / (0.5 - HORIZON), 0.0, 1.0);
    var color: vec3f;

    if uv.y >= HORIZON {
        // Sky.
        var sky = sky_gradient(h);
        let d_moon = length(uv - MOON);
        sky += vec3f(0.05, 0.10, 0.22) * exp(-d_moon * 2.4); // moonlit air
        // Stars, dimmed near the horizon and washed out around the moon.
        let wash = smoothstep(0.03, 0.40, d_moon) * smoothstep(0.0, 0.18, uv.y - HORIZON);
        sky += vec3f(0.85, 0.92, 1.0) * stars(uv) * wash;
        color = clouds(uv, sky);
        color += moon(uv);
    } else {
        color = sea(uv);
    }

    // A distant headland on the left, just breaking the horizon.
    let headland = HORIZON + 0.022 * exp(-pow((uv.x + 0.62) / 0.20, 2.0))
        * (0.7 + 0.5 * noise(vec2f(uv.x * 14.0, 2.0)));
    let land = smoothstep(headland + 0.0015, headland - 0.0015, uv.y) * step(HORIZON - 0.0015, uv.y);
    color = mix(color, vec3f(0.006, 0.014, 0.032), land);

    // Haze where the sky meets the water.
    color += vec3f(0.06, 0.15, 0.30) * exp(-abs(uv.y - HORIZON) * 40.0) * 0.7;

    // Soft tone curve (the moon runs past 1.0), vignette, and a hair of dither
    // so the dark gradients don't band.
    color = 1.0 - exp(-color * 1.18);
    color *= 1.0 - 0.30 * dot(uv, uv) * 0.7;
    color += (hash(frag) - 0.5) / 160.0;
    return vec4f(color, 1.0);
}
