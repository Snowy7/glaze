// Aurora: ribbons of light over a mountain ridge.
// Edit and save, and Glaze reloads it live. Inputs: see `paper` in
// crates/wallpaper/src/prelude.wgsl.

fn hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
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
        p = p * 2.03 + vec2f(1.7, 9.2);
        amplitude *= 0.5;
    }
    return value;
}

// One curtain: a wavy bright edge with light streaming up from it.
fn ribbon(uv: vec2f, t: f32, seed: f32) -> vec3f {
    let x = uv.x * 0.9 + seed * 3.1;
    let edge = 0.02 + seed * 0.10
        + 0.09 * sin(x * 1.7 + t * 2.0 + seed * 4.0)
        + 0.16 * (fbm(vec2f(x * 0.9 + t, seed * 7.0)) - 0.5);
    let d = uv.y - edge;
    let rays = 0.35 + 0.65 * fbm(vec2f(x * 14.0 + t * 3.0, seed * 3.0 + d * 1.5));
    let rise = max(d, 0.0);
    // Crisp lower edge with a soft glow beneath it, long fade upward.
    let below = exp(min(d, 0.0) * 28.0);
    let light = exp(-rise * (5.0 - seed * 2.0)) * below * rays;
    let color = mix(vec3f(0.15, 1.0, 0.55), vec3f(0.65, 0.25, 0.95), smoothstep(0.02, 0.42, rise));
    return color * light;
}

fn wallpaper(frag: vec2f) -> vec4f {
    let res = paper.resolution;
    // Centered, aspect-correct, y up.
    let uv = (frag - 0.5 * res) / res.y * vec2f(1.0, -1.0);
    let mouse = (paper.mouse - 0.5 * res) / res.y * vec2f(1.0, -1.0);
    let t = paper.time * 0.05;

    var color = mix(vec3f(0.006, 0.012, 0.035), vec3f(0.025, 0.018, 0.075), smoothstep(-0.5, 0.5, uv.y));

    // Stars, hidden behind the light.
    let star = hash(floor(frag / 2.0));
    color += step(0.9982, star) * (0.55 + 0.45 * sin(paper.time * 1.3 + star * 90.0)) * 0.8;

    var aurora = vec3f(0.0);
    aurora += ribbon(uv, t, 0.0) * 0.9;
    aurora += ribbon(uv, t * 0.8, 0.55) * 0.55;
    aurora += ribbon(uv, t * 1.2, 1.0) * 0.35;
    color = color * (1.0 - min(length(aurora), 1.0) * 0.8) + aurora;

    // A faint glow follows the cursor.
    color += vec3f(0.25, 0.35, 0.6) * 0.008 / (length(uv - mouse) + 0.04);

    // Mountain ridge, with a little reflected glow on the snow line.
    let ridge = -0.30 + 0.22 * fbm(vec2f(uv.x * 1.6, 3.0)) + 0.05 * fbm(vec2f(uv.x * 9.0, 8.0));
    let ground = smoothstep(ridge + 0.002, ridge - 0.002, uv.y);
    let rim = exp(-abs(uv.y - ridge) * 60.0) * 0.12;
    let land = vec3f(0.004, 0.008, 0.018) + aurora * 0.10 * smoothstep(-0.5, ridge, uv.y);
    color = mix(color, land + vec3f(0.1, 0.5, 0.35) * rim, ground);

    // Vignette.
    color *= 1.0 - 0.25 * dot(uv, uv);
    return vec4f(color, 1.0);
}
