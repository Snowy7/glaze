// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    // Only the cloud field is downscaled; preserve the sharp preceding stack.
    let base = sample_layer_input(uv);
    let cloud = sample_src(uv).r;
    let amount = clamp(params.amount, 0.0, 0.8);
    let sky = 1.0 - smoothstep(0.25, 0.95, uv.y);
    let shadowed = base.rgb * (1.0 - cloud * amount * (1.0 - sky) * 0.22);
    let veil = cloud * amount * (0.18 + sky * 0.65);
    return vec4f(mix(shadowed, vec3f(0.78, 0.84, 0.91), veil), base.a);
}
