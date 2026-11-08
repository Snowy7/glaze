//! `stack` wallpapers (docs/wallpaper-builder.md): a base plus effect, image and model layers.
//!
//! Every part draws into offscreen textures in the frame's format; the last layer draws into
//! the frame itself. Cost rules:
//! - a **still** stack (no moving base, no animated layer) draws once per change at full size
//!   and keeps nothing but its inputs: the engine's kept frame is the result, like an `image`;
//! - a **moving** stack bakes its base and static prefix (the layers before the first animated
//!   one) once per render size, then runs the rest at `fps` and `scale`.
//!
//! An effect that fails to load or compile is skipped; its error is logged and written, with
//! the base's, to `%LOCALAPPDATA%\mypaper\stack-errors.json` for Studio.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use mypaper_theme::stack::{self, Base, Blend, Diagnostic, Layer, StackWallpaper};
use mypaper_theme::{LoadedTheme, ShaderWallpaper, VideoWallpaper};

use super::scene::{Environment as SceneEnvironment, SceneOptions, SceneRenderer};
use super::wic::{Budget, Decoder, Picture};
use super::{Frame, Redraw, Source, begin};
use crate::gpu::Gpu;

/// Immediate data of an effect pass: `struct Paper` in effects/prelude.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
struct Paper {
    resolution: [f32; 2],
    mouse: [f32; 2],
    time: f32,
    frame: u32,
    clock_hours: f32,
    _pad: f32,
}

/// Immediate data of the compose pass (`COMPOSE`): 32 bytes, the device's immediate limit.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
struct Compose {
    rect: [f32; 4],
    resolution: [f32; 2],
    mix: f32,
    flags: u32,
}

const PREMULTIPLIED: u32 = 1;
const OPAQUE: u32 = 2;
const CONTAIN: u32 = 16;
/// [`Targets::groups`] key of a layer's compose pass.
const COMPOSE_GROUP: usize = usize::MAX;

fn blend_bits(blend: Blend) -> u32 {
    (match blend {
        Blend::Normal => 0,
        Blend::Add => 1,
        Blend::Screen => 2,
        Blend::Multiply => 3,
    }) << 2
}

/// Draws layer `b` over input `a` inside `rect`, with a blend mode and `mix`.
const COMPOSE: &str = r#"
struct Compose { rect: vec4f, resolution: vec2f, mix: f32, flags: u32 }
var<immediate> c: Compose;
@group(0) @binding(0) var a: texture_2d<f32>;
@group(0) @binding(1) var b: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let uv = pos.xy / c.resolution;
    let under = textureSampleLevel(a, s, uv, 0.0);
    var r = c.rect;
    if (c.flags & 16u) != 0u {
        // Fit the layer inside its rect, keeping its aspect.
        let px = r.zw * c.resolution;
        let img = vec2f(textureDimensions(b));
        let fitted = img * min(px.x / img.x, px.y / img.y) / c.resolution;
        r = vec4f(r.xy + (r.zw - fitted) * 0.5, fitted);
    }
    let local = (uv - r.xy) / r.zw;
    if any(local < vec2f(0.0)) || any(local > vec2f(1.0)) {
        return vec4f(under.rgb, 1.0);
    }
    var over = textureSampleLevel(b, s, local, 0.0);
    if (c.flags & 2u) != 0u {
        over.a = 1.0;
    } else if (c.flags & 1u) != 0u && over.a > 0.0 {
        over = vec4f(over.rgb / over.a, over.a);
    }
    let mode = (c.flags >> 2u) & 3u;
    var blended = over.rgb;
    if mode == 1u {
        blended = under.rgb + over.rgb;
    } else if mode == 2u {
        blended = 1.0 - (1.0 - under.rgb) * (1.0 - over.rgb);
    } else if mode == 3u {
        blended = under.rgb * over.rgb;
    }
    let out = mix(under.rgb, clamp(blended, vec3f(0.0), vec3f(1.0)), over.a * c.mix);
    return vec4f(out, 1.0);
}
"#;

/// Linear or radial gradient from up to eight stops (display-encoded colors).
const GRADIENT: &str = r#"
struct Gradient { count: u32, radial: u32, angle: f32, _pad: f32, stops: array<vec4f, 8> }
struct GradientPaper { resolution: vec2f }
var<immediate> paper: GradientPaper;
@group(0) @binding(0) var<uniform> g: Gradient;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let aspect = paper.resolution.x / paper.resolution.y;
    let p = (pos.xy / paper.resolution - 0.5) * vec2f(aspect, 1.0);
    var t: f32;
    if g.radial != 0u {
        t = length(p) / length(vec2f(aspect, 1.0) * 0.5);
    } else {
        let d = vec2f(cos(g.angle), sin(g.angle));
        t = dot(p, d) / (abs(d.x) * aspect + abs(d.y)) + 0.5;
    }
    t = clamp(t, 0.0, 1.0);
    var color = g.stops[0].rgb;
    for (var i = 1u; i < g.count; i++) {
        let a = g.stops[i - 1u];
        let b = g.stops[i];
        if t > a.w {
            color = mix(a.rgb, b.rgb, clamp((t - a.w) / max(b.w - a.w, 1e-5), 0.0, 1.0));
        }
    }
    return vec4f(color, 1.0);
}
"#;

/// The vertex half of a Shadertoy base (its fragment comes from naga's GLSL front end).
const FULLSCREEN_VS: &str = r#"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

/// One error for `stack-errors.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct StackError {
    pub layer: String,
    pub effect: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

pub struct StackSource {
    base: BaseDraw,
    layers: Vec<LayerDraw>,
    /// Layers before this index never move; with a still base they bake once per size.
    split: usize,
    moving: bool,
    interval: Duration,
    scale: f32,
    shared: bool,
    releasable: bool,
    kit: Kit,
    /// Kept per render size while the stack moves; a still stack keeps none.
    targets: Vec<Targets>,
    /// This load's errors (also in stack-errors.json); read by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub errors: Vec<StackError>,
    _watches: Vec<mypaper_fswatch::Watch>,
}

/// Pipelines and bindings every stack shares.
struct Kit {
    sampler: wgpu::Sampler,
    effect_layout: wgpu::BindGroupLayout,
    effect_pipeline_layout: wgpu::PipelineLayout,
    compose_layout: wgpu::BindGroupLayout,
    compose: wgpu::RenderPipeline,
    /// 1x1 R32Float holding 1.0: the depth of a base that has none.
    flat_depth: wgpu::TextureView,
}

