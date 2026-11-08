// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_layer_input(uv);
    let halo = sample_src(uv).rgb * to_display(params.tint.rgb);
    // Display-space screen blending gives this a softer photographic veil than linear bloom.
    let veil = clamp(halo * clamp(params.amount, 0.0, 2.0), vec3f(0.0), vec3f(1.0));
    return vec4f(1.0 - (1.0 - base.rgb) * (1.0 - veil), base.a);
}
