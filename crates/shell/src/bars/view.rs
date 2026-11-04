//! Pure display lists for the top bar and dock, shared by live windows and goldens.
use super::{
    look::{self, Tokens},
    services::Model,
};
use mypaper_bars::{
    BarsConfig, Module, ModuleRef,
    actions::BarKind,
    apps::Item,
    config::{Dimension, DockPosition, DockStyle, Extra, IndicatorStyle, ItemsAlign, TopLayout},
    network,
    style::{Family, Paint},
};
use mypaper_ui::{DisplayList, Hit, Op, Point, Rect, Rgba, Role, TextStyle, Ui};

#[derive(Clone, Debug)]
pub enum Target {
    Module(ModuleRef),
    Workspace(usize),
    Tray(String),
    Dock(Item),
    Extra(Extra),
}

pub struct Frame {
    pub list: DisplayList,
    pub targets: Vec<Target>,
    pub shapes: Vec<(Rect, f32)>,
    pub size: (u32, u32),
}

impl Frame {
    fn new(size: (u32, u32)) -> Self {
        Self {
            list: DisplayList::default(),
            targets: vec![],
            shapes: vec![],
            size,
        }
    }
    fn panel(&mut self, rect: Rect, radius: f32, tokens: &Tokens) {
        self.shapes.push((rect, radius));
        self.list.ops.push(Op::Fill {
            rect,
            radius,
            color: Rgba::HIT,
        });
        self.list.ops.push(Op::Fill {
            rect,
            radius,
            color: tokens.fill.with_alpha(tokens.fill.a.max(1.0 / 255.0)),
        });
        self.list.ops.push(Op::Stroke {
            rect: rect.inflate(-0.5),
            radius,
            width: 1.0,
            color: tokens.rim,
        });
    }
    fn hit(&mut self, rect: Rect, label: &str, target: Target) -> u32 {
        self.targets.push(target);
        let id = self.targets.len() as u32;
        self.list.hits.push(Hit {
            id,
            rect,
            role: Role::Button,
            label: label.into(),
            enabled: true,
            checked: None,
        });
        id
    }
    pub fn target(&self, at: Point) -> Option<(u32, Target)> {
        self.list
            .hit_at(at)
            .map(|h| (h.id, self.targets[h.id as usize - 1].clone()))
    }
}

pub use super::icons::Images;

#[derive(Clone)]
struct Content {
    text: String,
    glyph: Option<&'static str>,
    icon: Option<String>,
    target: Target,
    width: f32,
}

