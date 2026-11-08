// Rain on glass, pass 2 of 2 (full resolution): beads and sliding drops on a fogged window.
// `src` is the drop field from glass.wgsl (r = mask, g = trail); the scene is `layer_input`.
// The glass between drops is fogged (a soft blur); drops refract the scene behind them.

// Four bilinear taps on the diagonals: a soft blur of about `radius` (in screen heights).
fn fogged(uv: vec2f, radius: f32) -> vec3f {
    let r = vec2f(radius * paper.resolution.y / paper.resolution.x, radius);
    var sum = sample_layer_input(uv + vec2f(r.x, r.y)).rgb;
    sum += sample_layer_input(uv + vec2f(-r.x, r.y)).rgb;
    sum += sample_layer_input(uv + vec2f(r.x, -r.y)).rgb;
    sum += sample_layer_input(uv + vec2f(-r.x, -r.y)).rgb;
    return sum * 0.25;
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let here = sample_src(uv).rg;
    // The drop's slope from screen derivatives of the upsampled mask (per full-res pixel).
    // dpdy runs down the screen while the drop field runs up, so -dpdy is the y-up slope.
    // Taken before the branches below: derivatives need uniform control flow.
    let slope = vec2f(dpdx(here.x), -dpdy(here.x)) * 1.5;
    let clear = clamp(here.x + here.y, 0.0, 1.0);
    // Most of the glass is either fully fogged or fully clear: sample only what shows.
    var fog = vec3f(0.0);
    if clear < 1.0 {
        fog = fogged(uv, params.fog * 0.004) * (1.0 - params.fog * 0.15);
    }
    var sharp = vec3f(0.0);
    if clear > 0.0 {
        sharp = sample_layer_input(uv + vec2f(slope.x, -slope.y) * 0.5).rgb;
    }
    var color = mix(fog, sharp, clear);
    // A uniform branch: the default white tint skips six pows per pixel.
    if any(params.tint.rgb != vec3f(1.0)) {
        color = to_display(to_linear(color) * params.tint.rgb);
    }
    return vec4f(color, 1.0);
}