enum BaseDraw {
    Picture(Picture, mypaper_theme::Fit, [f32; 2]),
    Color(wgpu::Color),
    Gradient {
        pipeline: wgpu::RenderPipeline,
        group: wgpu::BindGroup,
    },
    /// An existing source (shader, video) drawn into the base texture.
    Source(Box<dyn Source>),
    Glsl {
        pipeline: wgpu::RenderPipeline,
        uniforms: wgpu::Buffer,
        group: wgpu::BindGroup,
        last: Duration,
    },
    Scene(SceneRenderer),
    /// Another theme's own source (`Base::Theme`), rendered offscreen as it is. `every` is its
    /// redraw interval when it moves; `None` keeps it baked like a picture.
    Theme {
        source: Box<dyn Source>,
        every: Option<Duration>,
    },
}

enum LayerDraw {
    Effect(EffectDraw),
    Image {
        view: wgpu::TextureView,
        rect: [f32; 4],
        flags: u32,
        mix: f32,
    },
    Model {
        renderer: SceneRenderer,
        rect: [f32; 4],
        flags: u32,
        mix: f32,
        moves: bool,
    },
    /// Disabled or failed: passes its input through untouched.
    Skip,
}

struct EffectDraw {
    passes: Vec<(wgpu::RenderPipeline, f32)>,
    params: wgpu::Buffer,
    mix: f32,
    flags: u32,
    animated: bool,
}

impl LayerDraw {
    fn moves(&self) -> bool {
        match self {
            LayerDraw::Effect(e) => e.animated,
            LayerDraw::Model { moves, .. } => *moves,
            LayerDraw::Image { .. } | LayerDraw::Skip => false,
        }
    }
}

/// Offscreen textures for one render size.
struct Targets {
    size: (u32, u32),
    base: Tex,
    depth: Option<Tex>,
    /// The static prefix's result (moving stacks with `split > 0` and a still base).
    prefix: Option<Tex>,
    chain: [Tex; 2],
    /// Scaled pass outputs and model renders, by (layer, pass).
    temps: HashMap<(usize, usize), Tex>,
    /// Bind groups by (layer, pass or [`COMPOSE_GROUP`]): every view they bind is fixed per size.
    groups: HashMap<(usize, usize), wgpu::BindGroup>,
    base_ready: bool,
    prefix_ready: bool,
}

#[derive(Clone)]
struct Tex {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Tex {
    fn new(gpu: &Gpu, label: &str, size: (u32, u32), format: wgpu::TextureFormat) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            _texture: texture,
            view,
        }
    }

    fn size(&self) -> (u32, u32) {
        let t = &self._texture;
        (t.width(), t.height())
    }
}

fn scaled(size: (u32, u32), scale: f32) -> (u32, u32) {
    (
        ((size.0 as f32 * scale).ceil() as u32).max(1),
        ((size.1 as f32 * scale).ceil() as u32).max(1),
    )
}

impl StackSource {
    pub fn new(gpu: &Gpu, theme: &LoadedTheme, config: &StackWallpaper) -> Result<Self> {
        Self::new_in(gpu, theme, config, &stack::effect_search(&theme.dir))
    }

    /// [`Self::new`] resolving effects in `roots` (in order).
    fn new_in(
        gpu: &Gpu,
        theme: &LoadedTheme,
        config: &StackWallpaper,
        roots: &[PathBuf],
    ) -> Result<Self> {
        let kit = Kit::new(gpu)?;
        let mut errors = Vec::new();
        let base = match BaseDraw::new(gpu, theme, config) {
            Ok(base) => base,
            Err(error) => {
                let (line, message) = describe(&error);
                log::error!("stack base: {message} (line {line:?})");
                errors.push(StackError {
                    layer: "base".into(),
                    effect: None,
                    line,
                    message,
                });
                BaseDraw::Color(wgpu::Color::BLACK)
            }
        };
        let mut effect_dirs = Vec::new();
        let mut layers = Vec::new();
        for layer in &config.layers {
            if !layer.active() {
                layers.push(LayerDraw::Skip);
                continue;
            }
            match LayerDraw::new(gpu, &kit, theme, roots, layer, &mut effect_dirs) {
                Ok(draw) => layers.push(draw),
                Err(error) => {
                    let (line, message) = describe(&error);
                    log::error!("stack layer {:?}: {message} (line {line:?})", layer.id());
                    errors.push(StackError {
                        layer: layer.id().to_owned(),
                        effect: match layer {
                            Layer::Effect(e) => Some(e.effect.clone()),
                            _ => None,
                        },
                        line,
                        message,
                    });
                    layers.push(LayerDraw::Skip);
                }
            }
        }
        let per_frame = stack::per_frame_from(base.moves(), layers.iter().map(LayerDraw::moves));
        let moving = per_frame.is_some();
        let reads_mouse = match &config.base {
            Base::Shader(_) | Base::Glsl(_) => {
                matches!(&base, BaseDraw::Source(s) if !s.shared())
                    || matches!(&base, BaseDraw::Glsl { .. })
            }
            Base::Scene(s) => s.parallax != 0.0 || s.interactive,
            Base::Theme(_) => matches!(&base, BaseDraw::Theme { source, .. } if !source.shared()),
            _ => false,
        } || config
            .layers
            .iter()
            .any(|l| matches!(l, Layer::Model(m) if m.enabled && m.parallax != 0.0));
        let releasable = match &base {
            BaseDraw::Theme { source, .. } => source.releasable(),
            _ => !matches!(config.base, Base::Video(_) | Base::Scene(_)),
        };
        // A moving theme base keeps its own frame rate (a 30 fps shader stays 30 fps).
        let interval = match &base {
            BaseDraw::Theme { every: Some(every), .. } => *every,
            _ => Duration::from_secs(1) / config.fps,
        };
        write_errors(theme, &errors);
        let watches = watch(&effect_dirs, &theme.dir);
        Ok(Self {
            base,
            layers,
            split: per_frame.unwrap_or(config.layers.len()),
            moving,
            interval,
            scale: config.scale,
            shared: !reads_mouse,
            releasable,
            kit,
            targets: Vec::new(),
            errors,
            _watches: watches,
        })
    }

    /// True when nothing in the stack changes over time.
    #[cfg(test)]
    pub fn still(&self) -> bool {
        !self.moving
    }

