// Color grade: exposure, warmth and tint in light (linear), then contrast and saturation on the
// display values.

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    var c = to_linear(sample_src(uv).rgb) * exp2(params.exposure);
    c *= vec3f(1.0 + 0.12 * params.warmth, 1.0, 1.0 - 0.12 * params.warmth) * params.tint.rgb;
    var d = to_display(c);
    d = (d - 0.5) * params.contrast + 0.5;
    d = mix(vec3f(luma(d)), d, params.saturation);
    return vec4f(clamp(d, vec3f(0.0), vec3f(1.0)), 1.0);
}
