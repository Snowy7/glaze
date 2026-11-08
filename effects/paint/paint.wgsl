// Original Glaze effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = clamp(params.radius, 1.0, 12.0) / max(paper.resolution, vec2f(1.0)) * paper.resolution.y / 1080.0;
    var chosen = base.rgb;
    var minimum = 1000.0;
    for (var quadrant = 0u; quadrant < 4u; quadrant++) {
        let direction = vec2f(select(-1.0, 1.0, (quadrant & 1u) != 0u), select(-1.0, 1.0, (quadrant & 2u) != 0u));
        var mean = vec3f(0.0);
        var square = vec3f(0.0);
        for (var y = 0u; y < 3u; y++) { for (var x = 0u; x < 3u; x++) {
                let rgb = sample_src(uv + vec2f(f32(x), f32(y)) * direction * px).rgb;
                mean += rgb;
                square += rgb * rgb;
        }}
        mean /= 9.0;
        let variance = abs(square / 9.0 - mean * mean);
        let score = variance.r + variance.g + variance.b;
        if score < minimum { minimum = score; chosen = mean; }
    }
    return vec4f(chosen, base.a);
}
