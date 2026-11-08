// Glaze shader prelude, appended after every theme shader.
// A theme shader only has to define:
//
//     fn wallpaper(frag: vec2f) -> vec4f
//
// `frag` is the pixel position, top-left origin. `paper` holds the inputs.

struct Paper {
    resolution: vec2f, // screen size in pixels
    mouse: vec2f,      // cursor position in pixels (same space as frag)
    time: f32,         // seconds; stops while the wallpaper is hidden
    frame: u32,
    _pad: vec2f,
}

// Immediate data (DX12 root constants): set in the draw itself, so a frame uploads no buffer.
var<immediate> paper: Paper;

@vertex
fn glaze_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // One triangle that covers the whole screen.
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn glaze_fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    return wallpaper(pos.xy);
}
