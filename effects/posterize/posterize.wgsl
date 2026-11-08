// Original mypaper effect. SPDX-License-Identifier: MIT
fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let base = sample_src(uv);
    let rgb = to_linear(clamp(base.rgb, vec3f(0.0), vec3f(1.0)));
    // Oklab separates perceived lightness from hue. Independent RGB quantization
    // made neighboring sky tones jump to unrelated green and magenta bands.
    let cone = pow(max(vec3f(
        dot(rgb, vec3f(0.41222147, 0.53633254, 0.05144599)),
        dot(rgb, vec3f(0.21190350, 0.68069955, 0.10739696)),
        dot(rgb, vec3f(0.08830246, 0.28171884, 0.62997870))
    ), vec3f(0.0)), vec3f(1.0 / 3.0));
    let lab = vec3f(
        dot(cone, vec3f(0.21045426, 0.79361779, -0.00407205)),
        dot(cone, vec3f(1.97799850, -2.42859221, 0.45059371)),
        dot(cone, vec3f(0.02590404, 0.78277177, -0.80867577))
    );
    let levels = max(round(clamp(params.levels, 2.0, 24.0)) - 1.0, 1.0);
    let dither = (fract(0.75487766 * floor(frag.x) + 0.56984029 * floor(frag.y)) - 0.5) * clamp(params.dither, 0.0, 1.0);
    let lightness = clamp(floor(lab.x * levels + 0.5 + dither) / levels, 0.0, 1.0);
    let c = vec3f(lightness) + vec3f(
        0.39633778 * lab.y + 0.21580376 * lab.z,
        -0.10556135 * lab.y - 0.06385417 * lab.z,
        -0.08948418 * lab.y - 1.29148555 * lab.z
    );
    let cube = c * c * c;
    let toned = to_display(vec3f(
        dot(cube, vec3f(4.07674166, -3.30771159, 0.23096993)),
        dot(cube, vec3f(-1.26843800, 2.60975740, -0.34131940)),
        dot(cube, vec3f(-0.00419609, -0.70341861, 1.70761470))
    ));
    return vec4f(mix(base.rgb, clamp(toned, vec3f(0.0), vec3f(1.0)), clamp(params.strength, 0.0, 1.0)), base.a);
}
