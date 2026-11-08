// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let cycle = paper.time * clamp(params.frequency, 0.5, 20.0) / 60.0;
    let event_id = floor(cycle);
    let offset = hash21(vec2f(event_id, 31.0)) * 0.65;
    let elapsed = (fract(cycle) - offset) * 60.0 / clamp(params.frequency, 0.5, 20.0);
    let gate = step(0.0, elapsed) * (1.0 - step(clamp(params.duration, 0.03, 0.5), elapsed));
    let row = floor(uv.y * 28.0);
    let seed = hash21(vec2f(row, floor(paper.time * 18.0)));
    let slip = (seed - 0.5) * step(0.72, seed) * gate * clamp(params.amount, 0.0, 1.0) * 0.09;
    let q = uv + vec2f(slip, 0.0);
    let base = sample_src(q);
    let fringe = gate * clamp(params.amount, 0.0, 1.0) * 0.003;
    return vec4f(sample_src(q + vec2f(fringe, 0.0)).r, base.g, sample_src(q - vec2f(fringe, 0.0)).b, base.a);
}

