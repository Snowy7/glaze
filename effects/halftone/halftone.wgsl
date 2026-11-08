// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let angle = clamp(params.angle, 0.0, 90.0) * 0.0174533;
    let spacing = max(clamp(params.size, 3.0, 24.0) * paper.resolution.y / 1080.0, 2.0);
    let p = vec2f(cos(angle) * frag.x - sin(angle) * frag.y, sin(angle) * frag.x + cos(angle) * frag.y) / spacing;
    let center = (floor(p) + 0.5) * spacing;
    let center_screen = vec2f(cos(angle) * center.x + sin(angle) * center.y, -sin(angle) * center.x + cos(angle) * center.y);
    let y = smoothstep(0.08, 0.82, luma(sample_src(center_screen / max(paper.resolution, vec2f(1.0))).rgb));
    let radius = sqrt(1.0 - y) * 0.565;
    let d = length(fract(p) - 0.5);
    let aa = max(fwidth(d), 0.008);
    let dot_mask = 1.0 - smoothstep(radius - aa, radius + aa, d);
    let toned = mix(to_display(params.paper_color.rgb), to_display(params.ink.rgb), dot_mask);
    return vec4f(mix(base.rgb, toned, clamp(params.strength, 0.0, 1.0)), base.a);
}