fn contents(
    module: &ModuleRef,
    model: &Model,
    config: &BarsConfig,
    kind: BarKind,
    ui: &mut Ui,
    tokens: &Tokens,
    scale: f32,
) -> Vec<Content> {
    let target = Target::Module(module.clone());
    let mut output = Vec::new();
    let mut add =
        |text: String, glyph: Option<&'static str>, icon: Option<String>, target: Target| {
            let options = config.modules.get(module);
            let font = options
                .map_or(tokens.font.clone(), |o| TextStyle {
                    size: o.style.size.unwrap_or(tokens.font.size),
                    weight: o.style.weight.unwrap_or(tokens.font.weight),
                    ..tokens.font.clone()
                })
                .scaled(scale);
            let width = ui.text.measure(&text, &font).0.min(240.0 * scale)
                + if glyph.is_some() || icon.is_some() {
                    20.0 * scale
                } else {
                    0.0
                };
            output.push(Content {
                text,
                glyph,
                icon,
                target,
                width: (width + options.and_then(|o| o.style.padding).unwrap_or(7.0) * 2.0 * scale)
                    .max(28.0 * scale),
            });
        };
    let ModuleRef::Builtin(module) = module else {
        if let ModuleRef::Custom(id) = module
            && let Some(out) = model.custom.outputs.get(id)
        {
            let label = if out.error {
                "—".into()
            } else {
                format!("{}{}", out.icon.as_deref().unwrap_or(""), out.text)
            };
            add(label, None, None, target);
        }
        return output;
    };
    match module {
        Module::Start => add(String::new(), Some("\u{e71d}"), None, target),
        Module::Search => add(
            if tokens.family == Family::Fluent {
                "Search".into()
            } else {
                String::new()
            },
            Some("\u{e721}"),
            None,
            target,
        ),
        Module::Workspaces => {
            for i in 0..model.workspaces.count.clamp(1, 16) {
                add(
                    if i == model.workspaces.current {
                        "●"
                    } else {
                        "○"
                    }
                    .into(),
                    None,
                    None,
                    Target::Workspace(i),
                );
            }
        }
        Module::App => add(model.app_name.clone(), None, model.app_icon.clone(), target),
        Module::Clock => {
            let c = if kind == BarKind::Top {
                &config.top.clock
            } else {
                &config.dock.clock
            };
            add(
                mypaper_bars::clock::format(c, &model.clock),
                None,
                None,
                target,
            );
        }
        Module::Media => {
            if let Some(m) = &model.media {
                add(
                    format!(
                        "{}{}{}",
                        m.title,
                        if m.artist.is_empty() { "" } else { " — " },
                        m.artist
                    ),
                    Some(if m.playing { "\u{e769}" } else { "\u{e768}" }),
                    None,
                    target,
                );
            }
        }
        Module::Tray => {
            for item in model
                .tray
                .iter()
                .filter(|i| i.pinned && i.id != mypaper_bars::tray::OVERFLOW)
            {
                add(
                    String::new(),
                    None,
                    item.icon.clone(),
                    Target::Tray(item.id.clone()),
                );
            }
            add(
                String::new(),
                Some("\u{e70e}"),
                None,
                Target::Tray(mypaper_bars::tray::OVERFLOW.into()),
            );
        }
        Module::Network => add(
            String::new(),
            Some(match model.network.as_ref().map(|n| n.kind) {
                Some(network::Kind::Wifi) => "\u{e701}",
                Some(network::Kind::Ethernet) => "\u{e839}",
                _ => "\u{e871}",
            }),
            None,
            target,
        ),
        Module::Volume => {
            if let Some(v) = &model.volume {
                add(
                    String::new(),
                    Some(if v.muted {
                        "\u{e74f}"
                    } else if v.level < 0.33 {
                        "\u{e993}"
                    } else {
                        "\u{e767}"
                    }),
                    None,
                    target,
                );
            }
        }
        Module::Battery => {
            if let Some(b) = &model.battery {
                add(
                    format!("{}%", b.percent),
                    Some(if b.charging { "\u{e945}" } else { "\u{e850}" }),
                    None,
                    target,
                );
            }
        }
        Module::Quick => add(String::new(), Some("\u{e713}"), None, target),
        Module::Power => add(String::new(), Some("\u{e7e8}"), None, target),
        Module::Stats => {
            if let Some(s) = &model.stats {
                add(
                    format!(
                        "CPU {:.0}%  RAM {:.0}%",
                        s.cpu,
                        100.0 * s.memory.used as f64 / s.memory.total.max(1) as f64
                    ),
                    None,
                    None,
                    target,
                );
            }
        }
        Module::Language => add(model.language.clone(), None, None, target),
        Module::Notifications => add(
            model
                .notifications
                .filter(|&n| n > 0)
                .map_or_else(String::new, |n| n.to_string()),
            Some("\u{e7f4}"),
            None,
            target,
        ),
    }
    output
}

fn text(
    list: &mut DisplayList,
    ui: &mut Ui,
    value: &str,
    rect: Rect,
    font: &TextStyle,
    color: Rgba,
) {
    // Text is deliberately truncated before shaping; no layout may overpaint a neighbor.
    let mut value = value.to_owned();
    while value.chars().count() > 1 && ui.text.measure(&value, font).0 > rect.width() {
        value.pop();
    }
    let (w, h) = ui.text.measure(&value, font);
    list.ops.push(Op::Text {
        rect: Rect::new(
            rect.left + (rect.width() - w) / 2.0,
            rect.top + (rect.height() - h) / 2.0,
            w,
            h,
        ),
        text: value.into(),
        style: font.clone(),
        color,
    });
}

