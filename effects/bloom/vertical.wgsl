// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let radius = clamp(params.radius, 2.0, 80.0) * paper.resolution.y / 1080.0;
    let pixel = vec2f(0.0, radius / max(paper.resolution.y, 1.0) / 3.230769);
    var glow = to_linear(sample_src(uv).rgb) * 0.227027;
    glow += (to_linear(sample_src(uv + pixel * 1.384615).rgb) + to_linear(sample_src(uv - pixel * 1.384615).rgb)) * 0.316216;
    glow += (to_linear(sample_src(uv + pixel * 3.230769).rgb) + to_linear(sample_src(uv - pixel * 3.230769).rgb)) * 0.070270;
    return vec4f(to_display(glow), 1.0);
}
