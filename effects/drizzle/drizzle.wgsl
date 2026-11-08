// Squaring explicitly also accepts negative coordinates without pow domain errors.
fn fx_square(x: f32) -> f32 { return x * x; }
// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let angle = clamp(params.angle, -60.0, 60.0) * 0.0174533 + sin(paper.time * 0.4) * clamp(params.wind, -1.0, 1.0) * 0.08;
    let screen = (uv - 0.5) * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let p = vec2f(screen.x * cos(angle) - screen.y * sin(angle), screen.x * sin(angle) + screen.y * cos(angle)) * vec2f(70.0, 10.0) - vec2f(0.0, paper.time * clamp(params.speed, 0.2, 3.0) * 18.0);
    let cell = floor(p);
    let h = hash21(cell);
    let q = fract(p);
    let center = 0.2 + hash21(cell + 9.1) * 0.6;
    let width = 0.075;
    let streak = exp(-fx_square((q.x - center) / width)) * pow(max(sin(q.y * 3.141593), 0.0), 2.0 / clamp(params.length, 0.2, 2.0));
    let alpha = streak * step(h, clamp(params.amount, 0.0, 1.0) * 0.55) * 0.62;
    return vec4f(mix(base.rgb, vec3f(0.68, 0.81, 0.92), alpha), base.a);
}
