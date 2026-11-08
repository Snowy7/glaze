// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let aspect = vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    let mouse = clamp(paper.mouse / max(paper.resolution, vec2f(1.0)), vec2f(0.0), vec2f(1.0));
    let delta = (uv - mouse) * aspect;
    let d = length(delta);
    let direction = delta / max(d, 0.001);
    let wave = sin(d * clamp(params.frequency, 3.0, 30.0) * 6.283185 - paper.time * clamp(params.speed, 0.0, 3.0) * 5.0);
    let envelope = exp(-d * clamp(params.decay, 0.5, 8.0)) * smoothstep(0.0, 0.035, d);
    let offset = direction / aspect * wave * envelope * clamp(params.amount, 0.0, 32.0) / max(paper.resolution.y, 1.0) * paper.resolution.y / 1080.0;
    let base = sample_src(uv + offset);
    // Surface light catches the same ring crests as the refracted image.
    let sheen = wave * envelope * clamp(params.amount, 0.0, 32.0) / 32.0 * 0.12;
    return vec4f(clamp(base.rgb + sheen * vec3f(0.7, 0.85, 1.0), vec3f(0.0), vec3f(1.0)), base.a);
}
