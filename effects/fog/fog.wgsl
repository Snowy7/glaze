// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let floor_height = 1.0 - clamp(params.height, 0.0, 1.0);
    let ground = smoothstep(floor_height, floor_height + clamp(params.softness, 0.05, 0.7), uv.y);
    if ground < 0.001 || params.amount <= 0.0 { return base; }
    let aspect = paper.resolution.x / max(paper.resolution.y, 1.0);
    let t = paper.time * clamp(params.speed, 0.0, 1.0);
    let p = uv * vec2f(aspect * 3.5, 2.5);
    // Two decorrelated octaves keep this broad mist inexpensive at full resolution.
    let billow = noise2(p + vec2f(t * 0.13, -t * 0.035));
    let wisps = noise2(p * 2.03 + vec2f(-t * 0.08, billow * 0.6) + 17.7);
    let scene_depth = sample_depth(uv);
    let density = clamp(params.amount, 0.0, 1.0) * ground * (0.35 + billow * 0.5 + wisps * 0.35) * mix(0.25, 1.0, clamp(scene_depth, 0.0, 1.0));
    return vec4f(mix(base.rgb, to_display(params.tint.rgb), clamp(density, 0.0, 0.9)), base.a);
}
