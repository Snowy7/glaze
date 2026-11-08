// Original mypaper effect. SPDX-License-Identifier: MIT
fn snow_layer(screen: vec2f, z: f32, t: f32, size: f32, amount: f32, seed: f32) -> f32 {
    let cells = 29.0 / (1.0 + z * 0.9);
    let p = screen * cells - vec2f(t * clamp(params.wind, -1.0, 1.0) * 1.9, t * (1.1 + z));
    let id = floor(p);
    let h = hash21(id + seed);
    if h > amount * 0.6 { return 0.0; }
    let center = id + 0.5 + (fract(vec2f(h * 13.13, h * 31.71)) - 0.5) * 0.28;
    let sway = abs(fract(t * 0.07 + h) * 2.0 - 1.0) - 0.5;
    let delta = p - center + vec2f(sway * 0.1, 0.0);
    let radius = size * (0.04 + z * 0.045) * (0.65 + h * 0.8);
    let shape = max(1.0 - dot(delta, delta) / max(radius * radius * 3.24, 0.00001), 0.0);
    return shape * shape * (0.65 + z * 0.1) * (0.65 + h * 0.7);
}
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let screen = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let t = paper.time * clamp(params.speed, 0.0, 2.0);
    let amount = clamp(params.amount, 0.0, 1.0);
    let size = clamp(params.size, 0.3, 4.0);
    // Explicit near/far layers let the compiler fold the fixed far-layer grid.
    let flakes = snow_layer(screen, 0.0, t, size, amount, 0.0)
        + snow_layer(screen, clamp(params.depth, 0.0, 1.0) * 2.0, t, size, amount, 79.1);
    return vec4f(mix(base.rgb, vec3f(0.88, 0.94, 1.0), clamp(flakes, 0.0, 0.85)), base.a);
}
