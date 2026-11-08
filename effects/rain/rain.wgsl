// Rain on glass: beads that sit and fade, plus drops that slide down leaving clear trails.
// The glass between them is fogged (a soft blur); drops refract the scene behind them.
// Works in a y-up space centred on the screen, in screen heights, so drops keep their size
// at every resolution and pass scale.

fn n31(p: f32) -> vec3f {
    var q = fract(vec3f(p) * vec3f(0.1031, 0.11369, 0.13787));
    q += dot(q, q.yzx + 19.19);
    return fract(vec3f((q.x + q.y) * q.z, (q.x + q.z) * q.y, (q.y + q.z) * q.x));
}

// Rises fast to 1 at `b`, then falls slowly back to 0.
fn saw(b: f32, t: f32) -> f32 {
    return smoothstep(0.0, b, t) * smoothstep(1.0, b, t);
}

// Beads: one per cell, fading in and out.
fn beads(p: vec2f, t: f32) -> f32 {
    let q = p * 40.0;
    let id = floor(q);
    let f = fract(q) - 0.5;
    let n = n31(id.x * 107.45 + id.y * 3543.654);
    let d = length(f - (n.xy - 0.5) * 0.7);
    return smoothstep(0.3, 0.0, d) * fract(n.z * 10.0) * saw(0.025, fract(t + n.z));
}

// Sliding drops: x = drop mask, y = its trail (where the fog is wiped off).
fn sliding(p_in: vec2f, t: f32) -> vec2f {
    let aspect = vec2f(2.0, 1.0);
    let grid = aspect * 2.0;
    var p = p_in;
    p.y += t * 0.75;
    let column = floor(p.x * grid.x);
    p.y += n31(column).x;
    let id = floor(p * grid);
    let n = n31(id.x * 35.2 + id.y * 2376.1);
    let cell = fract(p * grid) - vec2f(0.5, 0.0);
    let wobble = p_in.y * 20.0;
    var x = n.x - 0.5;
    x += sin(wobble + sin(wobble)) * (0.5 - abs(x)) * (n.z - 0.5);
    x *= 0.7;
    let y = (saw(0.85, fract(t + n.z)) - 0.5) * 0.9 + 0.5;
    let head = smoothstep(0.4, 0.0, length((cell - vec2f(x, y)) * aspect.yx));
    let behind = sqrt(smoothstep(1.0, y, cell.y));
    let across = abs(cell.x - x);
    let front = smoothstep(-0.02, 0.02, cell.y - y);
    var trail = smoothstep(0.23 * behind, 0.15 * behind * behind, across) * front * behind * behind;
    // Small beads left behind in the trail.
    let left = fract(p_in.y * 10.0) + (cell.y - 0.5);
    let crumbs = smoothstep(0.3, 0.0, length(cell - vec2f(x, left))) * behind * front;
    if params.streaks == 0u {
        trail = 0.0;
    }
    return vec2f(head + crumbs * f32(params.streaks), trail);
}

fn glass(p: vec2f, t: f32, amount: f32) -> vec2f {
    let bead = beads(p, t) * smoothstep(-0.5, 1.0, amount) * 2.0;
    let a = sliding(p, t) * smoothstep(0.25, 0.75, amount);
    let b = sliding(p * 1.85, t) * smoothstep(0.0, 0.5, amount);
    let mask = smoothstep(0.3, 1.0, bead + a.x + b.x);
    return vec2f(mask, max(a.y, b.y));
}

fn fogged(uv: vec2f, radius: f32) -> vec3f {
    let r = vec2f(radius * paper.resolution.y / paper.resolution.x, radius);
    var sum = sample_src(uv).rgb * 0.2;
    sum += sample_src(uv + vec2f(r.x, r.y)).rgb * 0.2;
    sum += sample_src(uv + vec2f(-r.x, r.y)).rgb * 0.2;
    sum += sample_src(uv + vec2f(r.x, -r.y)).rgb * 0.2;
    sum += sample_src(uv + vec2f(-r.x, -r.y)).rgb * 0.2;
    return sum;
}

fn effect(frag: vec2f, uv: vec2f) -> vec4f {
    let res = paper.resolution;
    let t = paper.time * 0.2 * params.speed;
    let p = vec2f(frag.x - 0.5 * res.x, 0.5 * res.y - frag.y) / res.y / params.size;
    let here = glass(p, t, params.amount);
    let e = 1.5 / res.y / params.size;
    let dx = glass(p + vec2f(e, 0.0), t, params.amount).x - here.x;
    let dy = glass(p + vec2f(0.0, e), t, params.amount).x - here.x;
    // y-up normal back to the y-down uv.
    let bent = uv + vec2f(dx, -dy) * 0.5;
    let sharp = sample_src(bent).rgb;
    let clear = clamp(here.x + here.y, 0.0, 1.0);
    let fog = fogged(uv, params.fog * 0.004) * (1.0 - params.fog * 0.15);
    var color = mix(fog, sharp, clear);
    color = to_display(to_linear(color) * params.tint.rgb);
    return vec4f(color, 1.0);
}
