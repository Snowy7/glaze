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
    // A feathered atmospheric fan also reads on photos without HDR highlights.
    let d = (uv - light) * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let distance = max(length(d), 0.001);
    let angle = atan2(d.y, d.x + 0.00001);
    let shafts = smoothstep(0.32, 0.78, noise2(vec2f(angle * 16.0, 3.1)));
    let fan = smoothstep(0.15, 0.8, dot(d / distance, normalize(vec2f(-0.45, 1.0))));
    let atmosphere = (shafts * 0.28 + 0.025) * fan * exp(-distance * 2.0);
    let rays = sum / 12.0 + vec3f(1.0, 0.83, 0.56) * atmosphere;
    return vec4f(base.rgb + rays * clamp(params.amount, 0.0, 2.0) * (1.0 - base.rgb * 0.65), base.a);
}
