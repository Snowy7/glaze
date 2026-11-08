// Fine, zero-mean monochrome grain. Shadows and highlights retain their detail.

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let c = base.rgb;
    // Grain cells in screen heights / 1080, so the grain keeps its size at any pass scale.
    let cell = floor(uv * vec2f(paper.resolution.x / paper.resolution.y, 1.0) * 1080.0 / max(params.size, 0.5));
    let seed = f32(paper.frame % 4096u) * 17.13;
    let n = hash21(cell + seed) + hash21(cell * 1.7 - seed) - 1.0;
    let l = luma(c);
    let weight = smoothstep(0.015, 0.22, l) * (1.0 - smoothstep(0.65, 0.98, l));
    var tone = vec3f(n);
    if params.colored != 0u {
        let color = vec3f(hash21(cell + seed + 3.1), hash21(cell - seed + 7.3), hash21(cell + seed + 19.7)) - 0.5;
        tone += color * 0.35;
    }
    return vec4f(clamp(c + tone * clamp(params.amount, 0.0, 0.3) * weight, vec3f(0.0), vec3f(1.0)), base.a);
}
