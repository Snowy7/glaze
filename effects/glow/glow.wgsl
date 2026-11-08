// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = clamp(params.radius, 1.0, 80.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    var halo = vec3f(0.0);
    for (var i = 0u; i < 8u; i++) {
        let a = f32(i) * 0.785398;
        let rgb = sample_src(uv + vec2f(cos(a), sin(a)) * px).rgb;
        halo += rgb * smoothstep(clamp(params.threshold, 0.0, 0.95), 1.0, luma(rgb));
    }
    return vec4f(base.rgb + halo / 8.0 * to_display(params.tint.rgb) * clamp(params.amount, 0.0, 2.0) * (1.0 - base.rgb), base.a);
}
