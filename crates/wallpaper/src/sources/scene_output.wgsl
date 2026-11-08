struct Globals { view_proj:mat4x4f,model:mat4x4f,eye:vec4f,light:vec4f,background:vec4f,horizon:vec4f,clip:vec4f }
@group(0) @binding(0) var<uniform> g:Globals;
@group(1) @binding(0) var hardware_depth:texture_depth_2d;
@group(1) @binding(1) var color:texture_2d<f32>;
@group(1) @binding(2) var samp:sampler;
struct Out { @builtin(position) pos:vec4f,@location(0) uv:vec2f }
@vertex fn vs(@builtin(vertex_index)i:u32)->Out{let p=vec2f(f32((i<<1u)&2u),f32(i&2u));var o:Out;o.pos=vec4f(p*2.0-1.0,0.0,1.0);o.uv=vec2f(p.x,1.0-p.y);return o;}
@fragment fn fs_depth(i:Out)->@location(0)f32{let dimensions=textureDimensions(hardware_depth);let xy=min(vec2u(i.uv*vec2f(dimensions)),dimensions-1u);let z=textureLoad(hardware_depth,vec2i(xy),0);let near=g.clip.x;let far=g.clip.y;let linear=near*far/(far-z*(far-near));return select(clamp((linear-near)/(far-near),0.0,1.0),1.0,z>=1.0);}
@fragment fn fs_color(i:Out)->@location(0)vec4f{return textureSampleLevel(color,samp,i.uv,0.0);}
