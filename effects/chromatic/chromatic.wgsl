// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let d = uv - clamp(vec2f(params.center_x, params.center_y), vec2f(0.0), vec2f(1.0));
    let strength = pow(max(length(d) * 1.4, 0.001), clamp(params.falloff, 0.0, 3.0));
    let offset = d * strength * clamp(params.amount, 0.0, 32.0) / max(paper.resolution, vec2f(1.0));
    return vec4f(sample_src(uv + offset).r, base.g, sample_src(uv - offset).b, base.a);
}
