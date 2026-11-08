// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = clamp(params.radius, 0.5, 12.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    let dx = luma(sample_src(uv + vec2f(px.x, 0.0)).rgb) - luma(sample_src(uv - vec2f(px.x, 0.0)).rgb);
    let dy = luma(sample_src(uv + vec2f(0.0, px.y)).rgb) - luma(sample_src(uv - vec2f(0.0, px.y)).rgb);
    let edge = max(length(vec2f(dx, dy)) - clamp(params.threshold, 0.0, 0.5), 0.0);
    return vec4f(base.rgb + to_display(params.tint.rgb) * edge * clamp(params.amount, 0.0, 2.0) * (1.0 - base.rgb), base.a);
}
