// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let angle = clamp(params.wind, -180.0, 180.0) * 0.0174533;
    let drift = vec2f(cos(angle), sin(angle)) * paper.time * clamp(params.speed, 0.0, 1.0) * 0.08;
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 3.0 / clamp(params.scale, 0.3, 3.0);
    let n = noise2(p + drift);
    // Two soft octaves at quarter resolution, composited over the sharp layer input.
    let wisps = noise2(p * 2.03 + drift * 1.3 + n * 0.7);
    let cloud = smoothstep(0.28, 0.82, n * 0.72 + wisps * 0.28);
    return vec4f(vec3f(cloud), 1.0);
}
