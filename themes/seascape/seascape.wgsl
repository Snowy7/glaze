// "Seascape" — Alexander Alekseev (TDM), 2014.
// https://www.shadertoy.com/view/Ms2SD1
// Author's source: https://github.com/tdmaav/shadertoy/blob/master/Seascape.shader
// CC BY-NC-SA 3.0: https://creativecommons.org/licenses/by-nc-sa/3.0/
// NON-COMMERCIAL. This WGSL adaptation remains under the same license.
// Changes: WGSL syntax, paper uniforms, top-left -> bottom-left coordinates;
// reversed smoothstep expressed as its defined ascending equivalent. Original
// camera, colours, octaves, tracing and post-processing are retained (AA off).
// iMouse stays at its default zero to prevent cursor-driven camera snaps.

const PI: f32 = 3.141592;
const SEA_HEIGHT: f32 = 0.6;
const SEA_BASE = vec3f(0.0, 0.09, 0.18);
const SEA_WATER_COLOR = vec3f(0.8, 0.9, 0.6) * 0.6;
const OCTAVE = mat2x2f(vec2f(1.6, 1.2), vec2f(-1.2, 1.6));

fn from_euler(ang: vec3f) -> mat3x3f {
    let a1 = vec2f(sin(ang.x), cos(ang.x));
    let a2 = vec2f(sin(ang.y), cos(ang.y));
    let a3 = vec2f(sin(ang.z), cos(ang.z));
    return mat3x3f(
        vec3f(a1.y*a3.y+a1.x*a2.x*a3.x, a1.y*a2.x*a3.x+a3.y*a1.x, -a2.y*a3.x),
        vec3f(-a2.y*a1.x, a1.y*a2.y, a2.x),
        vec3f(a3.y*a1.x*a2.x+a1.y*a3.x, a1.x*a3.x-a1.y*a3.y*a2.x, a2.y*a3.y),
    );
}

fn sea_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453123);
}

fn sea_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f*f*(3.0-2.0*f);
    return -1.0 + 2.0 * mix(
        mix(sea_hash(i), sea_hash(i+vec2f(1.0, 0.0)), u.x),
        mix(sea_hash(i+vec2f(0.0, 1.0)), sea_hash(i+vec2f(1.0)), u.x), u.y,
    );
}

fn diffuse(n: vec3f, light: vec3f, exponent: f32) -> f32 {
    return pow(dot(n, light) * 0.4 + 0.6, exponent);
}

fn specular(n: vec3f, light: vec3f, eye: vec3f, exponent: f32) -> f32 {
    let norm = (exponent + 8.0) / (PI * 8.0);
    return pow(max(dot(reflect(eye, n), light), 0.0), exponent) * norm;
}

fn sky_color(eye: vec3f) -> vec3f {
    let y = (max(eye.y, 0.0)*0.8+0.2)*0.8;
    return vec3f(pow(1.0-y, 2.0), 1.0-y, 0.6+(1.0-y)*0.4)*1.1;
}

fn sea_octave(start: vec2f, choppy: f32) -> f32 {
    let uv = start + sea_noise(start);
    let wave = 1.0 - abs(sin(uv));
    let swave = abs(cos(uv));
    let mixed = mix(wave, swave, wave);
    return pow(1.0 - pow(mixed.x * mixed.y, 0.65), choppy);
}

fn sea_map(p: vec3f, iterations: i32) -> f32 {
    var frequency = 0.16;
    var amplitude = SEA_HEIGHT;
    var choppy = 4.0;
    var uv = p.xz * vec2f(0.75, 1.0);
    let time = 1.0 + paper.time * 0.8;
    var height = 0.0;
    for (var i = 0; i < iterations; i++) {
        let d = sea_octave((uv+time)*frequency, choppy)
            + sea_octave((uv-time)*frequency, choppy);
        height += d * amplitude;
        // The GLSL source uses a row vector: uv *= octave_m.
        uv = uv * OCTAVE;
        frequency *= 1.9;
        amplitude *= 0.22;
        choppy = mix(choppy, 1.0, 0.2);
    }
    return p.y - height;
}

fn sea_color(p: vec3f, n: vec3f, light: vec3f, eye: vec3f, distance: vec3f) -> vec3f {
    let f = clamp(1.0-dot(n, -eye), 0.0, 1.0);
    let fresnel = min(f*f*f, 0.5);
    let reflected = sky_color(reflect(eye, n));
    let refracted = SEA_BASE + diffuse(n, light, 80.0)*SEA_WATER_COLOR*0.12;
    var color = mix(refracted, reflected, fresnel);
    let attenuation = max(1.0-dot(distance, distance)*0.001, 0.0);
    color += SEA_WATER_COLOR*(p.y-SEA_HEIGHT)*0.18*attenuation;
    color += specular(n, light, eye, 600.0*inverseSqrt(dot(distance, distance)));
    return color;
}

fn sea_normal(p: vec3f, epsilon: f32) -> vec3f {
    let height = sea_map(p, 5);
    return normalize(vec3f(
        sea_map(p+vec3f(epsilon, 0.0, 0.0), 5)-height,
        epsilon,
        sea_map(p+vec3f(0.0, 0.0, epsilon), 5)-height,
    ));
}

fn trace_height(origin: vec3f, direction: vec3f) -> vec3f {
    var near = 0.0;
    var far = 1000.0;
    var high = sea_map(origin+direction*far, 3);
    if (high > 0.0) { return origin+direction*far; }
    var low = sea_map(origin, 3);
    var p = origin;
    for (var i = 0; i < 32; i++) {
        let middle = mix(near, far, low/(low-high));
        p = origin + direction*middle;
        let height = sea_map(p, 3);
        if (height < 0.0) {
            far = middle;
            high = height;
        } else {
            near = middle;
            low = height;
        }
        if (abs(height) < 0.001) { break; }
    }
    return p;
}

fn wallpaper(frag: vec2f) -> vec4f {
    let coord = vec2f(frag.x, paper.resolution.y-frag.y);
    var uv = coord/paper.resolution*2.0-1.0;
    uv.x *= paper.resolution.x/paper.resolution.y;
    let time = paper.time*0.3;
    let angles = vec3f(sin(time*3.0)*0.1, sin(time)*0.2+0.3, time);
    let origin = vec3f(0.0, 3.5, time*5.0);
    var direction = normalize(vec3f(uv, -2.0));
    direction.z += length(uv)*0.14;
    direction = normalize(direction)*from_euler(angles);
    let p = trace_height(origin, direction);
    let distance = p-origin;
    let n = sea_normal(p, dot(distance, distance)*(0.1/paper.resolution.x));
    let light = normalize(vec3f(0.0, 1.0, 0.8));
    let color = mix(sky_color(direction), sea_color(p, n, light, direction, distance),
        pow(1.0-smoothstep(-0.02, 0.0, direction.y), 0.2));
    return vec4f(pow(color, vec3f(0.65)), 1.0);
}
