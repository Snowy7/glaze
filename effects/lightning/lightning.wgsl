// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let cycle = paper.time * clamp(params.frequency, 0.5, 15.0) / 60.0;
    let id = floor(cycle);
    let offset = hash21(vec2f(id, 17.0)) * 0.5;
    let seconds = (fract(cycle) - offset) * 60.0 / clamp(params.frequency, 0.5, 15.0);
    let first = exp(-max(seconds, 0.0) * 18.0) * step(0.0, seconds);
    let second = exp(-max(seconds - 0.18, 0.0) * 12.0) * step(0.18, seconds) * 0.55;
    let flash = clamp(first + second, 0.0, 1.0) * clamp(params.intensity, 0.0, 1.0);
    let sky = mix(1.0, pow(1.0 - clamp(uv.y, 0.0, 1.0), 1.5), clamp(params.sky, 0.0, 1.0));
    return vec4f(base.rgb + vec3f(0.55, 0.65, 0.9) * flash * sky * (1.0 - base.rgb * 0.5), base.a);
}
