// Original mypaper effect. SPDX-License-Identifier: MIT
fn aperture(uv: vec2f, radius: f32) -> vec3f {
    if radius < 0.25 { return sample_src(uv).rgb; }
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
    let a = clamp(params.angle, -90.0, 90.0) * 0.0174533;
    let p = (uv - vec2f(0.5, clamp(params.focus, 0.0, 1.0))) * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let band = abs(dot(p, vec2f(sin(a), cos(a))));
    let mask = smoothstep(clamp(params.width, 0.02, 0.8) * 0.5, clamp(params.width, 0.02, 0.8) * 0.5 + 0.25, band);
    return vec4f(aperture(uv, mask * clamp(params.blur, 0.0, 24.0) * paper.resolution.y / 1080.0), sample_src(uv).a);
}
