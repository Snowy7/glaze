// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let angle = clamp(params.wind, -180.0, 180.0) * 0.0174533;
    let drift = vec2f(cos(angle), sin(angle)) * paper.time * clamp(params.speed, 0.0, 1.0) * 0.08;
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 3.0 / clamp(params.scale, 0.3, 3.0);
    let n = noise2(p + drift);
    let shadow = smoothstep(0.32, 0.72, n) * clamp(params.amount, 0.0, 0.8);
    return vec4f(base.rgb * (1.0 - shadow), base.a);
}
