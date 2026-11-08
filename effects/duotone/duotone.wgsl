// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let y = clamp((luma(base.rgb) - 0.45) * clamp(params.contrast, 0.5, 2.0) + 0.5, 0.0, 1.0);
    let toned = mix(to_display(params.shadows.rgb), to_display(params.highlights.rgb), smoothstep(0.0, 1.0, y));
    return vec4f(mix(base.rgb, toned, clamp(params.strength, 0.0, 1.0)), base.a);
}
