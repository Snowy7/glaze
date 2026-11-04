// Star Nest — Pablo Román Andrioli (Kali).
// Original: https://www.shadertoy.com/view/XlfGRj
// Licensed source: https://defold.com/tutorials/shadertoy/#the-star-nest-shader
// MIT: https://opensource.org/license/mit/ (full notice in themes/licenses/).
// Changes: WGSL/paper uniforms, y-up coordinates, aspect-correct framing,
// GLSL mod(x,y) expanded as x-y*floor(x/y). Defold's fixed mouse orientation
// and all iteration counts, colours, motion and fractal constants retained.

fn wallpaper(frag: vec2f) -> vec4f {
    var uv = vec2f(frag.x, paper.resolution.y-frag.y)/paper.resolution-0.5;
    uv.y *= paper.resolution.y/paper.resolution.x;
    var direction = vec3f(uv*0.8, 1.0);
    let time = paper.time*0.01;
    let a1 = 0.5;
    let a2 = 0.8;
    let rot1 = mat2x2f(vec2f(cos(a1), sin(a1)), vec2f(-sin(a1), cos(a1)));
    let rot2 = mat2x2f(vec2f(cos(a2), sin(a2)), vec2f(-sin(a2), cos(a2)));
    let dxz = direction.xz*rot1;
    direction = vec3f(dxz.x, direction.y, dxz.y);
    let dxy = direction.xy*rot2;
    direction = vec3f(dxy, direction.z);
    var origin = vec3f(1.0, 0.5, 0.5)+vec3f(time*2.0, time, -2.0);
    let oxz = origin.xz*rot1;
    origin = vec3f(oxz.x, origin.y, oxz.y);
    let oxy = origin.xy*rot2;
    origin = vec3f(oxy, origin.z);
    var distance = 0.1;
    var fade = 1.0;
    var color = vec3f(0.0);
    for (var r = 0; r < 20; r++) {
        var p = origin+distance*direction*0.5;
        p = abs(vec3f(0.85)-(p-1.7*floor(p/1.7)));
        var previous = 0.0;
        var amount = 0.0;
        for (var i = 0; i < 17; i++) {
            p = abs(p)/dot(p, p)-0.53;
            amount += abs(length(p)-previous);
            previous = length(p);
        }
        let dark = max(0.0, 0.3-amount*amount*0.001);
        amount *= amount*amount;
        if (r > 6) { fade *= 1.0-dark; }
        color += fade;
        color += vec3f(distance, distance*distance, pow(distance, 4.0))*amount*0.0015*fade;
        fade *= 0.73;
        distance += 0.1;
    }
    color = mix(vec3f(length(color)), color, 0.85);
    return vec4f(color*0.01, 1.0);
}