    fn targets(&mut self, gpu: &Gpu, size: (u32, u32)) -> Targets {
        if let Some(at) = self.targets.iter().position(|t| t.size == size) {
            return self.targets.swap_remove(at);
        }
        let format = gpu.format;
        let depth = matches!(self.base, BaseDraw::Scene(_))
            .then(|| Tex::new(gpu, "stack depth", size, wgpu::TextureFormat::R32Float));
        let prefix =
            (self.moving && self.split > 0).then(|| Tex::new(gpu, "stack prefix", size, format));
        Targets {
            size,
            base: Tex::new(gpu, "stack base", size, format),
            depth,
            prefix,
            chain: [
                Tex::new(gpu, "stack chain", size, format),
                Tex::new(gpu, "stack chain", size, format),
            ],
            temps: HashMap::new(),
            groups: HashMap::new(),
            base_ready: false,
            prefix_ready: false,
        }
    }

    fn draw_base(&mut self, gpu: &Gpu, frame: &mut Frame<'_>, t: &Targets) {
        let size = t.size;
        let view = &t.base.view;
        match &mut self.base {
            BaseDraw::Picture(picture, fit, focus) => {
                let mut pass = begin(frame.encoder, view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                picture.draw_focused(gpu, &mut pass, size, *fit, *focus, 1.0);
            }
            BaseDraw::Color(color) => super::clear(frame.encoder, view, *color),
            BaseDraw::Gradient { pipeline, group } => {
                let mut pass = begin(frame.encoder, view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_pipeline(pipeline);
                let resolution = [size.0 as f32, size.1 as f32];
                pass.set_immediates(0, bytemuck::cast_slice(&resolution));
                pass.set_bind_group(0, &*group, &[]);
                pass.draw(0..3, 0..1);
            }
            BaseDraw::Source(source) | BaseDraw::Theme { source, .. } => source.render(
                gpu,
                Frame {
                    screen: frame.screen,
                    size,
                    mouse: frame.mouse,
                    clock: frame.clock,
                    frame: frame.frame,
                    view,
                    encoder: frame.encoder,
                },
            ),
            BaseDraw::Glsl {
                pipeline,
                uniforms,
                group,
                last,
            } => {
                let time = frame.clock.as_secs_f32();
                let delta = frame.clock.saturating_sub(*last).as_secs_f32();
                *last = frame.clock;
                let mut words = [0u32; 28];
                let f = |v: f32| v.to_bits();
                words[..4].copy_from_slice(&[f(size.0 as f32), f(size.1 as f32), f(1.0), f(time)]);
                // iMouse: Shadertoy's bottom-left origin; zw (click) stays 0.
                words[4] = f(frame.mouse[0]);
                words[5] = f(size.1 as f32 - frame.mouse[1]);
                words[8] = f(delta);
                words[9] = frame.frame;
                words[10] = f(1.0 / self.interval.as_secs_f32());
                // iChannelResolution[0..4]: std140 vec3 array elements, 16 bytes apart.
                let noise = stack::GLSL_NOISE_SIZE as f32;
                for c in 0..4 {
                    words[12 + c * 4..15 + c * 4].copy_from_slice(&[f(noise), f(noise), f(1.0)]);
                }
                gpu.queue
                    .write_buffer(uniforms, 0, bytemuck::cast_slice(&words));
                let mut pass = begin(frame.encoder, view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &*group, &[]);
                pass.draw(0..3, 0..1);
            }
            BaseDraw::Scene(scene) => scene.render_into_for_screen(
                gpu,
                frame.encoder,
                view,
                t.depth.as_ref().map(|d| &d.view),
                size,
                frame.clock,
                frame.mouse,
                false,
                frame.screen,
            ),
        }
    }

    /// Runs `layers[range]` from `input` into `output`, ping-ponging through the chain.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        gpu: &Gpu,
        frame: &mut Frame<'_>,
        t: &mut Targets,
        range: std::ops::Range<usize>,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) {
        let range_start = range.start;
        let active: Vec<usize> = range
            .filter(|&i| !matches!(self.layers[i], LayerDraw::Skip))
            .collect();
        if active.is_empty() {
            let group = t
                .groups
                .entry((usize::MAX, range_start))
                .or_insert_with(|| self.compose_group(gpu, input, input))
                .clone();
            self.compose(
                frame.encoder,
                &group,
                output,
                t.size,
                [0.0, 0.0, 1.0, 1.0],
                0.0,
                OPAQUE,
            );
            return;
        }
        let original = t.base.view.clone();
        let depth = t
            .depth
            .as_ref()
            .map_or(self.kit.flat_depth.clone(), |d| d.view.clone());
        let mut current = input.clone();
        for (n, &i) in active.iter().enumerate() {
            let target = if n + 1 == active.len() {
                output.clone()
            } else {
                t.chain[n % 2].view.clone()
            };
            self.draw_layer(gpu, frame, t, i, &current, &target, &original, &depth);
            current = target;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_layer(
        &mut self,
        gpu: &Gpu,
        frame: &mut Frame<'_>,
        t: &mut Targets,
        index: usize,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        original: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        let size = t.size;
        let full = [0.0, 0.0, 1.0, 1.0];
        match &mut self.layers[index] {
            LayerDraw::Effect(effect) => {
                let hours = clock_hours();
                let mut src = input.clone();
                let last = effect.passes.len() - 1;
                let direct =
                    effect.mix >= 1.0 && effect.flags == OPAQUE && effect.passes[last].1 >= 1.0;
                for (p, (pipeline, scale)) in effect.passes.iter().enumerate() {
                    let pass_size = scaled(size, *scale);
                    let target = if p == last && direct {
                        output.clone()
                    } else {
                        let tex = t
                            .temps
                            .entry((index, p))
                            .or_insert_with(|| Tex::new(gpu, "stack pass", pass_size, gpu.format));
                        if tex.size() != pass_size {
                            *tex = Tex::new(gpu, "stack pass", pass_size, gpu.format);
                        }
                        tex.view.clone()
                    };
                    let kit = &self.kit;
                    let group = t.groups.entry((index, p)).or_insert_with(|| {
                        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("effect"),
                            layout: &kit.effect_layout,
                            entries: &[
                                entry(0, wgpu::BindingResource::TextureView(&src)),
                                entry(1, wgpu::BindingResource::Sampler(&kit.sampler)),
                                entry(2, wgpu::BindingResource::TextureView(original)),
                                entry(3, wgpu::BindingResource::TextureView(depth)),
                                entry(4, effect.params.as_entire_binding()),
                                entry(5, wgpu::BindingResource::TextureView(input)),
                            ],
                        })
                    });
                    let k = pass_size.0 as f32 / size.0.max(1) as f32;
                    let paper = Paper {
                        resolution: [pass_size.0 as f32, pass_size.1 as f32],
                        mouse: [frame.mouse[0] * k, frame.mouse[1] * k],
                        time: frame.clock.as_secs_f32(),
                        frame: frame.frame,
                        clock_hours: hours,
                        _pad: 0.0,
                    };
                    let mut pass = begin(
                        frame.encoder,
                        &target,
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    );
                    pass.set_pipeline(pipeline);
                    pass.set_immediates(0, bytemuck::bytes_of(&paper));
                    pass.set_bind_group(0, &*group, &[]);
                    pass.draw(0..3, 0..1);
                    drop(pass);
                    src = target;
                }
                if !direct {
                    let (mix, flags) = (effect.mix, effect.flags);
                    let group = t
                        .groups
                        .entry((index, COMPOSE_GROUP))
                        .or_insert_with(|| self.compose_group(gpu, input, &src))
                        .clone();
                    self.compose(
                        frame.encoder,
                        &group,
                        output,
                        size,
                        full,
                        mix,
                        flags,
                    );
                }
            }
            LayerDraw::Image {
                view,
                rect,
                flags,
                mix,
            } => {
                let (view, rect, flags, mix) = (view.clone(), *rect, *flags, *mix);
                let group = t
                    .groups
                    .entry((index, COMPOSE_GROUP))
                    .or_insert_with(|| self.compose_group(gpu, input, &view))
                    .clone();
                self.compose(
                    frame.encoder,
                    &group,
                    output,
                    size,
                    rect,
                    mix,
                    flags,
                );
            }
            LayerDraw::Model {
                renderer,
                rect,
                flags,
                mix,
                ..
            } => {
                let px = (
                    ((rect[2] * size.0 as f32).round() as u32).max(1),
                    ((rect[3] * size.1 as f32).round() as u32).max(1),
                );
                let tex = t
                    .temps
                    .entry((index, 0))
                    .or_insert_with(|| Tex::new(gpu, "stack model", px, gpu.format));
                if tex.size() != px {
                    *tex = Tex::new(gpu, "stack model", px, gpu.format);
                }
                let view = tex.view.clone();
                let mouse = [
                    frame.mouse[0] - rect[0] * size.0 as f32,
                    frame.mouse[1] - rect[1] * size.1 as f32,
                ];
                renderer.render_into_for_screen(
                    gpu,
                    frame.encoder,
                    &view,
                    None,
                    px,
                    frame.clock,
                    mouse,
                    true,
                    frame.screen,
                );
                let (rect, flags, mix) = (*rect, *flags, *mix);
                let group = t
                    .groups
                    .entry((index, COMPOSE_GROUP))
                    .or_insert_with(|| self.compose_group(gpu, input, &view))
                    .clone();
                self.compose(
                    frame.encoder,
                    &group,
                    output,
                    size,
                    rect,
                    mix,
                    flags,
                );
            }
            LayerDraw::Skip => {}
        }
    }

    /// The compose pass's bindings: layer `over` drawn on `under`.
    fn compose_group(
        &self,
        gpu: &Gpu,
        under: &wgpu::TextureView,
        over: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose"),
            layout: &self.kit.compose_layout,
            entries: &[
                entry(0, wgpu::BindingResource::TextureView(under)),
                entry(1, wgpu::BindingResource::TextureView(over)),
                entry(2, wgpu::BindingResource::Sampler(&self.kit.sampler)),
            ],
        })
    }

    /// Draws `group` (from [`Self::compose_group`]) into `output`.
    #[allow(clippy::too_many_arguments)]
    fn compose(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        group: &wgpu::BindGroup,
        output: &wgpu::TextureView,
        size: (u32, u32),
        rect: [f32; 4],
        mix: f32,
        flags: u32,
    ) {
        let immediates = Compose {
            rect,
            resolution: [size.0 as f32, size.1 as f32],
            mix,
            flags,
        };
        let mut pass = begin(encoder, output, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        pass.set_pipeline(&self.kit.compose);
        pass.set_immediates(0, bytemuck::bytes_of(&immediates));
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Tests pin `paper.clock_hours` here (f32 bits; `u32::MAX` reads the real clock).
#[cfg(test)]
static TEST_CLOCK_HOURS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// The local wall-clock hour, 0..24, for `paper.clock_hours`.
fn clock_hours() -> f32 {
    #[cfg(test)]
    match TEST_CLOCK_HOURS.load(std::sync::atomic::Ordering::Relaxed) {
        u32::MAX => {}
        bits => return f32::from_bits(bits),
    }
    // SAFETY: no preconditions.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    f32::from(t.wHour) + f32::from(t.wMinute) / 60.0 + f32::from(t.wSecond) / 3600.0
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

impl Source for StackSource {
    fn render_size(&self, size: (u32, u32)) -> (u32, u32) {
        if self.moving {
            scaled(size, self.scale)
        } else {
            size
        }
    }

    fn shared(&self) -> bool {
        self.shared
    }

    fn releasable(&self) -> bool {
        self.releasable
    }

    fn update(&mut self, gpu: &Gpu, clock: Duration) {
        if let BaseDraw::Source(source) | BaseDraw::Theme { source, .. } = &mut self.base {
            source.update(gpu, clock);
        }
    }

    fn render(&mut self, gpu: &Gpu, mut frame: Frame<'_>) {
        let size = frame.size;
        let mut t = self.targets(gpu, size);
        let output = frame.view.clone();
        if !t.base_ready || self.base.moves() {
            self.draw_base(gpu, &mut frame, &t);
            t.base_ready = true;
        }
        let base = t.base.view.clone();
        match t.prefix.as_ref().map(|p| p.view.clone()) {
            Some(prefix) => {
                if !t.prefix_ready {
                    self.run(gpu, &mut frame, &mut t, 0..self.split, &base, &prefix);
                    t.prefix_ready = true;
                }
                let end = self.layers.len();
                self.run(gpu, &mut frame, &mut t, self.split..end, &prefix, &output);
            }
            None => {
                let end = self.layers.len();
                self.run(gpu, &mut frame, &mut t, 0..end, &base, &output);
            }
        }
        // A still stack is drawn once per change: the engine keeps the frame, we keep nothing.
        if self.moving {
            self.targets.push(t);
        }
    }

    fn redraw(&self, clock: Duration) -> Redraw {
        // Over still layers, a theme base keeps its own schedule (a slideshow's next slide).
        if let BaseDraw::Theme { source, .. } = &self.base
            && !self.layers.iter().any(LayerDraw::moves)
        {
            return source.redraw(clock);
        }
        if self.moving {
            Redraw::Every(self.interval)
        } else {
            Redraw::OnChange
        }
    }
}

impl Kit {
    fn new(gpu: &Gpu) -> Result<Self> {
        let device = &gpu.device;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("stack"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let texture = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let effect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect"),
            entries: &[
                texture(0, true),
                sampler_entry(1),
                texture(2, true),
                texture(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(5, true),
            ],
        });
        let effect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("effect"),
                bind_group_layouts: &[Some(&effect_layout)],
                immediate_size: super::shader::PAPER_SIZE,
            });
        let compose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose"),
            entries: &[texture(0, true), texture(1, true), sampler_entry(2)],
        });
        let compose = gpu.checked(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("stack compose"),
                source: wgpu::ShaderSource::Wgsl(COMPOSE.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compose"),
                bind_group_layouts: &[Some(&compose_layout)],
                immediate_size: size_of::<Compose>() as u32,
            });
            pipeline(gpu, &layout, &module, "vs", &module, "fs")
        })?;
        let depth = gpu.texture(
            "flat depth",
            (1, 1),
            wgpu::TextureFormat::R32Float,
            wgpu::TextureUsages::empty(),
            Some(&1.0f32.to_le_bytes()),
        );
        Ok(Self {
            sampler,
            effect_layout,
            effect_pipeline_layout,
            compose_layout,
            compose,
            flat_depth: depth.create_view(&Default::default()),
        })
    }
}

