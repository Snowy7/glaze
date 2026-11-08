// Squaring explicitly also accepts negative coordinates without pow domain errors.
fn fx_square(x: f32) -> f32 { return x * x; }
// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 14.0 / clamp(params.size, 0.3, 3.0);
    let cell = floor(p - 0.5);
    var dirt = 0.0;
    for (var y = 0; y < 2; y++) { for (var x = 0; x < 2; x++) {
        let id = cell + vec2f(f32(x), f32(y));
        let h = hash21(id + clamp(params.seed, 0.0, 100.0));
        let center = id + 0.5 + (vec2f(h, hash21(id + 8.2)) - 0.5) * 0.5;
        let r = 0.06 + h * 0.19;
        let d = length(p - center);
        dirt += exp(-d * d / (r * r)) * step(0.65, h) + exp(-fx_square((d - r * 0.9) / max(r * 0.14, 0.001))) * 0.18 * step(0.8, h);
    }}
    let light = smoothstep(0.25, 0.9, luma(base.rgb));
    return vec4f(base.rgb + dirt * clamp(params.amount, 0.0, 1.0) * (0.035 + light * 0.2) * vec3f(1.0, 0.85, 0.65), base.a);
}
