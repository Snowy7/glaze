// Draws one NV12 video frame over the whole target: luma (R8) and chroma
// (RG8) planes, converted to RGB with the video's own matrix. `scale` and
// `offset` map screen UV to frame UV exactly like blit.wgsl; outside the
// frame is black.

struct Video {
    scale: vec2f,
    offset: vec2f,
    // Rows of the affine YUV -> RGB transform: channel = dot(row, (y, u, v, 1)).
    r: vec4f,
    g: vec4f,
    b: vec4f,
}

@group(0) @binding(0) var<uniform> video: Video;
@group(0) @binding(1) var luma: texture_2d<f32>;
@group(0) @binding(2) var chroma: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(p.x, 1.0 - p.y);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4f {
    let uv = in.uv * video.scale + video.offset;
    let inside = all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0));
    let yuv = vec4f(
        textureSampleLevel(luma, samp, uv, 0.0).r,
        textureSampleLevel(chroma, samp, uv, 0.0).rg,
        1.0,
    );
    let rgb = saturate(vec3f(dot(video.r, yuv), dot(video.g, yuv), dot(video.b, yuv)));
    return vec4f(select(vec3f(0.0), rgb, inside), 1.0);
}
