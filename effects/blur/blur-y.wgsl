// Separable Gaussian blur, vertical half (see blur-x.wgsl).

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let step = vec2f(0.0, params.radius * 0.006);
    let w = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var sum = sample_src(uv).rgb * w[0];
    for (var i = 1; i < 5; i++) {
        let o = step * f32(i);
        sum += (sample_src(uv + o).rgb + sample_src(uv - o).rgb) * w[i];
    }
    return vec4f(sum, 1.0);
}
