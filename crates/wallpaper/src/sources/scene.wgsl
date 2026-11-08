// Metallic-roughness glTF, GGX sun and analytic hemisphere environment.
struct Globals {
    view_proj:mat4x4f, model:mat4x4f, eye:vec4f, light:vec4f,
    background:vec4f, horizon:vec4f, clip:vec4f,
}
struct Material { base:vec4f, emissive:vec4f, surface:vec4f, alpha:vec4f,uv_sets:vec4u,uv_extra:vec4u }
@group(0) @binding(0) var<uniform> g:Globals;
@group(1) @binding(0) var<uniform> m:Material;
@group(1) @binding(1) var samp:sampler;
@group(1) @binding(2) var base_tex:texture_2d<f32>;
@group(1) @binding(3) var mr_tex:texture_2d<f32>;
@group(1) @binding(4) var normal_tex:texture_2d<f32>;
@group(1) @binding(5) var emissive_tex:texture_2d<f32>;
@group(1) @binding(6) var ao_tex:texture_2d<f32>;
@group(1) @binding(7) var mr_samp:sampler;
@group(1) @binding(8) var normal_samp:sampler;
@group(1) @binding(9) var emissive_samp:sampler;
@group(1) @binding(10) var ao_samp:sampler;
fn encode(c:vec3f)->vec3f{return select(1.055*pow(max(c,vec3f(0.0)),vec3f(1.0/2.4))-0.055,c*12.92,c<=vec3f(0.0031308));}
fn filmic(c:vec3f)->vec3f{let x=c*g.light.w;return clamp((x*(2.51*x+0.03))/(x*(2.43*x+0.59)+0.14),vec3f(0.0),vec3f(1.0));}
struct SkyOut { @builtin(position) pos:vec4f, @location(0) uv:vec2f }
@vertex fn vs_sky(@builtin(vertex_index)i:u32)->SkyOut{let p=vec2f(f32((i<<1u)&2u),f32(i&2u));var o:SkyOut;o.pos=vec4f(p*2.0-1.0,1.0,1.0);o.uv=vec2f(p.x,1.0-p.y);return o;}
@fragment fn fs_sky(i:SkyOut)->@location(0)vec4f{let sky=mix(g.background.rgb,g.horizon.rgb,smoothstep(0.0,1.0,i.uv.y));return vec4f(encode(select(sky,g.background.rgb,g.background.w>0.5)),1.0);}
struct VsIn { @location(0) pos:vec3f,@location(1) normal:vec3f,@location(2) uv:vec2f,@location(3) uv1:vec2f,@location(4) color:vec4f }
struct VsOut { @builtin(position) pos:vec4f,@location(0) world:vec3f,@location(1) normal:vec3f,@location(2) uv:vec2f,@location(3) uv1:vec2f,@location(4) color:vec4f }
@vertex fn vs_main(i:VsIn)->VsOut{var o:VsOut;let world=g.model*vec4f(i.pos,1.0);o.pos=g.view_proj*world;o.world=world.xyz;o.normal=(g.model*vec4f(i.normal,0.0)).xyz;o.uv=i.uv;o.uv1=i.uv1;o.color=i.color;return o;}
fn fresnel(f0:vec3f,vh:f32)->vec3f{return f0+(1.0-f0)*pow(1.0-vh,5.0);}
@fragment fn fs_nearest(i:VsOut,@builtin(front_facing)front:bool)->@location(0)vec4f{
    let alpha=textureSample(base_tex,samp,select(i.uv,i.uv1,m.uv_sets.x==1u)).a*m.base.a*i.color.a;
    if alpha<0.01||(!front&&m.alpha.z<0.5){discard;}
    return vec4f(0.0);
}
@fragment fn fs_main(i:VsOut,@builtin(front_facing)front:bool)->@location(0)vec4f{
    // Sample before alpha discard so derivatives remain uniform.
    let base=textureSample(base_tex,samp,select(i.uv,i.uv1,m.uv_sets.x==1u))*m.base*i.color;
    let mr=textureSample(mr_tex,mr_samp,select(i.uv,i.uv1,m.uv_sets.y==1u));
    let normal_uv=select(i.uv,i.uv1,m.uv_sets.z==1u);
    let map=textureSample(normal_tex,normal_samp,normal_uv).xyz*2.0-1.0;
    let emissive=textureSample(emissive_tex,emissive_samp,select(i.uv,i.uv1,m.uv_sets.w==1u)).rgb*m.emissive.rgb;
    let ao=mix(1.0,textureSample(ao_tex,ao_samp,select(i.uv,i.uv1,m.uv_extra.x==1u)).r,m.surface.w);
    var n=normalize(i.normal);n=select(-n,n,front);
    // Cotangent frame also covers models with no exported tangents.
    let p1=dpdx(i.world);let p2=dpdy(i.world);let uv1=dpdx(normal_uv);let uv2=dpdy(normal_uv);
    let t=cross(p2,n)*uv1.x+cross(n,p1)*uv2.x;
    let b=cross(p2,n)*uv1.y+cross(n,p1)*uv2.y;
    let inv=inverseSqrt(max(max(dot(t,t),dot(b,b)),1e-12));
    if dot(t,t)+dot(b,b)>1e-10{n=normalize(mat3x3f(t*inv,b*inv,n)*vec3f(map.xy*m.surface.z,map.z));}
    if (!front&&m.alpha.z<0.5)||(m.alpha.x>0.5&&m.alpha.x<1.5&&base.a<m.alpha.y){discard;}
    let rough=clamp(m.surface.y*mr.g,0.045,1.0);let metal=clamp(m.surface.x*mr.b,0.0,1.0);
    let v=normalize(g.eye.xyz-i.world);let l=g.light.xyz;let h=normalize(v+l);
    let nl=max(dot(n,l),0.0);let nv=max(dot(n,v),0.001);let nh=max(dot(n,h),0.0);let vh=max(dot(v,h),0.0);
    let f0=mix(vec3f(0.04),base.rgb,metal);let f=fresnel(f0,vh);
    let a=rough*rough;let a2=a*a;let denom=nh*nh*(a2-1.0)+1.0;
    let d=a2/max(3.14159265*denom*denom,0.0001);
    let k=(rough+1.0)*(rough+1.0)/8.0;
    let geometry=(nl/(nl*(1.0-k)+k))*(nv/(nv*(1.0-k)+k));
    let spec=d*geometry*f/max(4.0*nl*nv,0.0001);
    let diffuse=(1.0-f)*(1.0-metal)*base.rgb/3.14159265;
    let direct=(diffuse+spec)*vec3f(3.4,3.15,2.8)*nl;
    let hemi=mix(vec3f(0.09,0.075,0.065),vec3f(0.48,0.58,0.78),n.y*0.5+0.5);
    let reflected=reflect(-v,n);
    let environment=mix(vec3f(0.12,0.10,0.08),vec3f(0.65,0.78,1.0),smoothstep(-0.3,0.8,reflected.y));
    let ambient=(hemi*base.rgb*(1.0-metal)+environment*fresnel(f0,nv)*(1.0-rough*0.6))*ao;
    let alpha=select(1.0,base.a,m.alpha.x>1.5);
    return vec4f(encode(filmic(direct+ambient+emissive))*alpha,alpha);
}
