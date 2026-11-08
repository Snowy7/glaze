// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let px = clamp(params.radius, 0.5, 12.0) / max(paper.resolution, vec2f(1.0));
    let blur = (sample_src(uv + vec2f(px.x, 0.0)).rgb + sample_src(uv - vec2f(px.x, 0.0)).rgb + sample_src(uv + vec2f(0.0, px.y)).rgb + sample_src(uv - vec2f(0.0, px.y)).rgb) * 0.25;
    let limit = vec3f(clamp(params.limit, 0.01, 0.3));
    let detail = clamp((base.rgb - blur) * clamp(params.amount, 0.0, 2.0), -limit, limit);
    return vec4f(clamp(base.rgb + detail, vec3f(0.0), vec3f(1.0)), base.a);
}
