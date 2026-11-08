// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let angle = clamp(params.wind, -180.0, 180.0) * 0.0174533;
    let drift = vec2f(cos(angle), sin(angle)) * paper.time * clamp(params.speed, 0.0, 1.0) * 0.08;
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 3.0 / clamp(params.scale, 0.3, 3.0);
    // Fractal detail avoids isolated round blotches. Only this soft field is downscaled.
    let cloud = smoothstep(0.30, 0.72, fbm2(p + drift));
    return vec4f(vec3f(cloud), 1.0);
}
