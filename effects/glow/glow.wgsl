// Original Glaze effect. SPDX-License-Identifier: MIT
fn highlight(uv: vec2f) -> vec3f {
    let rgb = sample_src(uv).rgb;
    let threshold = clamp(params.threshold, 0.0, 0.95);
    return rgb * smoothstep(threshold * 0.55, min(threshold + 0.3, 1.0), luma(rgb));
}
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    // Separable diffusion avoids the old eight discrete copies of bright edges.
    let radius = clamp(params.radius, 1.0, 80.0) * paper.resolution.y / 1080.0;
    let px = vec2f(radius / max(paper.resolution.x, 1.0) / 3.230769, 0.0);
    var halo = highlight(uv) * 0.227027;
    halo += (highlight(uv + px * 1.384615) + highlight(uv - px * 1.384615)) * 0.316216;
    halo += (highlight(uv + px * 3.230769) + highlight(uv - px * 3.230769)) * 0.070270;
    return vec4f(halo, 1.0);
}
