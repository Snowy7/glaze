// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let levels = max(round(clamp(params.levels, 2.0, 24.0)) - 1.0, 1.0);
    let dither = (fract(0.75487766 * floor(frag.x) + 0.56984029 * floor(frag.y)) - 0.5) * clamp(params.dither, 0.0, 1.0);
    let toned = floor(clamp(base.rgb, vec3f(0.0), vec3f(1.0)) * levels + 0.5 + dither) / levels;
    return vec4f(mix(base.rgb, clamp(toned, vec3f(0.0), vec3f(1.0)), clamp(params.strength, 0.0, 1.0)), base.a);
}
