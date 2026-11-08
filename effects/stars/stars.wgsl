// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let sky = 1.0 - smoothstep(clamp(params.sky, 0.1, 1.0) * 0.65, clamp(params.sky, 0.1, 1.0), uv.y);
    let dark = 1.0 - smoothstep(0.08, 0.38, luma(base.rgb));
    if sky * dark < 0.001 || params.amount <= 0.0 { return base; }
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 75.0;
    let cell = floor(p);
    let h = hash21(cell);
    let q = fract(p) - vec2f(0.2 + hash21(cell + 1.7) * 0.6, 0.2 + hash21(cell + 51.0) * 0.6);
    let radius = mix(0.04, 0.11, h * h * h * h) * clamp(params.size, 0.5, 3.0);
    let dot_light = 1.0 - smoothstep(0.0, radius * radius * 3.0, dot(q, q));
    let cross_light = max(0.0, 1.0 - abs(q.x) * 140.0 - abs(q.y) * 16.0) + max(0.0, 1.0 - abs(q.x) * 16.0 - abs(q.y) * 140.0);
    let breath = abs(fract(paper.time * (0.08 + h * 0.15) + h * 57.0) * 2.0 - 1.0);
    let twinkle = mix(1.0, 0.3 + 0.7 * breath * breath * (3.0 - 2.0 * breath), clamp(params.twinkle, 0.0, 1.0));
    let star = (dot_light + cross_light * step(0.96, h) * 0.15) * step(hash21(cell + 76.7), clamp(params.amount, 0.0, 1.0) * 0.7) * sky * dark * twinkle * clamp(params.brightness, 0.0, 2.0);
    return vec4f(base.rgb + vec3f(0.72, 0.83, 1.0) * star, base.a);
}
