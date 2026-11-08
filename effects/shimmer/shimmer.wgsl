// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let t = paper.time * clamp(params.speed, 0.0, 2.0);
    let scale = clamp(params.scale, 0.5, 4.0);
    let n = noise2(vec2f(uv.x * 18.0 * scale + t * 0.17, uv.y * 60.0 * scale - t * 0.55));
    let band = select(1.0, smoothstep(clamp(params.waterline, 0.0, 1.0), min(clamp(params.waterline, 0.0, 1.0) + 0.12, 1.001), uv.y), params.water != 0u);
    let wave = sin(uv.y * 180.0 * scale + t * 2.2 + n * 5.0);
    let offset = vec2f(wave, sin(uv.x * 28.0 + t + n) * 0.2) * band * clamp(params.amount, 0.0, 32.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    return sample_src(uv + offset);
}
