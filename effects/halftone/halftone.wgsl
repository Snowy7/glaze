// Original Glaze effect. SPDX-License-Identifier: MIT

// The input's tonal range from a 3x3 sample: (low, high) luma. Dots then span the picture's own
// range, so a low-key photo keeps its shapes instead of turning into solid ink.
fn tone_range() -> vec2f {
    var sum = 0.0;
    var squares = 0.0;
    for (var i = 0; i < 9; i++) {
        let at = (vec2f(f32(i % 3), f32(i / 3)) + 0.5) / 3.0;
        let l = luma(sample_src(at).rgb);
        sum += l;
        squares += l * l;
    }
    let mean = sum / 9.0;
    let spread = sqrt(max(squares / 9.0 - mean * mean, 0.0));
    let half_width = max(spread * 1.8, 0.16);
    let low = clamp(mean - half_width, 0.0, 0.7);
    return vec2f(low, min(low + half_width * 2.0, 1.0));
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let angle = clamp(params.angle, 0.0, 90.0) * 0.0174533;
    // Spacing in 1080p pixels: the same dot count on every screen.
    let spacing = max(clamp(params.size, 3.0, 24.0) * paper.resolution.y / 1080.0, 2.0);
    let p = vec2f(cos(angle) * frag.x - sin(angle) * frag.y, sin(angle) * frag.x + cos(angle) * frag.y) / spacing;
    let center = (floor(p) + 0.5) * spacing;
    let center_screen = vec2f(cos(angle) * center.x + sin(angle) * center.y, -sin(angle) * center.x + cos(angle) * center.y);
    let range = tone_range();
    let y = smoothstep(range.x, range.y, luma(sample_src(center_screen / max(paper.resolution, vec2f(1.0))).rgb));
    let radius = sqrt(1.0 - y) * 0.565;
    let d = length(fract(p) - 0.5);
    let aa = max(fwidth(d), 0.008);
    let dot_mask = 1.0 - smoothstep(radius - aa, radius + aa, d);
    let toned = mix(to_display(params.paper_color.rgb), to_display(params.ink.rgb), dot_mask);
    return vec4f(mix(base.rgb, toned, clamp(params.strength, 0.0, 1.0)), base.a);
}
