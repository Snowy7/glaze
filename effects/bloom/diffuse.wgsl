// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    // Preserve all detail and preceding layers, including when the bloom strength is zero.
    let base = sample_layer_input(uv);
    let glow = to_linear(sample_src(uv).rgb);
    let output = to_linear(base.rgb) + glow * clamp(params.amount, 0.0, 2.0);
    return vec4f(to_display(output), base.a);
}
