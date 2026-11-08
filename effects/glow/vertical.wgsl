// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let radius = clamp(params.radius, 1.0, 80.0) * paper.resolution.y / 1080.0;
    let px = vec2f(0.0, radius / max(paper.resolution.y, 1.0) / 3.230769);
    var halo = sample_src(uv).rgb * 0.227027;
    halo += (sample_src(uv + px * 1.384615).rgb + sample_src(uv - px * 1.384615).rgb) * 0.316216;
    halo += (sample_src(uv + px * 3.230769).rgb + sample_src(uv - px * 3.230769).rgb) * 0.070270;
    return vec4f(halo, 1.0);
}
