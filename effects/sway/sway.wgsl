// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let t = paper.time * clamp(params.speed, 0.0, 2.0);
    let n = noise2(uv * clamp(params.scale, 0.5, 6.0) + vec2f(t * 0.2, 0.0));
    let wind = sin(t * 1.7 + uv.y * 4.0 + n * 3.0) * 0.7 + sin(t * 0.73 + uv.x * 6.0) * 0.3;
    let anchor = mix(1.0, pow(1.0 - clamp(uv.y, 0.0, 1.0), 1.5), clamp(params.anchor, 0.0, 1.0));
    let px = clamp(params.amount, 0.0, 60.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    let offset = vec2f(wind, sin(t + n * 4.0) * 0.15) * px * anchor;
    return sample_src(uv + offset);
}
