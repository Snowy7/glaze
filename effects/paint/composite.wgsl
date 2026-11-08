// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    // Run the expensive brush filter at half scale, then blend over the sharp input.
    let base = sample_layer_input(uv);
    return vec4f(mix(base.rgb, sample_src(uv).rgb, clamp(params.strength, 0.0, 1.0)), base.a);
}
