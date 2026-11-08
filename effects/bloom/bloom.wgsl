// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = vec2f(1.0) / vec2f(textureDimensions(src));
    var sum = base.rgb * 0.4;
    sum += sample_src(uv + vec2f(px.x, 0.0)).rgb * 0.15;
    sum += sample_src(uv - vec2f(px.x, 0.0)).rgb * 0.15;
    sum += sample_src(uv + vec2f(0.0, px.y)).rgb * 0.15;
    sum += sample_src(uv - vec2f(0.0, px.y)).rgb * 0.15;
    return vec4f(sum, base.a);
}
