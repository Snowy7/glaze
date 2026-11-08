// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let size = clamp(params.size, 2.0, 64.0) * paper.resolution.y / 1080.0;
    let cell = floor(frag / max(size, 1.0));
    let at = (cell + 0.5) * max(size, 1.0) / max(paper.resolution, vec2f(1.0));
    let base = sample_src(at);
    let q = fract(frag / max(size, 1.0));
    let border = 1.0 - smoothstep(0.02, 0.1, min(min(q.x, q.y), min(1.0 - q.x, 1.0 - q.y)));
    return vec4f(base.rgb * (1.0 - border * clamp(params.grid, 0.0, 1.0) * 0.3), base.a);
}
