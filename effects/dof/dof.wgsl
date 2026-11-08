// Original mypaper effect. SPDX-License-Identifier: MIT
fn aperture(uv: vec2f, radius: f32) -> vec3f {
    let pixel = radius / max(paper.resolution, vec2f(1.0));
    var sum = sample_src(uv).rgb;
    for (var i = 0u; i < 12u; i++) {
        let a = f32(i) * 2.3999632;
        let r = sqrt((f32(i) + 0.5) / 12.0);
        sum += sample_src(uv + vec2f(cos(a), sin(a)) * r * pixel).rgb;
    }
    return sum / 13.0;
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let d = sample_depth(uv);
    let has_depth = sample_depth(vec2f(0.2, 0.2)) < 0.99999 || sample_depth(vec2f(0.8, 0.8)) < 0.99999 || d < 0.99999;
    let distance = select(uv.y, d, has_depth);
    let coc = smoothstep(clamp(params.width, 0.02, 0.8) * 0.5, clamp(params.width, 0.02, 0.8) * 0.5 + 0.3, abs(distance - clamp(params.focus, 0.0, 1.0)));
    let radius = coc * clamp(params.size, 0.0, 24.0) * paper.resolution.y / 1080.0;
    return vec4f(aperture(uv, radius), base.a);
}
