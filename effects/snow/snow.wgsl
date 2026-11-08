// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let aspect = paper.resolution.x / max(paper.resolution.y, 1.0);
    let amount = clamp(params.amount, 0.0, 1.0);
    var flakes = 0.0;
    for (var layer = 0u; layer < 2u; layer++) {
        let z = f32(layer) * clamp(params.depth, 0.0, 1.0) * 2.0;
        let cells = 29.0 / (1.0 + z * 0.9);
        let t = paper.time * clamp(params.speed, 0.0, 2.0);
        let p = vec2f(uv.x * aspect, uv.y) * cells - vec2f(t * clamp(params.wind, -1.0, 1.0) * 1.9, t * (1.1 + z));
        let id = floor(p);
        let seed = id + f32(layer) * 79.1;
        let h = hash21(seed);
        if h > amount * 0.6 { continue; }
        let center = id + 0.5 + (fract(vec2f(h * 13.13, h * 31.71)) - 0.5) * 0.28;
        let sway = abs(fract(t * 0.07 + h) * 2.0 - 1.0) - 0.5;
        let delta = p - center + vec2f(sway * 0.1, 0.0);
        let d = dot(delta, delta);
        let radius = clamp(params.size, 0.3, 4.0) * (0.04 + z * 0.045);
        let shape = max(1.0 - d / max(radius * radius * 3.24, 0.00001), 0.0);
        let flake = shape * shape;
        flakes += flake * step(h, amount * 0.6) * (0.7 + z * 0.1);
    }
    return vec4f(mix(base.rgb, vec3f(0.88, 0.94, 1.0), clamp(flakes, 0.0, 0.85)), base.a);
}
