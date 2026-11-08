// Heartfelt row/profile cache, Glaze adaptation of Rocksdanister / BigWings.
// CC BY-NC-SA 3.0: https://creativecommons.org/licenses/by-nc-sa/3.0/
// Float32 data, generated once on the same GPU as the live shader. No quantization.
fn Sin(x: f32) -> f32 { return sin(x-floor(x/6.28318530718+0.5)*6.28318530718); }
fn Cos(x: f32) -> f32 { return cos(x-floor(x/6.28318530718+0.5)*6.28318530718); }
fn N(x: f32) -> f32 { return fract(Sin(x*12345.564)*7658.76); }
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p=vec2f(f32((i<<1u)&2u),f32(i&2u));return vec4f(p*2.0-1.0,0,1);
}
@fragment fn fs(@builtin(position) px: vec4f) -> @location(0) vec4f {
    let i=floor(px.x);
    let y=i*(80.0/8191.0)-40.0;
    let wave=y+Sin(y);
    return vec4f(Sin(wave),Cos(wave)*(1.0+Cos(y)),N(i-4096.0),0);
}