fn pipeline(
    gpu: &Gpu,
    layout: &wgpu::PipelineLayout,
    vs: &wgpu::ShaderModule,
    vs_entry: &str,
    fs: &wgpu::ShaderModule,
    fs_entry: &str,
) -> wgpu::RenderPipeline {
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stack"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: vs,
                entry_point: Some(vs_entry),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: fs,
                entry_point: Some(fs_entry),
                compilation_options: Default::default(),
                targets: &[Some(gpu.format.into())],
            }),
            multiview_mask: None,
            cache: None,
        })
}

/// A Diagnostic as an error the stack can downcast back.
impl std::fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.line {
            Some(line) => write!(f, "line {line}: {}", self.0.message),
            None => f.write_str(&self.0.message),
        }
    }
}

#[derive(Debug)]
struct DiagnosticError(Diagnostic);
impl std::error::Error for DiagnosticError {}

fn diagnostic(d: Diagnostic) -> anyhow::Error {
    anyhow::Error::new(DiagnosticError(d))
}

/// `(line, message)` of an error, with the line when it came from naga.
fn describe(error: &anyhow::Error) -> (Option<u32>, String) {
    match error.downcast_ref::<DiagnosticError>() {
        Some(DiagnosticError(d)) => (d.line, d.message.clone()),
        None => (None, format!("{error:#}")),
    }
}

