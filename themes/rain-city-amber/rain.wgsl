fn N21(input: vec2f) -> f32 { var p=fract(input*vec2f(123.34,345.45));p+=dot(p,p+34.345);return fract(p.x*p.y); }
// WGSL only guarantees sine accuracy in its reduced domain. Hash inputs reach far beyond it.
fn Sin(x: f32) -> f32 { return sin(x-floor(x/6.28318530718+0.5)*6.28318530718); }
// BigWings (Martijn Steinrucken), Heartfelt; Rocksdanister rain adaptation.
// CC BY-NC-SA 3.0: https://creativecommons.org/licenses/by-nc-sa/3.0/
// WGSL port by Glaze contributors. Original GLSL is preserved in shaders/rain.frag.
@group(0) @binding(1) var photo: texture_2d<f32>;
@group(0) @binding(2) var photo_sampler: sampler;
@group(0) @binding(3) var profile: texture_2d<f32>;
// The frame without drop refraction, baked once per render size (theme.json `bake`).
@group(0) @binding(5) var still: texture_2d<f32>;
// Smooth row functions are cached at 8192 samples across [-40,40]. Linear interpolation
// error is below a few hundredths of a final-screen pixel, with the same full-size rain field.
fn row_profile(y: f32) -> vec2f {
    let at=clamp((y+40.0)*(8191.0/80.0),0.0,8190.999);
    let i=i32(floor(at));
    return mix(textureLoad(profile,vec2i(i,0),0).xy,textureLoad(profile,vec2i(i+1,0),0).xy,fract(at));
}
fn column_noise(column: f32) -> f32 {
    return textureLoad(profile,vec2i(i32(column)+4096,0),0).z;
}
const RAIN: f32 = 0.72;
const SPEED: f32 = 0.32;
const BRIGHTNESS: f32 = 0.85;
const GRADE: vec3f = vec3f(1.12,0.82,0.60);
fn S(a: f32,b: f32,t: f32) -> f32 {
    let x = clamp((t-a)/select(b-a,0.000001,abs(b-a)<0.000001),0.0,1.0);
    return x*x*(3.0-2.0*x);
}
fn N13(p: f32) -> vec3f {
    var p3 = fract(vec3f(p)*vec3f(0.1031,0.11369,0.13787));
    p3 += dot(p3,p3.yzx+19.19);
    return fract(vec3f((p3.x+p3.y)*p3.z,(p3.x+p3.z)*p3.y,(p3.y+p3.z)*p3.x));
}
fn N(t: f32) -> f32 { return fract(Sin(t*12345.564)*7658.76); }
fn Saw(b: f32,t: f32) -> f32 { return S(0.0,b,t)*S(1.0,b,t); }
// Value and analytic UV gradient. Cell hashes are shared instead of evaluating all three
// Heartfelt fields again for finite differences; unlike quad derivatives this stays smooth.
fn Sd(a: f32,b: f32,x: f32) -> f32 {
    let t = clamp((x-a)/(b-a),0.0,1.0);
    return 6.0*t*(1.0-t)/(b-a);
}
fn Cos(x: f32) -> f32 { return cos(x-floor(x/6.28318530718+0.5)*6.28318530718); }
// A layer's value, and what its gradient needs: most pixels never need the gradient.
struct Layer {
    value: f32,
    st: vec2f,
    y: f32,
    dx_dv: f32,
    delta: vec2f,
    distance: f32,
    r: f32,
    front: f32,
    drop_delta: vec2f,
    drop_distance: f32,
    drops: f32,
}
fn DropLayer2(input: vec2f,t: f32) -> Layer {
    var uv = input;
    uv.y += t*0.75;
    let grid = vec2f(12.0,2.0);
    uv.y += column_noise(floor(uv.x*grid.x));
    let id = floor(uv*grid);
    let n = N13(id.x*35.2+id.y*2376.1);
    let st = fract(uv*grid)-vec2f(0.5,0.0);
    let base_x = n.x-0.5;
    let wiggle_y = input.y*20.0;
    let row = row_profile(wiggle_y);
    let amplitude = (0.5-abs(base_x))*(n.z-0.5)*0.7;
    let x = base_x*0.7+row.x*amplitude;
    let dx_dv = row.y*20.0*amplitude;
    let y = (Saw(0.85,fract(t+n.z))-0.5)*0.9+0.5;
    let delta = (st-vec2f(x,y))*vec2f(1.0,6.0);
    let distance = max(length(delta),0.000001);
    let main = S(0.4,0.0,distance);
    let r = sqrt(S(1.0,y,st.y));
    let front = S(-0.02,0.02,st.y-y);
    let drop_delta = vec2f(st.x-x,0.5-fract(input.y*10.0));
    let drop_distance = max(length(drop_delta),0.000001);
    let drops = S(0.3,0.0,drop_distance);
    return Layer(main+drops*r*front,st,y,dx_dv,delta,distance,r,front,drop_delta,drop_distance,drops);
}
fn LayerGradient(l: Layer) -> vec2f {
    let main_grad = Sd(0.4,0.0,l.distance)*vec2f(l.delta.x*12.0,l.delta.x*(-l.dx_dv)+l.delta.y*12.0)/l.distance;
    let dr_dv = Sd(1.0,l.y,l.st.y)/max(l.r,0.000001);
    let front_dv = Sd(-0.02,0.02,l.st.y-l.y)*2.0;
    let drops_grad = Sd(0.3,0.0,l.drop_distance)*vec2f(l.drop_delta.x*12.0,-l.drop_delta.x*l.dx_dv-l.drop_delta.y*10.0)/l.drop_distance;
    return main_grad+drops_grad*l.r*l.front+vec2f(0.0,l.drops*(dr_dv*l.front+l.r*front_dv));
}
// The static layer's value (x), and its gradient's fade-scaled shape (yz) and distance (w).
fn StaticDrops(input: vec2f,t: f32) -> vec4f {
    let uv = input*40.0;
    let id = floor(uv);
    let st = fract(uv)-0.5;
    let n = N13(id.x*107.45+id.y*3543.654);
    let delta = st-(n.xy-0.5)*0.7;
    let distance = max(length(delta),0.000001);
    let fade = fract(n.z*10.0)*Saw(0.025,fract(t+n.z));
    return vec4f(S(0.3,0.0,distance)*fade,Sd(0.3,0.0,distance)*fade*delta*40.0,distance);
}
// The refraction gradient of the combined field. It is exactly zero wherever the field's
// smoothstep is flat (outside 0.3..1), which is most pixels: only those compute layer gradients.
fn Drops(uv: vec2f,t: f32,l0: f32,l1: f32,l2: f32) -> vec2f {
    let s = StaticDrops(uv,t);
    let m1 = DropLayer2(uv,t);
    let m2 = DropLayer2(uv*1.85,t);
    let field = s.x*l0+m1.value*l1+m2.value*l2;
    let slope = Sd(0.3,1.0,field);
    if slope == 0.0 {
        return vec2f(0.0);
    }
    let gradient = s.yz/s.w*l0+LayerGradient(m1)*l1+LayerGradient(m2)*l2*1.85;
    return slope*gradient;
}
fn sample_photo(uv: vec2f) -> vec3f { return textureSample(photo,photo_sampler,vec2f(uv.x,1.0-uv.y)).rgb; }
// The web page always applied its 1.06 CSS scale, including before any pointer event.
fn css(frag: vec2f,res: vec2f) -> vec2f { return (vec2f(frag.x,res.y-frag.y)-res*0.5)/1.06+res*0.5; }
fn cover_uv(coord: vec2f,res: vec2f) -> vec2f {
    let UV = coord/res;
    let image = vec2f(textureDimensions(photo));
    let sa = res.x/res.y;
    let ta = image.x/image.y;
    let cover = select(vec2f(1.0,ta/sa),vec2f(sa/ta,1.0),ta>sa);
    return cover*(UV-0.5)+0.5;
}
// The photo seen through drop refraction n: all 18 original samples, including the centre,
// with the original /16 gain, then grading and vignette.
fn shade(coord: vec2f,UV: vec2f,n: vec2f) -> vec4f {
    var col = sample_photo(UV+n);
    let angle=N21(coord)*6.2831;
    var direction=vec2f(sin(angle),cos(angle));
    let disk=array<f32,17>(0.589455020,0.871941647,0.726960368,0.560964232,0.815803890,0.607816604,0.979664331,0.894431495,0.919477940,0.362277957,0.727700682,0.702685390,0.781555124,0.691411660,0.385012138,0.776420187,0.879505129);
    for(var i=0u;i<17u;i++) {
        col+=sample_photo(UV+n+direction*0.0065*disk[i]);
        direction=vec2f(direction.x*0.540302306+direction.y*0.841470985,direction.y*0.540302306-direction.x*0.841470985);
    }
    col=col/16.0*GRADE;

    col *= 1.0-dot(UV-0.5,UV-0.5);
    return vec4f(col*BRIGHTNESS,1.0);
}
// Baked: most of the screen has no refraction at any moment, and there it never changes.
@fragment fn still_frame(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let coord = css(pos.xy,paper.resolution);
    return shade(coord,cover_uv(coord,paper.resolution),vec2f(0.0));
}
fn wallpaper(frag: vec2f) -> vec4f {
    let res = paper.resolution;
    let coord = css(frag,res);
    var uv = (coord-res*0.5)/res.y;
    let t = paper.time*0.2*SPEED;
    uv *= 0.7*2.61;
    let l0 = S(-0.5,1.0,RAIN)*2.0;
    let l1 = S(0.25,0.75,RAIN);
    let l2 = S(0.0,0.5,RAIN);
    let n = Drops(uv,t,l0,l1,l2)*0.0005;
    // A 2x2 quad without refraction is the baked pixel exactly: the blur's implicit
    // derivatives see the same quad in both passes. Refracting quads draw the blur here.
    let refracts = select(0.0,1.0,any(n!=vec2f(0.0)));
    let row = refracts+abs(dpdxFine(refracts));
    if row+abs(dpdyFine(row)) == 0.0 {
        return textureLoad(still,vec2i(frag),0);
    }
    return shade(coord,cover_uv(coord,res),n);
}
