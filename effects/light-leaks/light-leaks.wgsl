// Squaring explicitly also accepts negative coordinates without pow domain errors.
fn fx_square(x: f32) -> f32 { return x * x; }
// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let t = paper.time * clamp(params.speed, 0.0, 1.0);
    let width = clamp(params.width, 0.05, 0.8);
    let center = 0.45 + sin(t * 0.6) * 0.28;
    let a = exp(-fx_square((uv.x + 0.08 + sin(t * 0.3) * 0.04) / width) - fx_square((uv.y - center) / 0.65));
    let edge = exp(-fx_square((uv.x - 1.13) / (width * 0.65)) - fx_square((uv.y - 0.7 - sin(t * 0.4) * 0.2) / 0.5));
    let leak = (to_display(params.tint.rgb) * a + vec3f(0.8, 0.16, 0.3) * edge) * clamp(params.amount, 0.0, 1.0) * (0.8 + sin(t * 0.7) * 0.2);
    return vec4f(1.0 - (1.0 - base.rgb) * (1.0 - clamp(leak, vec3f(0.0), vec3f(1.0))), base.a);
}
