// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_layer_input(uv);
    // Diffuse the low-resolution light field, preserving the sharp wallpaper underneath.
    let px = 1.5 / vec2f(textureDimensions(src));
    var rays = sample_src(uv).rgb * 0.5;
    rays += (sample_src(uv + vec2f(px.x, 0.0)).rgb + sample_src(uv - vec2f(px.x, 0.0)).rgb
        + sample_src(uv + vec2f(0.0, px.y)).rgb + sample_src(uv - vec2f(0.0, px.y)).rgb) * 0.125;
    return vec4f(base.rgb + rays * clamp(params.amount, 0.0, 2.0) * (1.0 - base.rgb * 0.65), base.a);
}
