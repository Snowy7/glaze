// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let p = uv * 2.0 - 1.0;
    let q = p * (1.0 + dot(p, p) * clamp(params.curvature, 0.0, 0.2)) * 0.5 + 0.5;
    let base = sample_src(clamp(q, vec2f(0.0), vec2f(1.0)));
    let edge_distance = min(min(q.x, q.y), min(1.0 - q.x, 1.0 - q.y));
    let visible = smoothstep(-0.002, 0.002, edge_distance);
    // Whole-pixel pitches that grow with the screen (1 px stripes at 1080p, 2 px at 4K), so
    // the mask and scanlines look the same everywhere without moire.
    let unit = max(round(paper.resolution.y / 1080.0), 1.0);
    let row = floor(max(frag.y, 0.0) / unit);
    let scan = 1.0 - clamp(params.scanlines, 0.0, 1.0) * (0.5 + 0.5 * cos(row * 3.141593));
    // Selected without a dynamic vector index: the engine compiles with FXC, which turned
    // `phosphor[channel] = 1.0` into a black screen.
    let channel = u32(floor(max(frag.x, 0.0) / unit)) % 3u;
    let lit = vec3f(f32(channel == 0u), f32(channel == 1u), f32(channel == 2u));
    let phosphor = mix(vec3f(1.0 - clamp(params.mask, 0.0, 1.0) * 0.35), vec3f(1.0), lit);
    let vignette = 1.0 - clamp(params.vignette, 0.0, 1.0) * pow(clamp(dot(p, p) * 0.5, 0.0, 1.0), 1.5);
    return vec4f(base.rgb * scan * phosphor * vignette * visible, base.a);
}
