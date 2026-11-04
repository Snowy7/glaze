// Clear glass: inward convex bevel, opposite specular cones, adaptive face, no shadow.
struct Uniforms { size: vec2f, _pad: vec2f };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var behind: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct Plate {
    @location(0) rect: vec4f,
    @location(1) shape: vec4f, // radius, clarity, refraction, highlight
    @location(2) tint: vec4f,
    @location(3) extra: vec4f, // blur px
};
struct Out {
    @builtin(position) pos: vec4f,
    @location(0) px: vec2f,
    @location(1) @interpolate(flat) rect: vec4f,
    @location(2) @interpolate(flat) shape: vec4f,
    @location(3) @interpolate(flat) tint: vec4f,
    @location(4) @interpolate(flat) extra: vec4f,
};
@vertex fn vs(@builtin(vertex_index) vi: u32, p: Plate) -> Out {
    let corners = array<vec2f,6>(vec2f(0,0),vec2f(1,0),vec2f(0,1),vec2f(0,1),vec2f(1,0),vec2f(1,1));
    let half=p.rect.zw*0.5;
    let r=min(p.shape.x,min(half.x,half.y));
    let band=max(min(min(half.x,half.y)*0.28,r*0.65+5.0),1.0);
    let outer=half+0.5;
    let inner=max(half-band,vec2f(0));
    let quad=vi/6u;
    var lo=-outer;var hi=outer;
    if quad==0u { hi.y=-inner.y; }
    else if quad==1u { lo.y=inner.y; }
    else if quad==2u { lo.y=-inner.y;hi=vec2f(-inner.x,inner.y); }
    else if quad==3u { lo=vec2f(inner.x,-inner.y);hi.y=inner.y; }
    else if p.shape.y>=0.999 && p.extra.x<=0.75 && p.tint.a==0.0 { lo=vec2f(0);hi=vec2f(0); }
    else { lo=-inner;hi=inner; }
    let px=p.rect.xy+half+mix(lo,hi,corners[vi%6u]);
    let extra=vec4f(p.extra.xyz,select(0.0,1.0,quad<4u));
    return Out(vec4f(px.x/u.size.x*2.0-1.0,1.0-px.y/u.size.y*2.0,0,1),px,p.rect,p.shape,p.tint,extra);
}
fn tap(px: vec2f) -> vec3f { return textureSampleLevel(behind,samp,px/u.size,0).rgb; }
fn luma(c: vec3f) -> f32 { return dot(c,vec3f(0.2126,0.7152,0.0722)); }
@fragment fn fs(v: Out) -> @location(0) vec4f {
    if v.extra.w<0.5 && v.shape.y>=0.999 && v.extra.x<=0.75 { return vec4f(v.tint.rgb,v.tint.a*0.25); }
    let half = v.rect.zw*0.5;
    let p = v.px-v.rect.xy-half;
    let r = min(v.shape.x,min(half.x,half.y));
    let q = abs(p)-half+r;
    let z = max(q,vec2f(0));
    // Fourth-power corner profile joins the straight sides continuously.
    let d = sqrt(sqrt(dot(z*z,z*z)))+min(max(q.x,q.y),0.0)-r;
    let cover = clamp(0.5-d,0,1);
    if cover <= 0 { discard; }
    let depth = max(-d,0);
    let band = max(min(min(half.x,half.y)*0.28,r*0.65+5.0),1.0);
    // Clear centres retain the restored wallpaper. Its bilinear footprint supplies the
    // subpixel blur; no backdrop fetch or adaptive recipe is needed in this region.
    if depth >= band && v.shape.y >= 0.999 && v.extra.x <= 0.75 {
        return vec4f(v.tint.rgb,v.tint.a*0.25);
    }
    var g = select(vec2f(0,1),vec2f(1,0),q.x>q.y);
    if q.x>0 && q.y>0 { g = normalize(q*q*q); }
    let n = g*select(vec2f(-1),vec2f(1),p>=vec2f(0));
    let t = clamp(1.0-depth/band,0,1);
    let cap = 1.0-sqrt(max(1.0-t*t,0));
    let shift = cap*band*v.shape.z;
    let at = v.px-n*shift;
    var c = tap(at);
    if shift > 0.5 { c = vec3f(tap(at+n*shift*0.025).r,c.g,tap(at-n*shift*0.025).b); }
    let k = v.extra.x*0.7;
    if k > 0.6 && k < 1.5 {
        c = (2.0*c+tap(at+vec2f(k,k))+tap(at-vec2f(k,k)))/4.0;
    } else if k >= 1.5 {
        c = (2.0*c+tap(at+vec2f(k,k))+tap(at+vec2f(-k,k))+tap(at+vec2f(k,-k))+tap(at-vec2f(k,k)))/6.0;
    }
    let clarity = v.shape.y;
    let adaptive = 1.0-clarity;
    let y = luma(c);
    // Clear lightly dims bright media; tinted compresses luminance and preserves detail.
    let dark_tint = luma(v.tint.rgb)<0.5;
    let face = select(vec3f(0.45)+c*0.55,c*0.60+0.06,dark_tint);
    c = mix(c*(1.0-0.18*smoothstep(0.45,0.95,y)*clarity),face,adaptive*0.65);
    let tint = v.tint.rgb*(0.65+0.35*y);
    c = mix(c,tint,v.tint.a*(0.25+0.75*adaptive));
    if depth >= band { return vec4f(c,cover); }
    let light = normalize(vec2f(-0.707,-0.707));
    let key = pow(max(dot(n,light),0),3.0);
    let fill = pow(max(dot(n,-light),0),3.0)*0.58;
    let line = exp(-depth*depth/0.65);
    let sheen = exp(-depth/max(band*0.25,1.0));
    let glint = (line*(key+fill)+sheen*(key+fill)*0.07)*v.shape.w;
    let colour = mix(vec3f(1),c,0.25);
    c += colour*glint*0.72;
    return vec4f(c,cover);
}
