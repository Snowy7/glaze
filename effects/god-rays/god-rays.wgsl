// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let light = clamp(vec2f(params.light_x, params.light_y), vec2f(0.0), vec2f(1.0));
    let delta = (light - uv) * clamp(params.length, 0.05, 1.0) / 12.0;
    var sum = vec3f(0.0);
    var q = uv;
    for (var i = 0u; i < 12u; i++) {
        q += delta;
        let rgb = sample_src(q).rgb;
        let mask = smoothstep(clamp(params.threshold, 0.2, 0.95), 1.0, luma(rgb));
        sum += rgb * mask * (1.0 - f32(i) / 18.0);
    }
    return vec4f(base.rgb + sum / 12.0 * clamp(params.amount, 0.0, 1.0) * (1.0 - base.rgb * 0.65), base.a);
}
