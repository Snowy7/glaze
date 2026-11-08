// Squaring explicitly also accepts negative coordinates without pow domain errors.
fn fx_square(x: f32) -> f32 { return x * x; }
// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let aspect = vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let light = clamp(vec2f(params.light_x, params.light_y), vec2f(0.0), vec2f(1.0));
    let d = (uv - light) * aspect;
    let r = length(d);
    let size = clamp(params.size, 0.1, 1.0);
    let angle = atan2(d.y, d.x + 0.00001);
    let halo = exp(-r * r / (size * size * 0.15)) * 0.35;
    let core = exp(-r * r / (size * size * 0.001));
    let rays = pow(max(cos(angle * 6.0), 0.0), 22.0) * exp(-r / (size * 0.6)) * 0.14;
    let ghost1 = exp(-fx_square(length((uv - (0.5 + (0.5 - light) * 0.5)) * aspect) / (size * 0.15))) * 0.035;
    let ghost2 = exp(-fx_square(length((uv - (0.5 + (0.5 - light) * 1.2)) * aspect) / (size * 0.25))) * 0.025;
    let flare = (to_display(params.tint.rgb) * (halo + core + rays) + vec3f(0.4, 0.7, 1.0) * ghost1 + vec3f(0.6, 1.0, 0.65) * ghost2) * clamp(params.amount, 0.0, 1.0);
    return vec4f(base.rgb + flare * (1.0 - base.rgb * 0.7), base.a);
}