#[allow(clippy::too_many_arguments)] // Explicit view inputs; no retained renderer state.
fn item(
    frame: &mut Frame,
    content: &Content,
    rect: Rect,
    config: &BarsConfig,
    tokens: &Tokens,
    ui: &mut Ui,
    images: &mut Images,
    scale: f32,
    hover: Option<u32>,
) {
    let label = if content.text.is_empty() {
        match &content.target {
            Target::Module(ModuleRef::Builtin(m)) => format!("{m:?}"),
            Target::Tray(id) => id.clone(),
            _ => String::new(),
        }
    } else {
        content.text.clone()
    };
    let id = frame.hit(rect, &label, content.target.clone());
    let options = match &content.target {
        Target::Module(m) => config.modules.get(m),
        _ => None,
    };
    let color = options
        .and_then(|o| o.style.color.as_ref())
        .map_or(tokens.ink, |c| {
            look::resolve_color(c, tokens.ink, tokens.accent)
        });
    let radius = options.and_then(|o| o.style.radius).unwrap_or(5.0) * scale;
    if let Some(Paint::Color(c)) = options.and_then(|o| o.style.background.as_ref()) {
        frame.list.ops.push(Op::Fill {
            rect,
            radius,
            color: look::resolve_color(c, tokens.ink, tokens.accent),
        });
    }
    if let Some(border) = options.and_then(|o| o.style.border.as_ref())
        && let Some(Paint::Color(color)) = &border.color
    {
        let width = border.width.unwrap_or(1.0) * scale;
        frame.list.ops.push(Op::Stroke {
            rect: rect.inflate(-width / 2.0),
            radius,
            width,
            color: look::resolve_color(color, tokens.ink, tokens.accent),
        });
    }
    if Some(id) == hover {
        frame.list.ops.push(Op::Fill {
            rect: rect.inflate(-2.0 * scale),
            radius,
            color: tokens.hover,
        });
    }
    let mut label = rect;
    let key = options
        .and_then(|o| o.icon.as_deref())
        .or(match &content.target {
            Target::Tray(_) => Some("chevron"),
            Target::Module(ModuleRef::Builtin(m)) => Some(match m {
                Module::Start => "start",
                Module::Search => "search",
                Module::Network => "wifi-3",
                Module::Volume => "volume-high",
                Module::Quick => "quick",
                Module::Power => "power",
                Module::Battery => "battery",
                Module::Media => "play",
                _ => "",
            }),
            _ => None,
        });
    let packed = key.and_then(|key| images.key(key));
    let icon = packed
        .as_ref()
        .and_then(|(source, mono)| images.load(ui, source, mono.then_some(color)))
        .or_else(|| {
            options
                .and_then(|o| o.icon.as_deref())
                .or(content.icon.as_deref())
                .and_then(|source| images.get(ui, source))
        });
    let g = Rect::new(
        rect.left + 7.0 * scale,
        rect.top + (rect.height() - 16.0 * scale) / 2.0,
        16.0 * scale,
        16.0 * scale,
    );
    if let Some(image) = icon {
        frame.list.ops.push(Op::Image {
            rect: g,
            image,
            opacity: 1.0,
            source: None,
        });
        label.left += 20.0 * scale;
    } else if let Some(glyph) = content.glyph {
        text(
            &mut frame.list,
            ui,
            glyph,
            g,
            &TextStyle::new(&mypaper_ui::icons::system_family(&ui.text), 15.0 * scale),
            color,
        );
        label.left += 20.0 * scale;
    }
    let font = options
        .map_or(tokens.font.clone(), |o| TextStyle {
            size: o.style.size.unwrap_or(tokens.font.size),
            weight: o.style.weight.unwrap_or(tokens.font.weight),
            ..tokens.font.clone()
        })
        .scaled(scale);
    text(
        &mut frame.list,
        ui,
        &content.text,
        label.inflate(-2.0 * scale),
        &font,
        color,
    );
}

