// Keep the photograph sharp while the half-resolution flake field softens out-of-focus snow.
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_layer_input(uv);
    let flakes = sample_src(uv).r;
    return vec4f(mix(base.rgb, vec3f(0.88, 0.94, 1.0), flakes), base.a);
}
