// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let p = uv * vec2f(paper.resolution.x / max(paper.resolution.y, 1.0), 1.0) * 4.0 + vec2f(paper.time * clamp(params.speed, 0.0, 1.0) * 0.12, -paper.time * clamp(params.speed, 0.0, 1.0) * 0.08);
    let id = floor(p);
    var light = 0.0;
        let h = hash21(id);
        let center = id + 0.5 + (vec2f(h, hash21(id + 17.1)) - 0.5) * 0.12;
        let r = min((0.09 + h * 0.12) * clamp(params.size, 0.3, 2.5), 0.38);
        let d = length(p - center);
        let disc = 1.0 - smoothstep(r * 0.75, r * 1.12, d);
        let rim = 1.0 - smoothstep(r * 0.02, r * 0.18, abs(d - r * 0.87));
        light += (disc * 0.07 + rim * 0.04) * step(h, clamp(params.amount, 0.0, 1.0) * 0.8);
    return vec4f(base.rgb + to_display(params.tint.rgb) * light * (1.0 - base.rgb), base.a);
}
