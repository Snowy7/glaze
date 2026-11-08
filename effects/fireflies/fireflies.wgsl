// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let screen = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0);
    var glow = 0.0;
    let t = paper.time * clamp(params.speed, 0.0, 2.0);
    for (var layer = 0u; layer < 2u; layer++) {
        let p = screen * (9.0 - f32(layer) * 3.0);
        let id = floor(p);
        let h = hash21(id + f32(layer) * 41.3);
        if h <= clamp(params.amount, 0.0, 1.0) * 0.32 {
            let phase = abs(fract(vec2f(t * 0.035, t * 0.023) + vec2f(h, h * 7.3)) * 2.0 - 1.0);
            let drift = (phase * phase * (3.0 - 2.0 * phase) - 0.5) * 0.14;
            let center = id + 0.5 + (fract(vec2f(h * 13.13, h * 31.71)) - 0.5) * 0.2 + drift;
            let d2 = dot(p - center, p - center);
            let size = clamp(params.size, 0.3, 2.0) * (0.022 + f32(layer) * 0.015);
            let breath = abs(fract(t * 0.13 + h * 5.7) * 2.0 - 1.0);
            let pulse = breath * breath * (3.0 - 2.0 * breath);
            let core = 1.0 - smoothstep(0.0, size * size * 3.0, d2);
            let halo = 1.0 - smoothstep(0.0, size * size * 16.0, d2);
            glow += (core * 0.9 + halo * 0.1) * step(h, clamp(params.amount, 0.0, 1.0) * 0.32) * pulse;
        }
    }
    return vec4f(base.rgb + to_display(params.tint.rgb) * glow * (1.0 - base.rgb * 0.5), base.a);
}
