// Snow — Copyright (c) 2013 Andrew Baldwin (baldand, http://thndl.com).
// https://www.shadertoy.com/view/ldsGDn
// Source distributed by Dani John (Rocksdanister):
// https://github.com/rocksdanister/snow/blob/main/shaders/snow.frag
// CC BY-NC-SA 3.0: https://creativecommons.org/licenses/by-nc-sa/3.0/
// NON-COMMERCIAL. This adaptation remains under the same license.
// Changes: WGSL, paper uniforms, y-up coordinates, fixed original light-snow
// parameters (50 layers). Lively's optional background texture/post-processing
// are omitted; Baldwin's snow is rendered on black. No new particle algorithm.

fn wallpaper(frag: vec2f) -> vec4f {
    let p = mat3x3f(vec3f(13.323122, 23.5112, 21.71123),
        vec3f(21.1212, 28.7312, 11.9312), vec3f(21.8112, 14.7212, 61.3934));
    let coord = vec2f(frag.x, paper.resolution.y-frag.y);
    let uv = vec2f(1.0, paper.resolution.y/paper.resolution.x)*coord/paper.resolution;
    var accumulated = 0.0;
    let dof = 5.0*sin(paper.time*0.1);
    for (var i = 0; i < 50; i++) {
        let fi = f32(i);
        var q = uv*(1.0+fi*0.5);
        q += vec2f(q.y*(0.3*fract(fi*7.238917)-0.3*0.5),
            0.6*paper.time/(1.0+fi*0.5*0.03));
        let n = vec3f(floor(q), 31.189+fi);
        let m = floor(n)*0.00001+fract(n);
        let r = fract((31415.9+m)/fract(p*m));
        var s = abs(fract(q)-0.5+0.9*r.xy-0.45);
        s += 0.01*abs(2.0*fract(10.0*q.yx)-1.0);
        let d = 0.6*max(s.x-s.y, s.x+s.y)+max(s.x, s.y)-0.01;
        let edge = 0.005+0.05*min(0.5*abs(fi-5.0-dof), 1.0);
        accumulated += (1.0-smoothstep(-edge, edge, d))*(r.x/(1.0+0.02*fi*0.5));
    }
    return vec4f(vec3f(accumulated), 1.0);
}