impl BaseDraw {
    fn new(gpu: &Gpu, theme: &LoadedTheme, config: &StackWallpaper) -> Result<Self> {
        Ok(match &config.base {
            Base::Image(b) => {
                let path = theme.asset(&b.source);
                let decoded = Decoder::new()?
                    .decode(&path, Budget::new(gpu, b.fit))
                    .with_context(|| format!("decoding {}", path.display()))?;
                BaseDraw::Picture(Picture::upload(gpu, "stack base", &decoded)?, b.fit, b.focus)
            }
            Base::Color(b) => BaseDraw::Color(display(b.color)),
            Base::Gradient(b) => gradient(gpu, b)?,
            Base::Video(b) => BaseDraw::Source(Box::new(super::video::VideoSource::new(
                gpu,
                theme,
                &VideoWallpaper {
                    source: b.source.clone(),
                    fit: b.fit,
                    fps: config.fps,
                    speed: 1.0,
                },
            )?)),
            Base::Shader(b) => {
                let path = theme.asset(&b.source);
                let source = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                stack::check::shader_base(&source, &b.params).map_err(diagnostic)?;
                let code = format!(
                    "{source}\n{}",
                    stack::shader_params_wgsl(&stack::shader_params(&source), &b.params)
                );
                let shader = ShaderWallpaper {
                    source: b.source.clone(),
                    images: Vec::new(),
                    fps: config.fps,
                    scale: 1.0,
                };
                BaseDraw::Source(Box::new(super::shader::ShaderSource::with_code(
                    gpu, theme, &shader, &code,
                )?))
            }
            Base::Glsl(b) => glsl(gpu, &theme.asset(&b.source))?,
            Base::Theme(b) => {
                let other = b.load()?;
                let source = super::load(gpu, &other, &[])?;
                let every = match source.redraw(Duration::ZERO) {
                    super::Redraw::Every(every) => Some(every),
                    // A slideshow schedules its own frames; see `redraw`.
                    super::Redraw::At(_) => Some(Duration::from_secs(1) / config.fps),
                    super::Redraw::OnChange => None,
                };
                BaseDraw::Theme { source, every }
            }
            Base::Scene(b) => BaseDraw::Scene(SceneRenderer::load(
                gpu,
                &theme.asset(&b.source),
                &SceneOptions {
                    orbit: b.orbit,
                    parallax: b.parallax,
                    scale: 1.0,
                    background: b.background,
                    environment: environment(b.environment),
                    exposure: b.exposure,
                    ..Default::default()
                },
            )?),
        })
    }

    fn moves(&self) -> bool {
        match self {
            BaseDraw::Picture(..) | BaseDraw::Color(_) | BaseDraw::Gradient { .. } => false,
            BaseDraw::Source(_) | BaseDraw::Glsl { .. } => true,
            BaseDraw::Scene(scene) => scene.animated(),
            BaseDraw::Theme { every, .. } => every.is_some(),
        }
    }
}

fn environment(e: mypaper_theme::Environment) -> SceneEnvironment {
    match e {
        mypaper_theme::Environment::Sky => SceneEnvironment::Sky,
        mypaper_theme::Environment::Color => SceneEnvironment::Color,
        mypaper_theme::Environment::Transparent => SceneEnvironment::Transparent,
    }
}

/// A theme color as the frame's (display-encoded, unorm) value.
fn display(color: mypaper_theme::Color) -> wgpu::Color {
    let [r, g, b] = color.0.map(|c| f64::from(c) / 255.0);
    wgpu::Color { r, g, b, a: 1.0 }
}

