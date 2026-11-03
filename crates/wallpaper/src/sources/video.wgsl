// Draws one 4:2:0 video frame (NV12 or P010) over the whole target: a luma
// plane (R8 or R16) and a chroma plane (RG8 or RG16), converted to RGB with
// the video's own matrix. HDR video (PQ or HLG) is then tone-mapped to SDR.
// `scale` and `offset` map screen UV to frame UV exactly like blit.wgsl;
// outside the frame is black.

struct Video {
    scale: vec2f,
    offset: vec2f,
    // Rows of the affine YUV -> RGB transform: channel = dot(row, (y, u, v, 1)).
    // They already account for the bit depth and P010's shifted samples.
    r: vec4f,
    g: vec4f,
    b: vec4f,
    // 0: SDR (shown as is), 1: PQ (ST 2084), 2: HLG.
    transfer: u32,
    // 1: the RGB primaries are BT.2020 (converted to BT.709 when tone-mapping).
    wide_gamut: u32,
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

// Reference white: the luminance (BT.2408) that SDR white 1.0 maps to.
const SDR_WHITE_NITS = 203.0;
// Below this (relative to SDR white) the tone map leaves light untouched.
const KNEE = 0.8;

// ST 2084 EOTF: encoded signal 0..1 to absolute light in nits.
fn pq_to_nits(e: vec3f) -> vec3f {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let p = pow(e, vec3f(1.0 / m2));
    return 10000.0 * pow(max(p - c1, vec3f(0.0)) / (c2 - c3 * p), vec3f(1.0 / m1));
}

// HLG: inverse OETF, then the OOTF of a 1000-nit display (system gamma 1.2),
// giving light in nits.
fn hlg_to_nits(e: vec3f) -> vec3f {
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    let scene = select((exp((e - c) / a) + b) / 12.0, e * e / 3.0, e <= vec3f(0.5));
    let luminance = dot(scene, vec3f(0.2627, 0.6780, 0.0593));
    return 1000.0 * pow(luminance, 0.2) * scene;
}

// Linear BT.2020 to linear BT.709 primaries.
fn bt2020_to_bt709(c: vec3f) -> vec3f {
    return vec3f(
        dot(vec3f(1.6605, -0.5876, -0.0728), c),
        dot(vec3f(-0.1246, 1.1329, -0.0083), c),
        dot(vec3f(-0.0182, -0.1006, 1.1187), c),
    );
}

// Soft-clips light above the knee, scaling all channels by the same factor so
// colours keep their hue. Linear below the knee, with a matching slope, then a
// Reinhard shoulder that approaches 1.0 but still separates bright highlights.
fn tone_map(c: vec3f) -> vec3f {
    let peak = max(c.r, max(c.g, c.b));
    if peak <= KNEE {
        return c;
    }
    let t = (peak - KNEE) / (1.0 - KNEE);
    let mapped = KNEE + (1.0 - KNEE) * t / (1.0 + t);
    return c * (mapped / peak);
}

fn srgb_encode(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3f(0.0031308));
}

// Encoded HDR RGB to encoded SDR RGB.
fn hdr_to_sdr(encoded: vec3f) -> vec3f {
    var nits = pq_to_nits(encoded);
    if video.transfer == 2u {
        nits = hlg_to_nits(encoded);
    }
    var linear = nits / SDR_WHITE_NITS;
    if video.wide_gamut == 1u {
        linear = bt2020_to_bt709(linear);
    }
    return srgb_encode(tone_map(max(linear, vec3f(0.0))));
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
    var rgb = saturate(vec3f(dot(video.r, yuv), dot(video.g, yuv), dot(video.b, yuv)));
    if video.transfer != 0u {
        rgb = hdr_to_sdr(rgb);
    }
    return vec4f(select(vec3f(0.0), rgb, inside), 1.0);
}
