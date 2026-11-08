// Original mypaper effect. SPDX-License-Identifier: MIT
fn bright(uv: vec2f) -> vec3f {
    let rgb = sample_src(uv).rgb;
    let threshold = clamp(params.threshold, 0.1, 0.95);
    let gate = smoothstep(threshold * 0.7, min(threshold + 0.1, 1.0), max(rgb.r, max(rgb.g, rgb.b)));
    return to_linear(rgb) * gate;
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    // Five bilinear taps approximate a nine-tap Gaussian, in linear light at half scale.
    let radius = clamp(params.radius, 2.0, 40.0) * paper.resolution.y / 1080.0;
    let pixel = vec2f(radius / max(paper.resolution.x, 1.0) / 3.230769, 0.0);
    var glow = bright(uv) * 0.227027;
    glow += (bright(uv + pixel * 1.384615) + bright(uv - pixel * 1.384615)) * 0.316216;
    glow += (bright(uv + pixel * 3.230769) + bright(uv - pixel * 3.230769)) * 0.070270;
    return vec4f(to_display(glow), 1.0);
}
