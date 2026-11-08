// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let p = uv * 2.0 - 1.0;
    let q = p * (1.0 + dot(p, p) * clamp(params.curvature, 0.0, 0.2)) * 0.5 + 0.5;
    let base = sample_src(clamp(q, vec2f(0.0), vec2f(1.0)));
    let edge_distance = min(min(q.x, q.y), min(1.0 - q.x, 1.0 - q.y));
    let visible = smoothstep(-0.002, 0.002, edge_distance);
    let scan = 1.0 - clamp(params.scanlines, 0.0, 1.0) * (0.5 + 0.5 * cos(frag.y * 3.141593));
    let channel = u32(floor(max(frag.x, 0.0))) % 3u;
    var phosphor = vec3f(1.0 - clamp(params.mask, 0.0, 1.0) * 0.35);
    phosphor[channel] = 1.0;
    let vignette = 1.0 - clamp(params.vignette, 0.0, 1.0) * pow(clamp(dot(p, p) * 0.5, 0.0, 1.0), 1.5);
    return vec4f(base.rgb * scan * phosphor * vignette * visible, base.a);
}