fn zones(
    entries: &[&[ModuleRef]],
    model: &Model,
    config: &BarsConfig,
    kind: BarKind,
    ui: &mut Ui,
    tokens: &Tokens,
    scale: f32,
) -> Vec<Vec<Vec<Content>>> {
    let mut seen = std::collections::BTreeSet::new();
    entries
        .iter()
        .map(|zone| {
            let mut groups = vec![vec![]];
            for module in *zone {
                if *module == ModuleRef::Break {
                    groups.push(vec![]);
                    continue;
                }
                if seen.insert(module.clone()) {
                    groups
                        .last_mut()
                        .unwrap()
                        .extend(contents(module, model, config, kind, ui, tokens, scale));
                }
            }
            groups.into_iter().filter(|g| !g.is_empty()).collect()
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn top(
    width: u32,
    config: &BarsConfig,
    model: &Model,
    tokens: &Tokens,
    scale: f32,
    ui: &mut Ui,
    images: &mut Images,
    hover: Option<u32>,
) -> Frame {
    let t = &config.top;
    let m = t.margin() as f32 * scale;
    let h = t.height as f32 * scale;
    let pad = t.padding.unwrap_or(5.0) as f32 * scale;
    let gap = t.spacing.unwrap_or(1.0) as f32 * scale;
    let groups = zones(
        &[&t.left, &t.center, &t.right],
        model,
        config,
        BarKind::Top,
        ui,
        tokens,
        scale,
    );
    let group_width = |g: &Vec<Content>| {
        g.iter().map(|c| c.width).sum::<f32>() + gap * g.len().saturating_sub(1) as f32 + pad * 2.0
    };
    let widths: Vec<_> = groups
        .iter()
        .map(|z| z.iter().map(group_width).sum::<f32>() + m * z.len().saturating_sub(1) as f32)
        .collect();
    let available = (width as f32 - 2.0 * m).max(1.0);
    let w = match t.width.unwrap_or(Dimension::Full) {
        Dimension::Full => available,
        Dimension::Auto => widths.iter().sum::<f32>() + 32.0 * scale,
        Dimension::Dips(d) => d as f32 * scale,
        Dimension::Percent(p) => available * p as f32 / 100.0,
    }
    .min(available);
    let left = (width as f32 - w) / 2.0;
    let mut frame = Frame::new((width, (h + 2.0 * m).ceil() as u32));
    if t.layout != TopLayout::Islands {
        frame.panel(
            Rect::new(left, m, w, h),
            if t.layout == TopLayout::Strip {
                0.0
            } else {
                tokens.radius * scale
            },
            tokens,
        );
    }
    let starts = [left, left + (w - widths[1]) / 2.0, left + w - widths[2]];
    for (z, groups) in groups.iter().enumerate() {
        let mut x = starts[z];
        for group in groups {
            let gw = group_width(group);
            if t.layout == TopLayout::Islands {
                frame.panel(Rect::new(x, m, gw, h), tokens.radius * scale, tokens);
            }
            x += pad;
            for c in group {
                let rect = Rect::new(x, m, c.width, h);
                if rect.right <= width as f32 && rect.left >= 0.0 {
                    item(
                        &mut frame, c, rect, config, tokens, ui, images, scale, hover,
                    );
                }
                x += c.width + gap;
            }
            x += pad - gap + m;
        }
    }
    frame
}

/// Stable cropped envelope: magnification changes content and plates, never monitor-sized buffers.
#[allow(clippy::too_many_arguments)]
pub fn dock(
    config: &BarsConfig,
    model: &Model,
    apps: &[Item],
    tokens: &Tokens,
    scale: f32,
    extent: (u32, u32),
    zoom: &[(f32, f32)],
    hops: &[f32],
    ui: &mut Ui,
    images: &mut Images,
    hover: Option<u32>,
) -> Frame {
    let d = &config.dock;
    let horizontal = matches!(d.position, DockPosition::Bottom | DockPosition::Top);
    let taskbar = d.style == DockStyle::Taskbar;
    let size = d.size() as f32 * scale;
    let pad = d.padding.unwrap_or(5.0) as f32 * scale;
    let gap = d.spacing.unwrap_or(6.0) as f32 * scale;
    let length = if taskbar {
        if d.titles && horizontal {
            168.0 * scale
        } else {
            d.button.unwrap_or(d.size() + 16.0) as f32 * scale
        }
    } else {
        size
    };
    let groups = zones(
        &[&d.left, &d.right],
        model,
        config,
        BarKind::Dock,
        ui,
        tokens,
        scale,
    );
    let side_width = |i: usize| {
        if taskbar {
            groups[i]
                .iter()
                .flatten()
                .map(|c| c.width + gap)
                .sum::<f32>()
        } else {
            0.0
        }
    };
    let sides = [side_width(0), side_width(1)];
    let base =
        apps.len() as f32 * (length + gap) + d.extras.len() as f32 * (size + gap) + 2.0 * pad - gap;
    let along = if horizontal { extent.0 } else { extent.1 } as f32;
    let extra = if taskbar {
        0.0
    } else {
        size * (d.magnify.scale as f32 - 1.0) * d.magnify.range as f32 * 2.0
    };
    let envelope = if taskbar {
        along
    } else {
        (base + extra + 24.0 * scale).min(along)
    }
    .max(1.0);
    let across = if taskbar {
        size + 2.0 * pad
    } else {
        size * d.magnify.scale as f32 + pad * 2.0 + 24.0 * scale
    };
    let dimensions = if horizontal {
        (envelope.ceil() as u32, across.ceil() as u32)
    } else {
        (across.ceil() as u32, envelope.ceil() as u32)
    };
    let mut frame = Frame::new(dimensions);
    let convert = |a: f32, b: f32, w: f32, h: f32| {
        if horizontal {
            Rect::new(a, b, w, h)
        } else {
            Rect::new(b, a, h, w)
        }
    };
    let forward = matches!(d.position, DockPosition::Top | DockPosition::Left);
    let edge = if forward { pad } else { across - size - pad };
    let start = if taskbar {
        if d.align == ItemsAlign::Start {
            pad + sides[0]
        } else {
            ((envelope - base) / 2.0).max(sides[0] + pad)
        }
    } else {
        (envelope - base) / 2.0
    };
    let minimum = zoom.iter().map(|v| v.1).fold(0.0, f32::min);
    let maximum = zoom.iter().map(|v| v.1).fold(0.0, f32::max);
    let panel_start = if taskbar { 0.0 } else { start + minimum };
    let panel_length = if taskbar {
        envelope
    } else {
        base + maximum - minimum
    };
    frame.panel(
        convert(panel_start, edge - pad, panel_length, size + pad * 2.0),
        tokens.radius * scale,
        tokens,
    );
    for (i, zone) in groups.iter().enumerate().take(2) {
        if !taskbar {
            break;
        }
        let mut a = if i == 0 {
            pad
        } else {
            envelope - sides[1] - pad
        };
        for c in zone.iter().flatten() {
            item(
                &mut frame,
                c,
                convert(a, 0.0, c.width, across),
                config,
                tokens,
                ui,
                images,
                scale,
                hover,
            );
            a += c.width + gap;
        }
    }
    for (i, app) in apps.iter().enumerate() {
        let (factor, shift) = zoom.get(i).copied().unwrap_or((1.0, 0.0));
        let a = start + pad + i as f32 * (length + gap) + shift;
        let rect = convert(a, edge, length, size);
        if rect.right > dimensions.0 as f32
            || rect.bottom > dimensions.1 as f32
            || rect.left < 0.0
            || rect.top < 0.0
        {
            continue;
        }
        let s = size * factor;
        let hop = hops.get(i).copied().unwrap_or(0.0);
        let b = if forward {
            edge + hop
        } else {
            edge + size - s - hop
        };
        let icon_rect = convert(
            a + if taskbar && d.titles && horizontal {
                0.0
            } else {
                (length - s) / 2.0
            },
            b,
            s,
            s,
        );
        let id = frame.hit(rect.union(icon_rect), &app.name, Target::Dock(app.clone()));
        let active = app.windows.iter().any(|w| w.active);
        if Some(id) == hover || active && taskbar {
            frame.list.ops.push(Op::Fill {
                rect: rect.inflate(3.0 * scale),
                radius: 5.0 * scale,
                color: tokens.hover,
            });
        }
        let packed = images.app(app);
        let image = packed
            .as_ref()
            .and_then(|(source, mono)| images.load(ui, source, mono.then_some(tokens.ink)))
            .or_else(|| app.icon.as_deref().and_then(|s| images.get(ui, s)));
        if let Some(image) = image {
            frame.list.ops.push(Op::Image {
                rect: icon_rect,
                image,
                opacity: 1.0,
                source: None,
            });
        } else {
            text(
                &mut frame.list,
                ui,
                &app.name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
                icon_rect,
                &TextStyle::new("Segoe UI", size * 0.55),
                tokens.ink,
            );
        }
        if taskbar && d.titles && horizontal {
            text(
                &mut frame.list,
                ui,
                app.windows
                    .first()
                    .map_or(app.name.as_str(), |w| w.title.as_str()),
                Rect::new(
                    a + size + 8.0 * scale,
                    edge,
                    length - size - 8.0 * scale,
                    size,
                ),
                &tokens.font.scaled(scale),
                tokens.ink,
            );
        }
        if !app.windows.is_empty() && d.indicator != IndicatorStyle::None {
            let w = if active { 12.0 } else { 4.0 } * scale;
            let attention = app.windows.iter().any(|w| w.attention);
            let custom = if attention {
                d.look.indicator.attention.as_ref()
            } else if active {
                d.look.indicator.active.as_ref()
            } else {
                d.look.indicator.running.as_ref()
            };
            let color = custom.map_or_else(
                || {
                    if attention {
                        Rgba::rgb8([255, 174, 66], 1.0)
                    } else if active {
                        tokens.accent
                    } else {
                        tokens.ink.with_alpha(0.65)
                    }
                },
                |c| look::resolve_color(c, tokens.ink, tokens.accent),
            );
            frame.list.ops.push(Op::Fill {
                rect: convert(
                    a + (length - w) / 2.0,
                    if forward { 1.0 } else { across - 3.0 * scale },
                    w,
                    2.0 * scale,
                ),
                radius: 2.0 * scale,
                color,
            });
        }
        if d.counter && app.windows.len() > 1 {
            let badge = Rect::new(
                icon_rect.right - 14.0 * scale,
                icon_rect.top,
                16.0 * scale,
                16.0 * scale,
            );
            frame.list.ops.push(Op::Fill {
                rect: badge,
                radius: 8.0 * scale,
                color: tokens.accent,
            });
            text(
                &mut frame.list,
                ui,
                &app.windows.len().min(99).to_string(),
                badge,
                &TextStyle::new("Segoe UI", 10.0 * scale),
                tokens.fill.with_alpha(1.0),
            );
        }
    }
    for (i, extra) in d.extras.iter().enumerate() {
        let a =
            start + pad + apps.len() as f32 * (length + gap) + i as f32 * (size + gap) + maximum;
        let rect = convert(a, edge, size, size);
        frame.hit(
            rect,
            if *extra == Extra::Desktop {
                "Show desktop"
            } else if model.trash_full {
                "Recycle Bin (full)"
            } else {
                "Recycle Bin"
            },
            Target::Extra(*extra),
        );
        text(
            &mut frame.list,
            ui,
            if *extra == Extra::Desktop {
                "\u{e7f4}"
            } else {
                "\u{e74d}"
            },
            rect,
            &TextStyle::new(&mypaper_ui::icons::system_family(&ui.text), 24.0 * scale),
            if *extra == Extra::Trash && model.trash_full {
                tokens.accent
            } else {
                tokens.ink
            },
        );
    }
    frame
}
