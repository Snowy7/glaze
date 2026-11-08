// Darkens (or tints) toward the corners, as an ellipse that follows the screen's shape.

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let c = sample_src(uv).rgb;
    let d = length((uv - 0.5) * 2.0) / sqrt(2.0);
    let inner = params.size * (1.0 - params.softness);
    let edge = smoothstep(inner, inner + params.softness + 1e-3, d) * params.amount;
    return vec4f(mix(c, to_display(params.color.rgb), edge), 1.0);
}