fn gradient(gpu: &Gpu, b: &stack::GradientBase) -> Result<BaseDraw> {
    if b.stops.len() > 8 {
        bail!("a gradient takes at most 8 stops");
    }
    let mut stops = b.stops.clone();
    stops.sort_by(|x, y| x.at.total_cmp(&y.at));
    let mut words = vec![
        stops.len() as u32,
        u32::from(b.kind == stack::GradientKind::Radial),
        b.angle.to_radians().to_bits(),
        0,
    ];
    for i in 0..8 {
        let stop = stops[i.min(stops.len() - 1)];
        let [r, g, bl] = stop.color.0.map(|c| f32::from(c) / 255.0);
        words.extend([r, g, bl, stop.at].map(f32::to_bits));
    }
    let device = &gpu.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gradient"),
        size: (words.len() * 4) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue
        .write_buffer(&buffer, 0, bytemuck::cast_slice(&words));
    let layout = uniform_layout(gpu);
    let pipeline = gpu.checked(|| {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gradient"),
            source: wgpu::ShaderSource::Wgsl(GRADIENT.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gradient"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 8,
        });
        pipeline(gpu, &pl, &module, "vs", &module, "fs")
    })?;
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gradient"),
        layout: &layout,
        entries: &[entry(0, buffer.as_entire_binding())],
    });
    Ok(BaseDraw::Gradient { pipeline, group })
}

fn uniform_layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
}

/// A Shadertoy base: naga translates the wrapped GLSL; its errors carry the file's lines.
/// `iChannel0..3` sample one tiling RGBA noise texture, as most Shadertoy imports expect.
fn glsl(gpu: &Gpu, path: &Path) -> Result<BaseDraw> {
    let code =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let module = stack::check::glsl(&code).map_err(diagnostic)?;
    let device = &gpu.device;
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shadertoy"),
        size: stack::GLSL_UNIFORM_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadertoy"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let side = stack::GLSL_NOISE_SIZE;
    let noise = gpu
        .texture(
            "shadertoy noise",
            (side, side),
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::empty(),
            Some(&noise_rgba(side)),
        )
        .create_view(&Default::default());
    let repeat = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("shadertoy repeat"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let pipeline = gpu.checked(|| {
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadertoy"),
            source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
        });
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadertoy vs"),
            source: wgpu::ShaderSource::Wgsl(FULLSCREEN_VS.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadertoy"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        pipeline(gpu, &pl, &vs, "vs", &fs, "main")
    })?;
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadertoy"),
        layout: &layout,
        entries: &[
            entry(0, uniforms.as_entire_binding()),
            entry(1, wgpu::BindingResource::TextureView(&noise)),
            entry(2, wgpu::BindingResource::Sampler(&repeat)),
        ],
    });
    Ok(BaseDraw::Glsl {
        pipeline,
        uniforms,
        group,
        last: Duration::ZERO,
    })
}

/// `side`² RGBA texels of uniform white noise, the same every run (a PCG hash per byte).
fn noise_rgba(side: u32) -> Vec<u8> {
    (0..side * side * 4)
        .map(|i| {
            let state = i.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
            let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
            ((word >> 22) ^ word) as u8
        })
        .collect()
}

impl LayerDraw {
    fn new(
        gpu: &Gpu,
        kit: &Kit,
        theme: &LoadedTheme,
        roots: &[PathBuf],
        layer: &Layer,
        effect_dirs: &mut Vec<PathBuf>,
    ) -> Result<Self> {
        Ok(match layer {
            Layer::Effect(l) => {
                let effect = stack::locate_effect_in(&l.effect, roots)?;
                if !effect.dir.starts_with(&theme.dir) && !effect_dirs.contains(&effect.dir) {
                    effect_dirs.push(effect.dir.clone());
                }
                let manifest = &effect.manifest;
                let mut passes = Vec::new();
                for (i, pass) in manifest.passes.iter().enumerate() {
                    let built = effect.pass(i)?;
                    let module = stack::check::wgsl(&built.code, |line| built.source_line(line))
                        .map_err(|mut d| {
                            if manifest.passes.len() > 1 {
                                d.message = format!("{}: {}", pass.source.display(), d.message);
                            }
                            diagnostic(d)
                        })?;
                    let pipeline = gpu.checked(|| {
                        let module =
                            gpu.device
                                .create_shader_module(wgpu::ShaderModuleDescriptor {
                                    label: Some(&manifest.id),
                                    source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
                                });
                        pipeline(
                            gpu,
                            &kit.effect_pipeline_layout,
                            &module,
                            "mypaper_vs",
                            &module,
                            "mypaper_fx",
                        )
                    })?;
                    passes.push((pipeline, pass.scale));
                }
                let bytes = stack::params_bytes(&manifest.params, &l.params);
                let params = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effect params"),
                    size: bytes.len() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                gpu.queue.write_buffer(&params, 0, &bytes);
                LayerDraw::Effect(EffectDraw {
                    passes,
                    params,
                    mix: l.mix,
                    flags: OPAQUE | blend_bits(l.blend),
                    animated: manifest.animated,
                })
            }
            Layer::Image(l) => {
                let path = theme.asset(&l.source);
                let decoded = Decoder::new()?
                    .decode(&path, Budget::new(gpu, mypaper_theme::Fit::Contain))
                    .with_context(|| format!("decoding {}", path.display()))?;
                let texture = gpu.texture(
                    "stack image",
                    decoded.size,
                    wgpu::TextureFormat::Bgra8Unorm,
                    wgpu::TextureUsages::empty(),
                    Some(&decoded.bgra),
                );
                LayerDraw::Image {
                    view: texture.create_view(&Default::default()),
                    rect: l.rect,
                    flags: CONTAIN | blend_bits(l.blend),
                    mix: l.mix,
                }
            }
            Layer::Model(l) => {
                let renderer = SceneRenderer::load(
                    gpu,
                    &theme.asset(&l.source),
                    &SceneOptions {
                        orbit: l.orbit,
                        parallax: l.parallax,
                        scale: l.scale.clamp(0.25, 1.0),
                        rotation: l.rotation,
                        light: l.light,
                        environment: SceneEnvironment::Transparent,
                        animation: l.animation.clone(),
                        ..Default::default()
                    },
                )?;
                LayerDraw::Model {
                    moves: renderer.animated(),
                    renderer,
                    rect: l.rect,
                    flags: PREMULTIPLIED | blend_bits(l.blend),
                    mix: l.mix,
                }
            }
        })
    }
}

/// `%LOCALAPPDATA%\mypaper\stack-errors.json`: this load's errors (empty when it is clean).
fn write_errors(theme: &LoadedTheme, errors: &[StackError]) {
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("mypaper"))
    else {
        return;
    };
    let id = theme
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let errors: Vec<_> = errors
        .iter()
        .map(|e| {
            serde_json::json!({
                "layer": e.layer, "effect": e.effect, "line": e.line, "message": e.message,
            })
        })
        .collect();
    let json = serde_json::json!({ "theme": id, "dir": theme.dir, "errors": errors });
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::fs::write(dir.join("stack-errors.json"), json.to_string()) {
        log::warn!("writing stack-errors.json: {e}");
    }
}

