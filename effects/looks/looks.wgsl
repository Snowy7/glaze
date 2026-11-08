// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let rgb = clamp(base.rgb, vec3f(0.0), vec3f(1.0));
    let y = luma(rgb);
    var graded = rgb;
    switch min(params.look, 6u) {
        case 0u: {
            let shadows = 1.0 - smoothstep(0.08, 0.6, y);
            let lights = smoothstep(0.25, 0.9, y);
            graded = rgb * 1.04 + vec3f(-0.04, 0.04, 0.08) * shadows + vec3f(0.12, 0.025, -0.08) * lights;
        }
        case 1u: { graded = mix(vec3f(y), rgb, 0.78) * 0.89 + vec3f(0.065, 0.052, 0.045); }
        case 2u: { graded = vec3f(smoothstep(0.015, 0.98, y)); }
        case 3u: { graded = rgb * vec3f(1.09, 1.015, 0.88) + vec3f(0.025, 0.008, 0.0); }
        case 4u: { graded = rgb * vec3f(0.78, 0.88, 1.04) + vec3f(0.0, 0.007, 0.026) * (1.0 - y); }
        case 5u: { graded = (mix(vec3f(y), rgb, 1.2) - 0.5) * 1.08 + 0.5; }
        default: { graded = mix(vec3f(y), rgb, 0.65) * 0.86 + vec3f(0.11, 0.10, 0.105); }
    }
    return vec4f(mix(rgb, clamp(graded, vec3f(0.0), vec3f(1.0)), clamp(params.strength, 0.0, 1.0)), base.a);
}
