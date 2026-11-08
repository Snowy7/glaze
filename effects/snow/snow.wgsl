// Original Glaze effect. SPDX-License-Identifier: MIT
fn snow_layer(screen: vec2f, z: f32, t: f32, size: f32, amount: f32, seed: f32) -> f32 {
    let cells = mix(38.7, 12.3, z);
    let wind = clamp(params.wind, -1.0, 1.0);
    let p = screen * cells - vec2f(t * (wind * (0.7 + z) + sin(t * 0.31 + z * 7.0) * 0.11), 0.0);
    // Each column falls at its own speed and phase, so rows never travel as a lattice.
    let column = hash21(vec2f(floor(p.x), seed + 71.0));
    let falling = p - vec2f(0.0, t * mix(0.45, 1.8, z) * mix(0.65, 1.3, column) + column * 37.0);
    let h = hash21(floor(falling) + seed);
    if h > amount * mix(0.65, 0.48, z) { return 0.0; }
    let random = fract(vec3f(h) * vec3f(13.13, 31.71, 73.37));
    let center = vec2f(0.08) + random.xy * 0.84;
    let delta = fract(falling) - center;
    let defocus = smoothstep(0.65, 1.0, random.z) * z;
    let radius = size * mix(0.8, 3.4, z) * mix(0.6, 1.5, random.z) * (1.0 + defocus);
    let pixels = delta * 1080.0 / cells;
    let shape = max(1.0 - dot(pixels, pixels) / max(radius * radius, 0.001), 0.0);
    let opacity = mix(0.32, 0.72, z) / (1.0 + defocus * 1.5);
    return shape * shape * opacity;
}
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let screen = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let t = paper.time * clamp(params.speed, 0.0, 2.0);
    let amount = clamp(params.amount, 0.0, 1.0);
    let size = clamp(params.size, 0.3, 4.0);
    let depth = clamp(params.depth, 0.0, 1.0);
    let flakes = snow_layer(screen, 0.0, t, size, amount, 19.07)
        + snow_layer(screen + vec2f(0.37, 0.19), depth * 0.55, t, size, amount, 79.19)
        + snow_layer(screen + vec2f(0.13, 0.61), depth, t, size, amount, 154.01);
    return vec4f(vec3f(clamp(flakes, 0.0, 0.85)), 1.0);
}