/// Reloads the theme when a used effect outside its folder changes (the theme folder itself is
/// watched by the control loop). Posts `WM_RELOAD` to the thread that loaded the stack.
fn watch(dirs: &[PathBuf], _theme: &Path) -> Vec<mypaper_fswatch::Watch> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
    // SAFETY: no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    dirs.iter()
        .filter_map(|dir| {
            mypaper_fswatch::Watch::directory(dir, true, move |_| {
                // SAFETY: posting to a thread id is always safe; failure means it is gone.
                let _ = unsafe {
                    PostThreadMessageW(thread, crate::window::WM_RELOAD, WPARAM(0), LPARAM(0))
                };
            })
            .inspect_err(|e| log::warn!("not watching {}: {e:#}", dir.display()))
            .ok()
        })
        .collect()
}

#[cfg(test)]
mod effects_real;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The repo's effects, as the deployed exe (in the repo's `target\release`) finds them.
    fn roots() -> Vec<PathBuf> {
        vec![repo().join("effects")]
    }

    /// A stack theme in a fresh temp folder holding `files`.
    fn stack_in(name: &str, json: &str, files: &[(&str, &str)]) -> (LoadedTheme, StackWallpaper) {
        let dir = std::env::temp_dir().join(format!("mypaper-stack-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let theme =
            LoadedTheme::parse(dir, &format!(r#"{{ "name": "t", "wallpaper": {json} }}"#)).unwrap();
        let mypaper_theme::Wallpaper::Stack(config) = theme.theme.wallpaper.clone() else {
            unreachable!()
        };
        (theme, config)
    }

    const COLOR: &str = r##""base": { "kind": "color", "color": "#203040" }"##;

    #[test]
    fn a_linear_gradient_renders_both_stop_colors() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let (theme, config) = stack_in(
            "gradient",
            r##"{"kind":"stack","base":{"kind":"gradient","angle":0,"stops":[{"color":"#ff0000","at":0},{"color":"#0000ff","at":1}]}}"##,
            &[],
        );
        let mut source = StackSource::new_in(&gpu, &theme, &config, &roots()).unwrap();
        assert!(source.errors.is_empty(), "{:?}", source.errors);
        let pixels = render(&gpu, &mut source, (64, 32), 0.0);
        let left = &pixels[(16 * 64 + 2) * 4..][..4];
        let right = &pixels[(16 * 64 + 61) * 4..][..4];
        assert!(left[2] > 200 && left[0] < 40, "left BGRA: {left:?}");
        assert!(right[0] > 200 && right[2] < 40, "right BGRA: {right:?}");
    }

    /// Every bundled effect (ours and the effects library's) compiles through the real prelude.
    #[test]
    fn every_bundled_effect_compiles_through_the_prelude() {
        let gpu = Gpu::headless().expect("DX12 is required to validate effects");
        let kit = Kit::new(&gpu).unwrap();
        let (theme, _) = stack_in("all", &format!(r#"{{ "kind": "stack", {COLOR} }}"#), &[]);
        let mut compiled = 0;
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(repo().join("effects")).unwrap().flatten() {
            if !entry.path().join(stack::EFFECT_MANIFEST).is_file() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let layer: Layer =
                serde_json::from_str(&format!(r#"{{ "kind": "effect", "effect": "{id}" }}"#))
                    .unwrap();
            match LayerDraw::new(&gpu, &kit, &theme, &roots(), &layer, &mut Vec::new()) {
                Ok(_) => compiled += 1,
                Err(e) => failures.push(format!("{id}: {:?}", describe(&e))),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(compiled >= 5, "only {compiled} effects");
    }

    #[test]
    fn still_and_moving_stacks_split_where_the_first_animated_layer_is() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let make = |layers: &str| {
            let json = format!(r#"{{ "kind": "stack", {COLOR}, "layers": [{layers}] }}"#);
            let (theme, config) = stack_in("split", &json, &[]);
            StackSource::new_in(&gpu, &theme, &config, &roots()).unwrap()
        };
        let still = make(
            r#"{ "kind": "effect", "effect": "color" }, { "kind": "effect", "effect": "vignette" }"#,
        );
        assert!(still.errors.is_empty(), "{:?}", still.errors);
        assert!(still.still());
        assert_eq!(still.redraw(Duration::ZERO), Redraw::OnChange);
        assert_eq!(still.render_size((100, 100)), (100, 100));
        assert!(still.shared() && still.releasable());

        let moving = make(
            r#"{ "kind": "effect", "effect": "color" }, { "kind": "effect", "effect": "rain" },
               { "kind": "effect", "effect": "vignette" }"#,
        );
        assert!(!moving.still());
        assert_eq!(moving.split, 1, "color bakes; rain and vignette run");
        assert!(matches!(moving.redraw(Duration::ZERO), Redraw::Every(_)));
        assert_eq!(moving.render_size((100, 100)), (100, 100));

        let disabled = make(r#"{ "kind": "effect", "effect": "rain", "enabled": false }"#);
        assert!(
            disabled.still(),
            "a disabled animated layer does not move the stack"
        );
        let unmixed = make(r#"{ "kind": "effect", "effect": "rain", "mix": 0 }"#);
        assert!(unmixed.still(), "nor does one mixed out (Studio's estimate agrees)");

        let toy = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    fragColor = vec4(fragCoord / iResolution.xy, 0.5 + 0.5 * sin(iTime), 1.0);\n}\n";
        let (theme, config) = stack_in(
            "toy",
            r#"{ "kind": "stack", "scale": 0.5, "base": { "kind": "glsl", "source": "toy.glsl" },
                "layers": [{ "kind": "effect", "effect": "vignette" }] }"#,
            &[("toy.glsl", toy)],
        );
        let toy = StackSource::new_in(&gpu, &theme, &config, &roots()).unwrap();
        assert!(toy.errors.is_empty(), "{:?}", toy.errors);
        assert!(
            !toy.still() && toy.split == 0,
            "a moving base runs everything"
        );
        assert_eq!(toy.render_size((100, 100)), (50, 50));
        assert!(!toy.shared(), "Shadertoy reads iMouse");
    }

    #[test]
    fn a_broken_effect_or_base_is_skipped_and_reported_with_its_line() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let broken = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    fragColor = vec4(nope);\n}\n";
        let (theme, config) = stack_in(
            "broken",
            r#"{ "kind": "stack", "base": { "kind": "glsl", "source": "toy.glsl" },
                "layers": [{ "id": "a", "kind": "effect", "effect": "no-such-effect" },
                           { "id": "b", "kind": "effect", "effect": "vignette" }] }"#,
            &[("toy.glsl", broken)],
        );
        let source = StackSource::new_in(&gpu, &theme, &config, &roots()).unwrap();
        let errors: Vec<_> = source
            .errors
            .iter()
            .map(|e| (e.layer.as_str(), e.line))
            .collect();
        assert_eq!(errors, [("base", Some(2)), ("a", None)]);
        assert!(matches!(source.layers[0], LayerDraw::Skip));
        assert!(matches!(source.layers[1], LayerDraw::Effect(_)));
    }

    #[test]
    fn a_shadertoy_base_samples_noise_through_ichannel0() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let toy = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    vec2 uv = fragCoord / iChannelResolution[0].xy;\n    fragColor = vec4(texture(iChannel0, uv).rgb, 1.0);\n}\n";
        let (theme, config) = stack_in(
            "noise",
            r#"{ "kind": "stack", "base": { "kind": "glsl", "source": "toy.glsl" } }"#,
            &[("toy.glsl", toy)],
        );
        let mut source = StackSource::new_in(&gpu, &theme, &config, &roots()).unwrap();
        assert!(source.errors.is_empty(), "{:?}", source.errors);
        let pixels = render(&gpu, &mut source, (64, 64), 0.0);
        let greens: std::collections::HashSet<u8> = pixels.chunks(4).map(|p| p[1]).collect();
        assert!(greens.len() > 64, "noise, not a flat color: {}", greens.len());
    }

    /// Renders one frame of `source` at `size` and returns tightly packed BGRA.
    pub(crate) fn render(
        gpu: &Gpu,
        source: &mut dyn Source,
        size: (u32, u32),
        clock: f32,
    ) -> Vec<u8> {
        let target = gpu.texture(
            "stack test",
            size,
            gpu.format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            None,
        );
        let view = target.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let clock = Duration::from_secs_f32(clock);
        source.update(gpu, clock);
        source.render(
            gpu,
            Frame {
                screen: 0,
                size,
                mouse: [0.0, 0.0],
                clock,
                frame: 1,
                view: &view,
                encoder: &mut encoder,
            },
        );
        let row = (size.0 * 4).next_multiple_of(256);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row * size.1),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let mapped = buffer.get_mapped_range(..).unwrap();
        mapped
            .chunks(row as usize)
            .flat_map(|r| &r[..(size.0 * 4) as usize])
            .copied()
            .collect()
    }

    #[test]
    fn the_demo_theme_renders_non_black_moving_frames() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let theme = LoadedTheme::load(repo().join("themes/examples/builder-demo")).unwrap();
        let mypaper_theme::Wallpaper::Stack(config) = &theme.theme.wallpaper else {
            panic!("builder-demo is a stack");
        };
        let mut source = StackSource::new_in(&gpu, &theme, config, &roots()).unwrap();
        assert!(source.errors.is_empty(), "{:?}", source.errors);
        assert!(!source.still(), "rain moves");
        let size = source.render_size((960, 540));
        let a = render(&gpu, &mut source, size, 1.0);
        let lit = a
            .chunks(4)
            .filter(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) > 30)
            .count();
        assert!(lit > a.len() / 4 / 2, "only {lit} lit pixels");
        let b = render(&gpu, &mut source, size, 1.7);
        assert_ne!(a, b, "the rain moves between frames");
        if let Ok(out) = std::env::var("MYPAPER_STACK_DUMP") {
            std::fs::write(&out, &b).unwrap();
            eprintln!("{size:?} BGRA -> {out}");
        }
    }

    /// GPU ms of each reference effect at 1920x1080 over the demo photo, minus a layer-free
    /// baseline (`cargo test -p mypaper-wallpaper --release time_effects -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn time_effects() {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let size = (1920, 1080);
        let dir = repo().join("themes/examples/builder-demo");
        let time = |layers: &str| -> f64 {
            let json = format!(
                r#"{{ "name": "t", "wallpaper": {{ "kind": "stack",
                "base": {{ "kind": "image", "source": "media/city.jpg" }}, "layers": [{layers}] }} }}"#
            );
            let theme = LoadedTheme::parse(dir.clone(), &json).unwrap();
            let mypaper_theme::Wallpaper::Stack(config) = &theme.theme.wallpaper else {
                unreachable!()
            };
            let mut source = StackSource::new_in(&gpu, &theme, config, &roots()).unwrap();
            assert!(source.errors.is_empty(), "{:?}", source.errors);
            let target = gpu.texture(
                "timing",
                size,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                None,
            );
            let view = target.create_view(&Default::default());
            let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            });
            let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let period = f64::from(gpu.queue.get_timestamp_period());
            let mut samples = Vec::new();
            for frame in 0..40u32 {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                encoder.write_timestamp(&queries, 0);
                source.render(
                    &gpu,
                    Frame {
                        screen: 0,
                        size,
                        mouse: [0.0, 0.0],
                        clock: Duration::from_millis(u64::from(frame) * 33),
                        frame,
                        view: &view,
                        encoder: &mut encoder,
                    },
                );
                encoder.write_timestamp(&queries, 1);
                encoder.resolve_query_set(&queries, 0..2, &resolve, 0);
                encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16);
                gpu.queue.submit([encoder.finish()]);
                read.map_async(wgpu::MapMode::Read, .., |_| {});
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                let stamps: Vec<u64> = read
                    .get_mapped_range(..)
                    .unwrap()
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|b| u64::from_ne_bytes(*b))
                    .collect();
                read.unmap();
                if frame >= 10 {
                    samples.push(stamps[1].saturating_sub(stamps[0]) as f64 * period / 1e6);
                }
            }
            samples.sort_by(f64::total_cmp);
            samples[samples.len() / 2]
        };
        let baseline = time("");
        eprintln!("baseline (image + copy): {baseline:.3} ms");
        for id in ["rain", "blur", "color", "vignette", "grain"] {
            let ms = time(&format!(r#"{{ "kind": "effect", "effect": "{id}" }}"#));
            eprintln!("{id}: {:.3} ms (total {ms:.3})", (ms - baseline).max(0.0));
        }
        let demo = LoadedTheme::load(&dir).unwrap();
        let mypaper_theme::Wallpaper::Stack(config) = &demo.theme.wallpaper else {
            unreachable!()
        };
        let layers = serde_json::to_string(&config.layers).unwrap();
        let layers = &layers[1..layers.len() - 1];
        eprintln!("demo stack, every layer each frame: {:.3} ms", time(layers));
    }
}
