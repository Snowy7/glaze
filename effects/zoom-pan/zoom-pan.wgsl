// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let t = paper.time * clamp(params.speed, 0.02, 0.5);
    let zoom = clamp(params.zoom, 1.0, 1.4);
    let breathing = 1.0 + (zoom - 1.0) * (0.75 + 0.25 * sin(t * 0.43));
    let room = (1.0 - 1.0 / breathing) * 0.5;
    let drift = vec2f(sin(t * 0.31), cos(t * 0.23)) * room * clamp(params.travel, 0.0, 1.0);
    return sample_src((uv - 0.5) / breathing + 0.5 + drift);
}
