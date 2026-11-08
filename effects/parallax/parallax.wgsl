// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let amount = clamp(params.amount, 0.0, 0.05);
    let mouse = clamp(paper.mouse / max(paper.resolution, vec2f(1.0)), vec2f(0.0), vec2f(1.0)) - 0.5;
    let d = sample_depth(uv);
    let fake = 1.0 - luma(sample_src(uv).rgb);
    let has_depth = d < 0.99999 || sample_depth(vec2f(0.5)) < 0.99999;
    var z = select(fake, d, has_depth);
    z = select(z, 1.0 - z, params.invert != 0u);
    let travel = mouse * amount * (0.3 + (1.0 - z) * clamp(params.depth, 0.0, 2.0));
    let overscan = 1.0 + amount * 3.0;
    return sample_src((uv - 0.5) / overscan + 0.5 + travel);
}
