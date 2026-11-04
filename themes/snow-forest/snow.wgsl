fn Sin(x: f32) -> f32 { return sin(x-floor(x/6.28318530718+0.5)*6.28318530718); }
// Andrew Baldwin (2013), https://www.shadertoy.com/view/ldsGDn
// Rocksdanister photo adaptation. CC BY-NC-SA 3.0.
// https://creativecommons.org/licenses/by-nc-sa/3.0/
// WGSL port by mypaper contributors. Original GLSL in shaders/snow.frag.
@group(0) @binding(1) var photo: texture_2d<f32>;
@group(0) @binding(2) var photo_sampler: sampler;
fn rand(co: vec2f) -> f32 { return fract(Sin(dot(co,vec2f(12.9898,78.233)))*43758.5453); }
fn wallpaper(frag: vec2f) -> vec4f {
    // The original snow page passed CSS resolution with a 0.65 canvas. Keep its visible crop
    // and flake density, rather than silently changing the appearance during the port.
    let res = paper.resolution/0.65;
    let coord = vec2f(frag.x,paper.resolution.y-frag.y);
    let p = mat3x3f(vec3f(13.323122,23.5112,21.71123),vec3f(21.1212,28.7312,11.9312),vec3f(21.8112,14.7212,61.3934));
    let uv = vec2f(1.0,res.y/res.x)*coord/res;
    var acc = vec3f(0.0);
    let dof = 5.0*Sin(paper.time*0.1);
    for(var i=0;i<50;i++) {
        let fi = f32(i);
        var q = uv*(1.0+fi*0.5);
        q += vec2f(q.y*(0.3*fract(fi*7.238917)-0.15),0.6*paper.time/(1.0+fi*0.5*0.03));
        let n = vec3f(floor(q),31.189+fi);
        let m = floor(n)*0.00001+fract(n);
        let r = fract((31415.9+m)/fract(p*m));
        var s = abs(fract(q)-0.5+0.9*r.xy-0.45);
        s += 0.01*abs(2.0*fract(10.0*q.yx)-1.0);
        let d = 0.6*max(s.x-s.y,s.x+s.y)+max(s.x,s.y)-0.01;
        let edge = 0.005+0.05*min(0.5*abs(fi-5.0-dof),1.0);
        let t = clamp((edge-d)/(2.0*edge),0.0,1.0);
        acc += vec3f(t*t*(3.0-2.0*t)*(r.x/(1.0+0.02*fi*0.5)));
    }
    var UV = coord/res;
    let image = vec2f(textureDimensions(photo));
    let sa = res.x/res.y;
    let ta = image.x/image.y;
    UV = select(vec2f(1.0,ta/sa),vec2f(sa/ta,1.0),ta>sa)*(UV-0.5)+0.5;
    let jitter = 0.3/100.0*(rand(UV)-0.5);
    let at = UV+jitter;
    let col = textureSample(photo,photo_sampler,vec2f(at.x,1.0-at.y)).rgb*mix(vec3f(0.75),vec3f(0.8,0.9,1.3),0.5);
    return vec4f(acc+col,1.0);
}
