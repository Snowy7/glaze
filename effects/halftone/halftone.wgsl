// Original Glaze effect. SPDX-License-Identifier: MIT

// Area of a circle clipped by its square screen cell. Joined dots leave white holes in deep
// shadows, reaching 100% ink rather than stopping at pi/4 and washing the photo with paper.
fn screen_area(radius: f32) -> f32 {
    let r = clamp(radius, 0.0, 0.70710678);
    let clipped = max(r, 0.5);
    let segment = clipped * clipped * acos(0.5 / clipped) - 0.5 * sqrt(max(clipped * clipped - 0.25, 0.0));
    return clamp(3.14159265 * r * r - 4.0 * segment, 0.0, 1.0);
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let angle = clamp(params.angle, 0.0, 90.0) * 0.0174533;
    // Spacing in 1080p pixels: the same dot count on every screen.
    let spacing = max(clamp(params.size, 3.0, 24.0) * paper.resolution.y / 1080.0, 2.0);
    let p = vec2f(cos(angle) * frag.x - sin(angle) * frag.y, sin(angle) * frag.x + cos(angle) * frag.y) / spacing;
    let center = (floor(p) + 0.5) * spacing;
    let center_screen = vec2f(cos(angle) * center.x + sin(angle) * center.y, -sin(angle) * center.x + cos(angle) * center.y);
    let at = center_screen / max(paper.resolution, vec2f(1.0));
    // Average inside this cell so fine detail in preceding layers does not alias into density.
    // Never stretch the whole photo's range: a dark photograph must stay dark.
    let step_uv = vec2f(spacing * 0.25) / paper.resolution;
    let color = (sample_src(at).rgb * 4.0
        + sample_src(at + vec2f(step_uv.x, 0.0)).rgb
        + sample_src(at - vec2f(step_uv.x, 0.0)).rgb
        + sample_src(at + vec2f(0.0, step_uv.y)).rgb
        + sample_src(at - vec2f(0.0, step_uv.y)).rgb) / 8.0;
    let tone = luma(color);
    let paper_rgb = to_display(params.paper_color.rgb);
    let ink_rgb = to_display(params.ink.rgb);
    let paper_y = luma(paper_rgb);
    let ink_y = luma(ink_rgb);
    // Deep shadows and bright highlights can extend beyond the chosen inks' range.
    let ink = ink_rgb * min(1.0, tone / max(ink_y, 0.001));
    let paper_color = mix(paper_rgb, vec3f(1.0), clamp((tone - paper_y) / max(1.0 - paper_y, 0.001), 0.0, 1.0));
    let coverage = clamp((luma(paper_color) - tone) / max(luma(paper_color) - luma(ink), 0.001), 0.0, 1.0);
    let screen = screen_area(length(fract(p) - 0.5));
    let aa = max(fwidth(screen) * 0.5, 0.001);
    let antialiased = 1.0 - smoothstep(coverage - aa, coverage + aa, screen);
    let dot_mask = select(select(antialiased, 0.0, coverage <= 0.0), 1.0, coverage >= 1.0);
    let toned = mix(paper_color, ink, dot_mask);
    return vec4f(mix(base.rgb, toned, clamp(params.strength, 0.0, 1.0)), base.a);
}
