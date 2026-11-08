// Film grain: a new grain pattern each frame, strongest in the midtones.

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let c = sample_src(uv).rgb;
    // Grain cells in screen heights / 1080, so the grain keeps its size at any pass scale.
    let cell = floor(uv * vec2f(paper.resolution.x / paper.resolution.y, 1.0) * 1080.0 / params.size);
    let seed = f32(paper.frame % 64u) * 17.13;
    let n = hash21(cell + seed) + hash21(cell * 1.7 - seed) - 1.0;
    let l = luma(c);
    let weight = 1.0 - abs(l - 0.5) * 1.2;
    let tone = select(vec3f(n), vec3f(n, hash21(cell + seed + 3.1) * 2.0 - 1.0, n), params.colored != 0u);
    return vec4f(clamp(c + tone * params.amount * weight, vec3f(0.0), vec3f(1.0)), 1.0);
}
