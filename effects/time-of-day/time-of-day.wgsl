// Original mypaper effect. SPDX-License-Identifier: MIT
// Requires the engine's real local clock in the prelude; elapsed animation time is unsuitable.
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let hour = clamp(paper.clock_hours, 0.0, 24.0);
    let night = max(1.0 - smoothstep(4.5, 8.0, hour), smoothstep(17.5, 22.0, hour));
    let dawn = smoothstep(4.5, 6.0, hour) * (1.0 - smoothstep(7.0, 9.0, hour));
    let dusk = smoothstep(15.5, 17.5, hour) * (1.0 - smoothstep(19.0, 21.0, hour));
    let golden = max(dawn * 0.75, dusk) * clamp(params.warmth, 0.0, 1.0);
    let cool = mix(vec3f(1.0), vec3f(0.79, 0.88, 1.04), night);
    let temperature = mix(cool, vec3f(1.12, 1.015, 0.84), golden);
    let exposure = 1.0 - night * clamp(params.night_dim, 0.0, 0.5);
    let graded = clamp(base.rgb * temperature * exposure + vec3f(0.018, 0.006, 0.0) * golden, vec3f(0.0), vec3f(1.0));
    return vec4f(mix(base.rgb, graded, clamp(params.strength, 0.0, 1.0)), base.a);
}
