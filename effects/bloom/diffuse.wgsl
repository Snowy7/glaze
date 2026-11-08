// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = clamp(params.radius, 2.0, 40.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    var glow = vec3f(0.0);
    for (var i = 0u; i < 12u; i++) {
        let a = f32(i) * 2.3999632;
        let r = sqrt((f32(i) + 0.5) / 12.0);
        let rgb = sample_src(uv + vec2f(cos(a), sin(a)) * px * r).rgb;
        let threshold = clamp(params.threshold, 0.1, 0.95);
        let bright = smoothstep(threshold * 0.7, threshold + 0.1, max(rgb.r, max(rgb.g, rgb.b)));
        glow += to_linear(rgb) * bright;
    }
    let output = to_linear(base.rgb) + glow / 12.0 * clamp(params.amount, 0.0, 1.0);
    return vec4f(to_display(output), base.a);
}
