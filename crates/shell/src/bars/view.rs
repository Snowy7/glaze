//! The top bar and dock as kit display lists, shared by the live windows, Studio's preview and
//! the goldens.
//!
//! Everything is drawn with [`kit::Frame`] from the bar's [`Theme`](mypaper_ui::Theme)
//! (`look.rs`): the body is the kit's surface (an invisible hit fill over engine glass, or the
//! style's own fill), items are washes, the kit's icons and the theme's type, and hover, press
//! and keyboard focus come from the kit's [`Interaction`]. Nothing draws an outline on glass.
//! Items sit on whole device pixels, so text, icons and tray bitmaps stay crisp at every DPI.
use super::{look::Tokens, services::Model};
use mypaper_bars::{
    BarsConfig, Module, ModuleRef,
    actions::BarKind,
    apps::Item,
    config::{
        Dimension, DockPosition, DockStyle, Extra, IndicatorStyle, ItemsAlign, TopLayout,
        TopPosition,
    },
    network,
    presets::TopDesign,
    style::{Family, Paint},
};
use mypaper_ui::{
    Align, DisplayList, Image, Insets, Interaction, Op, Point, Rect, Rgba, Role, TextRole,
    TextStyle, Ui, icons::Icon, interact::State, kit,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Target {
    Module(ModuleRef),
    Workspace(usize),
    Tray(String),
    Dock(Item),
    Extra(Extra),
}

/// Whether a click on `target` activates the bar: modules that open a flyout or start
/// something, and tray icons (which hand their app the foreground), need it, or Windows refuses
/// them while a shell window (Search, Start, a toast) holds the foreground. Media and the app
/// title act in place and leave the app in front focused; dock items must too, since a click
/// on the active app's icon minimizes it.
pub fn claims_foreground(target: &Target) -> bool {
    match target {
        Target::Module(ModuleRef::Builtin(m)) => !matches!(m, Module::Media | Module::App),
        Target::Module(_) | Target::Tray(_) => true,
        Target::Workspace(_) | Target::Dock(_) | Target::Extra(_) => false,
    }
}

/// A bar's display list, what each hit id targets (id − 1 indexes `targets`), and the bodies
/// the engine puts glass under.
pub struct Frame {
    pub list: DisplayList,
    pub targets: Vec<Target>,
    pub shapes: Vec<(Rect, f32)>,
    pub size: (u32, u32),
    /// The dock's magnifiable slots (apps, then extras): their resting centres along it.
    pub rest: Vec<f32>,
}

impl Frame {
    pub fn target(&self, at: Point) -> Option<(u32, Target)> {
        self.list
            .hit_at(at)
            .map(|h| (h.id, self.targets[h.id as usize - 1].clone()))
    }

    /// A cropped island keeps its original hit IDs, with only its own accessible focus node.
    pub fn crop(&self, crop: Rect) -> (DisplayList, Point, (u32, u32)) {
        let offset = Point::new(crop.left.floor(), crop.top.floor());
        let mut list = self.list.clone();
        list.ops.retain(|op| op.bounds().intersects(&crop));
        list.hits.retain(|h| h.rect.intersects(&crop));
        list.focus = list
            .focus
            .filter(|id| list.hits.iter().any(|h| h.id == *id));
        let size = (
            (crop.right.ceil() - offset.x) as u32,
            (crop.bottom.ceil() - offset.y) as u32,
        );
        (list.translated(-offset.x, -offset.y), offset, size)
    }
}

pub use super::icons::Images;

/// Targets and glass bodies collected while the kit frame draws.
#[derive(Default)]
struct Out {
    targets: Vec<Target>,
    shapes: Vec<(Rect, f32)>,
}

impl Out {
    /// The hit id of a new target.
    fn add(&mut self, target: Target) -> u32 {
        self.targets.push(target);
        self.targets.len() as u32
    }

    fn finish(self, list: DisplayList, size: (u32, u32)) -> Frame {
        Frame {
            list,
            targets: self.targets,
            shapes: self.shapes,
            size,
            rest: vec![],
        }
    }
}

/// A body: the surface's hit fill, then the kit's surface (nothing over glass; the style's fill,
/// and Solid's hairline, without) or the config's own background colour.
fn panel(f: &mut kit::Frame, out: &mut Out, rect: Rect, radius: f32, tokens: &Tokens) {
    out.shapes.push((rect, radius));
    f.fill(rect, radius, Rgba::HIT);
    match tokens.fill {
        Some(color) => {
            f.fill(rect, radius, color);
            if let Some(color) = tokens.theme.border {
                f.push(Op::Stroke {
                    rect: rect.inflate(-0.5),
                    radius: (radius - 0.5).max(0.0),
                    width: 1.0,
                    color,
                });
            }
        }
        None => {
            f.surface_shaped(rect, radius, tokens.material.is_some());
        }
    }
}

/// What leads an item.
#[derive(Clone)]
enum Glyph {
    None,
    /// The theme's icon.
    Icon(Icon),
    /// A bitmap at its exact device size (tray icons, app icons, icon packs).
    Image(Arc<Image>),
    /// A glyph of the system icon font that an integration names.
    Font(&'static str),
    /// A short text icon (an emoji from a custom module), drawn in colour.
    Emoji(String),
}

#[derive(Clone)]
struct Content {
    text: String,
    glyph: Glyph,
    target: Target,
    /// Device px, whole.
    width: f32,
    /// The glyph's size, device px, whole.
    icon: f32,
    tooltip: Option<String>,
    color: Rgba,
    /// Device px.
    font: TextStyle,
    /// The current desktop's workspace dot.
    current: bool,
}

/// The theme's icon for a built-in module, from the live model.
fn module_icon(module: Module, model: &Model) -> Option<Icon> {
    Some(match module {
        Module::Start => Icon::Apps,
        Module::Search => Icon::Search,
        Module::Network => match model.network.as_ref().map(|n| n.kind) {
            Some(network::Kind::Wifi) => {
                let signal = model.network.as_ref().and_then(|n| n.signal).unwrap_or(100);
                Icon::Wifi((signal / 25).min(3) as u8)
            }
            Some(network::Kind::Ethernet) => Icon::Ethernet,
            _ => Icon::Offline,
        },
        Module::Volume => {
            let v = model.volume.as_ref()?;
            if v.muted {
                Icon::Mute
            } else {
                Icon::Volume((v.level * 3.0).ceil().clamp(0.0, 3.0) as u8)
            }
        }
        Module::Battery => {
            let b = model.battery.as_ref()?;
            if b.charging {
                Icon::Charging
            } else {
                Icon::Battery(((b.percent as f32 + 5.0) / 10.0).clamp(0.0, 10.0) as u8)
            }
        }
        Module::Quick => Icon::Controls,
        Module::Power => Icon::Power,
        Module::Notifications => Icon::Bell,
        Module::Media => {
            if model.media.as_ref()?.playing {
                Icon::Pause
            } else {
                Icon::Play
            }
        }
        _ => return None,
    })
}

/// The icon-pack key that replaces `icon` (a user's pack can draw its own).
fn pack_key(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Apps => "start",
        Icon::Search => "search",
        Icon::Wifi(0) => "wifi-1",
        Icon::Wifi(1) => "wifi-2",
        Icon::Wifi(2) => "wifi-3",
        Icon::Wifi(_) => "wifi-4",
        Icon::Ethernet => "ethernet",
        Icon::Offline => "offline",
        Icon::Mute => "volume-mute",
        Icon::Volume(0 | 1) => "volume-low",
        Icon::Volume(2) => "volume-mid",
        Icon::Volume(_) => "volume-high",
        Icon::Controls => "quick",
        Icon::Power => "power",
        Icon::Battery(_) | Icon::Charging => "battery",
        Icon::ChevronDown | Icon::ChevronUp => "chevron",
        Icon::Play => "play",
        Icon::Pause => "pause",
        Icon::Delete => "trash",
        Icon::Desktop => "desktop",
        _ => return None,
    })
}

/// `icon`, or a user icon pack's image for it at `px` device px.
fn themed(icon: Icon, color: Rgba, px: u32, ui: &Ui, images: &mut Images, full: bool) -> Glyph {
    let key = match (icon, full) {
        (Icon::Delete, true) => Some("trash-full"),
        _ => pack_key(icon),
    };
    key.and_then(|key| images.custom(key))
        .and_then(|(source, mono)| images.sized(ui, &source, mono.then_some(color), px))
        .map_or(Glyph::Icon(icon), Glyph::Image)
}

/// The text style of a module: the bar's font, the design's weight, the module's overrides.
fn module_font(
    module: &ModuleRef,
    config: &BarsConfig,
    kind: BarKind,
    tokens: &Tokens,
) -> TextStyle {
    let design = if kind == BarKind::Top {
        config.top.design
    } else {
        TopDesign::Monochrome
    };
    let weight = match (module, design) {
        // The macOS menu bar's bold app name.
        (ModuleRef::Builtin(Module::App), TopDesign::Macos) if kind == BarKind::Top => 700,
        (ModuleRef::Builtin(Module::Clock), TopDesign::Monochrome) if kind == BarKind::Top => {
            tokens.theme.font.font(TextRole::Headline).weight
        }
        _ => tokens.font.weight,
    };
    let style = config.modules.get(module).map(|o| &o.style);
    // A short top bar takes smaller type, so text keeps clear of the bar's rim (macOS's 24 px
    // menu bar sets 13 px).
    let room = if kind == BarKind::Top {
        (config.top.height as f32 * 0.56).round()
    } else {
        f32::MAX
    };
    let size = style
        .and_then(|s| s.size)
        .unwrap_or(tokens.font.size.min(room));
    TextStyle {
        size,
        weight: style.and_then(|s| s.weight).unwrap_or(weight),
        optical: size * 0.75,
        ..tokens.font.clone()
    }
}

/// A module glyph's size in design px: the bar's `icon`, else for a top bar a little over half
/// its height up to 16 (14 in macOS's 24 px menu bar), and in a taskbar 16 with Start at the
/// app icons' size (Windows' 24 px logo).
fn icon_size(
    module: &ModuleRef,
    config: &BarsConfig,
    kind: BarKind,
    theme: &mypaper_ui::Theme,
) -> f32 {
    if kind == BarKind::Top {
        return config.top.icon.map_or_else(
            || {
                (config.top.height as f32 * 0.58)
                    .round()
                    .clamp(12.0, theme.metrics.icon)
            },
            |v| v as f32,
        );
    }
    match (module, config.dock.icon) {
        (_, Some(icon)) => icon as f32,
        (ModuleRef::Builtin(Module::Start), None) => (config.dock.size() as f32).min(24.0),
        _ => theme.metrics.icon,
    }
}

#[allow(clippy::too_many_arguments)] // Explicit view inputs; no retained renderer state.
fn contents(
    module: &ModuleRef,
    model: &Model,
    config: &BarsConfig,
    kind: BarKind,
    ui: &mut Ui,
    images: &mut Images,
    tokens: &Tokens,
    scale: f32,
) -> Vec<Content> {
    let theme = &tokens.theme;
    let px = |v: f32| (v * scale).round();
    let options = config.modules.get(module);
    let font = module_font(module, config, kind, tokens).scaled(scale);
    let color = options
        .and_then(|o| o.style.color.as_ref())
        .map_or(theme.color.label, |c| tokens.color(c));
    let icon_px = px(icon_size(module, config, kind, theme)) as u32;
    let pad = px(options.and_then(|o| o.style.padding).unwrap_or(7.0));
    let target = Target::Module(module.clone());
    let fluent_search =
        matches!(module, ModuleRef::Builtin(Module::Search)) && tokens.family == Family::Fluent;
    let mut output = Vec::new();
    // An item's glyph from the module's own `icon` option, else `glyph`.
    let mut add = |text: String, glyph: Glyph, target: Target, ui: &mut Ui, images: &mut Images| {
        let glyph = match options.and_then(|o| o.icon.as_deref()) {
            Some(source) => images
                .sized(ui, source, svg(source).then_some(color), icon_px)
                .map_or(glyph, Glyph::Image),
            None => glyph,
        };
        let text_w = if text.is_empty() {
            0.0
        } else {
            ui.text.measure(&text, &font).0.min(px(240.0)).ceil()
        };
        let lead = match glyph {
            Glyph::None => 0.0,
            _ if text.is_empty() => icon_px as f32,
            _ => icon_px as f32 + px(theme.metrics.item),
        };
        let width = if fluent_search {
            px(200.0)
        } else {
            (text_w + lead + 2.0 * pad).max(px(theme.metrics.hit))
        };
        output.push(Content {
            text,
            glyph,
            target,
            width,
            icon: icon_px as f32,
            tooltip: None,
            color,
            font: font.clone(),
            current: false,
        });
    };
    let ModuleRef::Builtin(builtin) = module else {
        if let ModuleRef::Custom(id) = module
            && let Some(out) = model.custom.outputs.get(id)
        {
            let label = if out.error {
                "—".into()
            } else {
                out.text.clone()
            };
            let glyph = match out.icon.as_deref() {
                Some(icon) if icon.chars().count() <= 4 => Glyph::Emoji(icon.into()),
                Some(source) => images
                    .sized(ui, source, svg(source).then_some(color), icon_px)
                    .map_or(Glyph::None, Glyph::Image),
                None => Glyph::None,
            };
            add(label, glyph, target, ui, images);
            if let Some(c) = output.last_mut() {
                c.tooltip = out.tooltip.clone();
                if out.error {
                    c.color = theme.color.tertiary;
                }
            }
        } else if let ModuleRef::Custom(id) = module
            && let Some(label) = super::plugin_modules::label(id)
        {
            // A plugin's bar module (`custom:<plugin>--<bar>`): its view's line.
            add(label, Glyph::None, target, ui, images);
        }
        return output;
    };
    if let Some((label, glyph)) = crate::integrations::bar_content(module) {
        if !label.is_empty() {
            add(label, Glyph::Font(glyph), target, ui, images);
        }
        return output;
    }
    let icon = |ui: &Ui, images: &mut Images| {
        module_icon(*builtin, model).map_or(Glyph::None, |icon| {
            themed(icon, color, icon_px, ui, images, false)
        })
    };
    match builtin {
        Module::Start | Module::Quick | Module::Power => {
            let glyph = icon(ui, images);
            add(String::new(), glyph, target, ui, images)
        }
        Module::Search => {
            let glyph = icon(ui, images);
            let text = if fluent_search { "Search" } else { "" };
            add(text.into(), glyph, target, ui, images)
        }
        // Dots, drawn by `item`; with one desktop there is nothing to switch to, so nothing.
        Module::Workspaces if model.workspaces.count > 1 => {
            for i in 0..model.workspaces.count.min(16) {
                add(String::new(), Glyph::None, Target::Workspace(i), ui, images);
            }
        }
        Module::Workspaces => {}
        Module::App => {
            let glyph = model
                .app_icon
                .as_deref()
                .and_then(|source| images.sized(ui, source, None, icon_px))
                .map_or(Glyph::None, Glyph::Image);
            add(
                crate::integrations::app_title(&model.app_name),
                glyph,
                target,
                ui,
                images,
            )
        }
        Module::Clock => {
            let c = if kind == BarKind::Top {
                &config.top.clock
            } else {
                &config.dock.clock
            };
            add(
                mypaper_bars::clock::format(c, &model.clock),
                Glyph::None,
                target,
                ui,
                images,
            );
        }
        Module::Media => {
            if let Some(m) = &model.media {
                let separator = if m.artist.is_empty() { "" } else { " — " };
                let glyph = icon(ui, images);
                add(
                    format!("{}{separator}{}", m.title, m.artist),
                    glyph,
                    target,
                    ui,
                    images,
                );
            }
        }
        Module::Tray => {
            for item in model
                .tray
                .iter()
                .filter(|i| i.pinned && i.id != mypaper_bars::tray::OVERFLOW)
            {
                let glyph = item
                    .icon
                    .as_deref()
                    .and_then(|source| images.sized(ui, source, None, icon_px))
                    .map_or(Glyph::None, Glyph::Image);
                add(
                    String::new(),
                    glyph,
                    Target::Tray(item.id.clone()),
                    ui,
                    images,
                );
            }
            // The overflow opens toward the screen: down from a top bar.
            let up = kind == BarKind::Dock || config.top.position == TopPosition::Bottom;
            let chevron = if up {
                Icon::ChevronUp
            } else {
                Icon::ChevronDown
            };
            let glyph = themed(chevron, color, icon_px, ui, images, false);
            add(
                String::new(),
                glyph,
                Target::Tray(mypaper_bars::tray::OVERFLOW.into()),
                ui,
                images,
            );
        }
        Module::Network => {
            let glyph = icon(ui, images);
            add(String::new(), glyph, target, ui, images)
        }
        Module::Volume => {
            if model.volume.is_some() {
                let glyph = icon(ui, images);
                add(String::new(), glyph, target, ui, images);
            }
        }
        Module::Battery => {
            if let Some(b) = &model.battery {
                let glyph = icon(ui, images);
                add(format!("{}%", b.percent), glyph, target, ui, images);
            }
        }
        Module::Stats => {
            if let Some(s) = &model.stats {
                let memory = 100.0 * s.memory.used as f64 / s.memory.total.max(1) as f64;
                add(
                    format!("CPU {:.0}%  RAM {memory:.0}%", s.cpu),
                    Glyph::None,
                    target,
                    ui,
                    images,
                );
            }
        }
        // Windows' indicator is two lines ("ENG\nUS"); a bar shows the language only.
        Module::Language => add(
            model.language.lines().next().unwrap_or_default().into(),
            Glyph::None,
            target,
            ui,
            images,
        ),
        Module::Notifications => {
            let glyph = icon(ui, images);
            let count = model
                .notifications
                .filter(|&n| n > 0)
                .map_or_else(String::new, |n| n.to_string());
            add(count, glyph, target, ui, images)
        }
    }
    for c in &mut output {
        if let Target::Workspace(i) = c.target {
            c.current = i == model.workspaces.current;
            c.width = px(if c.current { 26.0 } else { 14.0 });
        }
        c.tooltip = match builtin {
            Module::Network => Some(model.network.as_ref().map_or(
                "Offline".into(),
                |n| match n.kind {
                    network::Kind::Wifi => format!(
                        "{} · {}% signal",
                        n.name.as_deref().unwrap_or("Wi-Fi"),
                        n.signal.unwrap_or(0)
                    ),
                    network::Kind::Ethernet => "Ethernet".into(),
                    network::Kind::None => "Offline".into(),
                },
            )),
            Module::Volume => model.volume.as_ref().map(|v| {
                format!(
                    "Volume {}%{}",
                    (v.level * 100.0).round(),
                    if v.muted { " (muted)" } else { "" }
                )
            }),
            Module::Tray => match &c.target {
                Target::Tray(id) if id == mypaper_bars::tray::OVERFLOW => {
                    Some("Hidden icons".into())
                }
                Target::Tray(id) => model
                    .tray
                    .iter()
                    .find(|i| &i.id == id)
                    .map(|i| i.name.clone()),
                _ => None,
            },
            _ => None,
        };
    }
    output
}

fn svg(source: &str) -> bool {
    source.to_ascii_lowercase().ends_with(".svg")
}

/// One bar item in `rect`: its wash (always in the list, transparent at rest), its glyph and
/// text centred as one line, its focus ring and its hit. `shadow` puts a soft copy under the
/// text and glyph, for a bar drawn with no surface straight on the wallpaper.
fn item(
    f: &mut kit::Frame,
    out: &mut Out,
    content: &Content,
    rect: Rect,
    config: &BarsConfig,
    tokens: &Tokens,
    shadow: bool,
) {
    let theme = f.theme;
    let c = theme.color;
    let id = out.add(content.target.clone());
    let options = match &content.target {
        Target::Module(m) => config.modules.get(m),
        _ => None,
    };
    // The highlight keeps clear of the bar's rim: 2 px in a 24 px menu bar, up to 5 px in a
    // taller bar (iOS's glass buttons sit well inside their plate).
    let inset = ((rect.height() / f.s - 22.0) / 2.0).clamp(2.0, 5.0);
    let wash = rect.inset(Insets::xy(f.px(1.0), f.px(inset)));
    let radius = options
        .and_then(|o| o.style.radius)
        .map_or_else(|| f.control_radius(wash.height()), |r| f.px(r));
    if let Some(Paint::Color(background)) = options.and_then(|o| o.style.background.as_ref()) {
        f.fill(wash, radius, tokens.color(background));
    }
    if tokens.family == Family::Fluent
        && matches!(
            content.target,
            Target::Module(ModuleRef::Builtin(Module::Search))
        )
    {
        // Windows 11's search box: a well, not an outline.
        f.fill(wash, f.control_radius(wash.height()), c.well);
    }
    // A border the user set on this module (never one of ours).
    if let Some(border) = options.and_then(|o| o.style.border.as_ref()) {
        let width = (border.width.unwrap_or(1.0) * f.s).round();
        let color = match &border.color {
            Some(Paint::Color(color)) => tokens.color(color),
            _ => c.separator,
        };
        if width > 0.0 {
            f.push(Op::Stroke {
                rect: wash.inflate(-width / 2.0),
                radius,
                width,
                color,
            });
        }
    }
    f.wash(id, wash, radius, true);
    let label = if let Target::Workspace(i) = content.target {
        format!("Desktop {}", i + 1)
    } else if content.text.is_empty() {
        match &content.target {
            Target::Module(ModuleRef::Builtin(m)) => format!("{m:?}"),
            Target::Tray(id) => id.clone(),
            _ => String::new(),
        }
    } else {
        content.text.clone()
    };
    if let Target::Workspace(_) = content.target {
        // One dot per desktop, the current one stretched into a pill.
        let current = content.current;
        let (w, h) = (f.px(if current { 16.0 } else { 6.0 }), f.px(6.0));
        let dot = rect.place(w, h, Align::Center, Align::Center);
        let ink = if current {
            content.color
        } else if f.state(id, true) == State::Rest {
            c.tertiary
        } else {
            c.secondary
        };
        f.fill(dot, h / 2.0, ink);
        f.focus_ring(id, wash, radius);
        f.hit(id, rect, Role::Button, &label, true, None);
        return;
    }
    // Glyph and text as one centred line.
    let icon_px = content.icon;
    let (text_w, text_h) = if content.text.is_empty() {
        (0.0, 0.0)
    } else {
        let fitted = f.text.fit(&content.text, &content.font, rect.width());
        f.text.measure(&fitted, &content.font)
    };
    let lead = match content.glyph {
        Glyph::None => 0.0,
        _ if content.text.is_empty() => icon_px,
        _ => icon_px + f.px(theme.metrics.item),
    };
    let line = rect.place(
        (lead + text_w.ceil()).min(rect.width()),
        rect.height(),
        Align::Center,
        Align::Start,
    );
    let glyph_rect = line.place(icon_px, icon_px, Align::Start, Align::Center);
    let drop = f.px(1.0).max(1.0);
    let shadow_ink = c.shadow;
    match &content.glyph {
        Glyph::None => {}
        Glyph::Icon(icon) => {
            // Media's play/pause is small and solid, as macOS draws it beside its title.
            let size = icon_px / f.s
                * if matches!(icon, Icon::Play | Icon::Pause) {
                    theme.metrics.icon_small / theme.metrics.icon
                } else {
                    1.0
                };
            if shadow {
                f.icon(glyph_rect.offset(0.0, drop), *icon, size, shadow_ink);
            }
            f.icon(glyph_rect, *icon, size, content.color);
        }
        Glyph::Image(image) => {
            f.push(Op::Image {
                rect: fit(image, glyph_rect),
                image: image.clone(),
                opacity: 1.0,
                source: None,
            });
        }
        Glyph::Font(glyph) => {
            let style = TextStyle::new(&mypaper_ui::icons::system_family(f.text), icon_px);
            let (w, h) = f.text.measure(glyph, &style);
            f.push(Op::Text {
                rect: glyph_rect.place(w, h, Align::Center, Align::Center),
                text: (*glyph).into(),
                style,
                color: content.color,
            });
        }
        Glyph::Emoji(emoji) => {
            let style = TextStyle {
                size: icon_px * 0.85,
                ..content.font.clone()
            };
            let (w, h) = f.text.measure(emoji, &style);
            f.push(Op::Text {
                rect: glyph_rect.place(w, h, Align::Center, Align::Center),
                text: emoji.as_str().into(),
                style,
                color: content.color,
            });
        }
    }
    if !content.text.is_empty() {
        let x = line.left + lead;
        let y = (rect.top + (rect.height() - text_h) / 2.0).round();
        let room = (line.right - x).max(1.0);
        if shadow {
            f.label_styled(x, y + drop, &content.text, &content.font, shadow_ink, room);
        }
        f.label_styled(x, y, &content.text, &content.font, content.color, room);
    }
    f.focus_ring(id, wash, radius);
    let hint = content.tooltip.as_deref().unwrap_or(&label);
    f.hit(id, rect, Role::Button, hint, true, None);
}

#[allow(clippy::too_many_arguments)]
fn zones(
    entries: &[&[ModuleRef]],
    model: &Model,
    config: &BarsConfig,
    kind: BarKind,
    ui: &mut Ui,
    images: &mut Images,
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
                    groups.last_mut().unwrap().extend(contents(
                        module, model, config, kind, ui, images, tokens, scale,
                    ));
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
    hot: Option<&Interaction>,
) -> Frame {
    let t = &config.top;
    let minimal = tokens.bare;
    let px = |v: f32| (v * scale).round();
    let m = px(t.margin() as f32);
    let h = px(t.height as f32);
    // A rounded bar or island keeps its first and last item off the curve of its corners.
    let corner = if t.layout == TopLayout::Strip {
        0.0
    } else {
        (px(tokens.radius) * 0.4).round()
    };
    let pad = px(t.padding.unwrap_or(5.0) as f32).max(corner);
    let gap = px(t.spacing.unwrap_or(1.0) as f32);
    let groups = zones(
        &[&t.left, &t.center, &t.right],
        model,
        config,
        BarKind::Top,
        ui,
        images,
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
    // Room between the zones, so a hugging bar never packs them into one clump.
    let zone_gap = px(24.0);
    let w = match t.width.unwrap_or(Dimension::Full) {
        Dimension::Full => available,
        // Hugging its content with the centre zone still centred: as wide as the wider side
        // twice, plus the centre.
        Dimension::Auto if widths[1] > 0.0 => {
            widths[1] + 2.0 * (widths[0].max(widths[2]) + zone_gap)
        }
        Dimension::Auto => widths[0] + widths[2] + zone_gap,
        Dimension::Dips(d) => px(d as f32),
        Dimension::Percent(p) => (available * p as f32 / 100.0).round(),
    }
    .min(available);
    let left = ((width as f32 - w) / 2.0).round();
    let input = hot.cloned().unwrap_or_default();
    let mut out = Out::default();
    let mut f = kit::Frame::new(&mut ui.text, &tokens.theme, scale, &input);
    let radius = px(tokens.radius);
    if t.layout != TopLayout::Islands && !minimal {
        let r = if t.layout == TopLayout::Strip {
            0.0
        } else {
            radius
        };
        panel(&mut f, &mut out, Rect::new(left, m, w, h), r, tokens);
    }
    // The centre zone is centred, but never over the sides: a narrow bar slides it over.
    let centred = (left + (w - widths[1]) / 2.0).round();
    let starts = [
        left,
        centred
            .max(left + widths[0] + m)
            .min((left + w - widths[2] - widths[1] - m).max(left + widths[0])),
        left + w - widths[2],
    ];
    for (z, groups) in groups.iter().enumerate() {
        let mut x = starts[z];
        for group in groups {
            let gw = group_width(group);
            if t.layout == TopLayout::Islands {
                panel(&mut f, &mut out, Rect::new(x, m, gw, h), radius, tokens);
            }
            x += pad;
            for c in group {
                let rect = Rect::new(x, m, c.width, h);
                if rect.right <= width as f32 && rect.left >= 0.0 {
                    if minimal {
                        let r = f.control_radius(h);
                        f.fill(rect, r, Rgba::HIT);
                    }
                    item(
                        &mut f,
                        &mut out,
                        c,
                        rect,
                        config,
                        tokens,
                        tokens.text_shadow,
                    );
                }
                x += c.width + gap;
            }
            x += pad - gap + m;
        }
    }
    let list = f.list;
    out.finish(list, (width, (h + 2.0 * m).ceil() as u32))
}

/// The macOS Dock, or a taskbar. A stable cropped envelope: magnification changes content and
/// plates, never monitor-sized buffers.
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
    hot: Option<&Interaction>,
) -> Frame {
    let d = &config.dock;
    let theme = &tokens.theme;
    let c = theme.color;
    let horizontal = matches!(d.position, DockPosition::Bottom | DockPosition::Top);
    let taskbar = d.style == DockStyle::Taskbar;
    let titled = taskbar && d.titles && horizontal;
    let px = |v: f32| (v * scale).round();
    let size = px(d.size() as f32);
    let pad = px(d.padding.unwrap_or(5.0) as f32);
    let gap = px(d.spacing.unwrap_or(6.0) as f32);
    let length = if taskbar {
        px(if titled {
            d.button.unwrap_or(168.0)
        } else {
            d.button.unwrap_or(d.size() + 16.0)
        } as f32)
    } else {
        size
    };
    // Islands: one plate per group (pinned, running, extras), apart by a gap of wallpaper.
    let islands = d.islands && !taskbar && !tokens.bare;
    // macOS's dividers: between the kept apps and the others, and before the trash. A divider
    // takes one more gap and its hairline; between islands, both plates' padding and a gap.
    let divider = if islands {
        2.0 * pad + px(8.0)
    } else {
        gap + px(1.0).max(1.0)
    };
    let split = (!taskbar)
        .then(|| apps.iter().position(|a| !a.pinned))
        .flatten()
        .filter(|&i| i > 0);
    let extras_divider = !taskbar && !apps.is_empty() && !d.extras.is_empty();
    let dividers = split.map_or(0.0, |_| divider) + if extras_divider { divider } else { 0.0 };
    let groups = zones(
        &[&d.left, &d.right],
        model,
        config,
        BarKind::Dock,
        ui,
        images,
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
        apps.len() as f32 * (length + gap) + d.extras.len() as f32 * (size + gap) + 2.0 * pad - gap
            + dividers;
    let along = if horizontal { extent.0 } else { extent.1 } as f32;
    // Off unless asked for: no zoom, no room kept for one.
    let magnified = if d.magnify.enabled {
        d.magnify.scale as f32
    } else {
        1.0
    };
    let extra = if taskbar {
        0.0
    } else {
        size * (magnified - 1.0) * d.magnify.range as f32 * 2.0
    };
    let envelope = if taskbar {
        along
    } else {
        (base + extra + px(24.0)).min(along)
    }
    .max(1.0);
    let across = if taskbar {
        size + 2.0 * pad
    } else {
        (size * magnified + pad * 2.0 + px(24.0)).round()
    };
    let dimensions = if horizontal {
        (envelope.ceil() as u32, across.ceil() as u32)
    } else {
        (across.ceil() as u32, envelope.ceil() as u32)
    };
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
            // Windows 11: Start leads the centred apps, the group centred as one.
            ((envelope - base + sides[0]) / 2.0).max(sides[0] + pad)
        }
    } else {
        (envelope - base) / 2.0
    }
    .round();
    // Where app `i` starts along the dock, past the dividers before it.
    let along_at = |i: usize, shift: f32| {
        start
            + pad
            + i as f32 * (length + gap)
            + shift
            + split.filter(|&s| i >= s).map_or(0.0, |_| divider)
    };
    let count = apps.len();
    let first_extra = along_at(count, 0.0) - gap + if extras_divider { divider } else { 0.0 } + gap;
    // The magnifiable slots' resting centres: the apps, then the extras. `zoom` gives each its
    // scale and the shift of its centre, so a magnified icon pushes its neighbours (and the
    // dividers between them) along instead of growing over them.
    let rest: Vec<f32> = (0..count)
        .map(|i| along_at(i, 0.0) + length / 2.0)
        .chain((0..d.extras.len()).map(|e| first_extra + e as f32 * (size + gap) + size / 2.0))
        .collect();
    let zoomed = |k: usize| zoom.get(k).copied().unwrap_or((1.0, 0.0));
    // Where slot `k`'s icon spans along the dock, magnified and pushed.
    let span = |k: usize| {
        let (factor, shift) = zoomed(k);
        let centre = rest.get(k).copied().unwrap_or(start) + shift;
        let half = size * factor / 2.0;
        (centre - half, centre + half)
    };
    // Icons come in size buckets from resting to fully magnified, all built while the dock rests
    // (so a magnification sweep never decodes or rasterizes one); a frame draws each icon from the
    // smallest bucket at or above its size, so the renderer scales it by at most one step.
    const STEPS: usize = 3;
    let buckets: [u32; STEPS + 1] = std::array::from_fn(|k| {
        (size * (1.0 + (magnified - 1.0) * k as f32 / STEPS as f32)).round() as u32
    });
    let icons: Vec<Option<Arc<Image>>> = apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let factor = zoom.get(i).map_or(1.0, |z| z.0);
            if factor <= 1.001 {
                for &bucket in &buckets[1..] {
                    app_icon(ui, images, app, c.label, bucket);
                }
            }
            let want = (size * factor).round() as u32;
            let at = buckets
                .into_iter()
                .find(|&b| b >= want)
                .unwrap_or(buckets[STEPS]);
            app_icon(ui, images, app, c.label, at)
        })
        .collect();
    let extra_glyphs: Vec<Glyph> = d
        .extras
        .iter()
        .map(|extra| {
            let (icon, full) = match extra {
                Extra::Desktop => (Icon::Desktop, false),
                Extra::Trash => (Icon::Delete, model.trash_full),
            };
            themed(icon, c.label, size as u32, ui, images, full)
        })
        .collect();
    let input = hot.cloned().unwrap_or_default();
    let mut out = Out::default();
    let mut f = kit::Frame::new(&mut ui.text, theme, scale, &input);
    // The plate hugs the magnified run, as the Dock's does: from the first icon to the last.
    let (panel_start, panel_length) = if taskbar {
        (0.0, envelope)
    } else if rest.is_empty() {
        (start, base)
    } else {
        let from = span(0).0 - pad;
        (from, span(rest.len() - 1).1 + pad - from)
    };
    let plate = |a: f32, length: f32| convert(a, edge - pad, length, size + pad * 2.0);
    if islands {
        // Pinned apps, the other running apps, the extras: each its own plate.
        let s = split.unwrap_or(count);
        let mut runs = Vec::new();
        if s > 0 {
            runs.push((span(0).0, span(s - 1).1));
        }
        if s < count {
            runs.push((span(s).0, span(count - 1).1));
        }
        if !d.extras.is_empty() {
            runs.push((span(count).0, span(rest.len() - 1).1));
        }
        for (from, to) in runs {
            panel(
                &mut f,
                &mut out,
                plate(from - pad, to - from + 2.0 * pad),
                px(tokens.radius),
                tokens,
            );
        }
    } else if !tokens.bare {
        panel(
            &mut f,
            &mut out,
            plate(panel_start, panel_length),
            px(tokens.radius),
            tokens,
        );
    } else {
        f.fill(
            plate(panel_start, panel_length),
            px(tokens.radius),
            Rgba::HIT,
        );
    }
    if taskbar {
        for (i, zone) in groups.iter().enumerate().take(2) {
            let mut a = if i == 0 {
                if d.align == ItemsAlign::Start {
                    pad
                } else {
                    start + pad - sides[0]
                }
            } else {
                envelope - sides[1] - pad
            };
            for content in zone.iter().flatten() {
                let rect = convert(a, 0.0, content.width, across);
                item(&mut f, &mut out, content, rect, config, tokens, tokens.bare);
                a += content.width + gap;
            }
        }
    }
    let mut badges = Vec::new();
    for (i, app) in apps.iter().enumerate() {
        let (factor, shift) = zoom.get(i).copied().unwrap_or((1.0, 0.0));
        let a = along_at(i, shift);
        let rect = convert(a, edge, length, size);
        if rect.right > dimensions.0 as f32
            || rect.bottom > dimensions.1 as f32
            || rect.left < 0.0
            || rect.top < 0.0
        {
            continue;
        }
        let s = (size * factor).round();
        let motion = hops.get(i).copied().unwrap_or(0.0);
        let hop = motion.max(0.0);
        let b = if forward {
            edge + hop
        } else {
            edge + size - s - hop
        };
        let icon_rect = convert(
            (a + if titled { 0.0 } else { (length - s) / 2.0 }).round(),
            b.round(),
            s,
            s,
        );
        let id = out.add(Target::Dock(app.clone()));
        let active = app.windows.iter().any(|w| w.active);
        let state = f.state(id, true);
        let radius = f
            .control_radius(rect.height())
            .min(f.px(theme.radius.small + 2.0));
        let wash = rect.inflate(f.px(3.0));
        if taskbar {
            // Windows' task buttons: the active app keeps a resting wash.
            let rest = if active { c.hover } else { Rgba::TRANSPARENT };
            f.fill(wash, radius, rest);
            f.wash(id, wash, radius, true);
        }
        // A pressed icon sinks a little and darkens, as the Dock's do; the rest is the
        // magnification and the name's tooltip.
        let pressed = state == State::Pressed;
        let sink = icon_rect.width() * (1.0 - state.press_scale()) / 2.0;
        let shown = icon_rect.inflate(-sink.round());
        let fade = if motion < 0.0 { -motion } else { 1.0 };
        match &icons[i] {
            Some(image) => {
                f.push(Op::Image {
                    rect: fit(image, shown),
                    image: image.clone(),
                    opacity: fade * if pressed { 0.72 } else { 1.0 },
                    source: None,
                });
            }
            None => {
                // No icon: a tile with the app's initial, at the corner ratio of app icons.
                f.fill(
                    shown,
                    shown.width() * 0.225,
                    c.fill.with_alpha(c.fill.a * fade),
                );
                let letter = app
                    .name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                f.text_in(
                    shown,
                    &letter,
                    TextRole::Title2,
                    c.label.with_alpha(fade),
                    Align::Center,
                );
            }
        }
        if titled {
            let title = app
                .windows
                .first()
                .map_or(app.name.as_str(), |w| w.title.as_str());
            let text = Rect::new(a + size + px(8.0), edge, length - size - px(8.0), size);
            let font = tokens.font.scaled(scale);
            let (_, h) = f.text.measure("Ag", &font);
            let y = text.top + ((text.height() - h) / 2.0).round();
            f.label_styled(text.left, y, title, &font, c.label, text.width());
        }
        if !app.windows.is_empty() && d.indicator != IndicatorStyle::None {
            let (w, thickness) = match d.indicator {
                IndicatorStyle::Dot => (px(4.0), px(4.0)),
                IndicatorStyle::Bar => ((length * 0.65).round(), px(2.0)),
                // Windows 11's pill: 6 px running, 16 px for the app in front, 3 px thick.
                IndicatorStyle::Pill => (px(if active { 16.0 } else { 6.0 }), px(3.0)),
                _ => (px(if active { 16.0 } else { 8.0 }), px(4.0)),
            };
            let attention = app.windows.iter().any(|w| w.attention);
            let custom = if attention {
                d.look.indicator.attention.as_ref()
            } else if active {
                d.look.indicator.active.as_ref()
            } else {
                d.look.indicator.running.as_ref()
            };
            let color = custom.map_or(
                if attention {
                    c.warning
                } else if active {
                    c.label
                } else {
                    c.secondary
                },
                |color| tokens.color(color),
            );
            // Centred in the padding between the icon and the plate's edge.
            let middle = if forward {
                edge - pad / 2.0
            } else {
                edge + size + pad / 2.0
            };
            let along = (a + (length - w) / 2.0).round();
            let dot = convert(along, (middle - thickness / 2.0).round(), w, thickness);
            let radius = if d.indicator == IndicatorStyle::Bar {
                0.0
            } else {
                thickness / 2.0
            };
            f.fill(dot, radius, color);
        }
        if d.counter && app.windows.len() > 1 {
            let count = app.windows.len().min(99).to_string();
            let y = icon_rect.top + f.px(8.0);
            f.badge(icon_rect.right - f.px(14.0), y, &count, Some(c.accent));
        }
        f.focus_ring(id, wash, radius);
        f.hit(
            id,
            rect.union(icon_rect),
            Role::Button,
            &app.name,
            true,
            None,
        );
        badges.push((i, icon_rect));
    }
    // The divider between the kept apps and the others.
    let across_line = |f: &kit::Frame| {
        let inset = f.px(theme.metrics.item);
        (edge + inset, edge + size - inset)
    };
    // Dividers sit midway between their neighbours' magnified edges, so nothing covers one.
    let between = |k: usize| ((span(k - 1).1 + span(k).0) / 2.0).round();
    if let Some(s) = split.filter(|_| !islands) {
        let (from, to) = across_line(&f);
        divide(&mut f, horizontal, from, to, between(s));
    }
    for (i, extra) in d.extras.iter().enumerate() {
        let label = if *extra == Extra::Desktop {
            "Show desktop"
        } else if model.trash_full {
            "Recycle Bin (full)"
        } else {
            "Recycle Bin"
        };
        if taskbar && *extra == Extra::Desktop {
            // Windows' sliver at the far end, after a hairline.
            let sliver = f.px(6.0);
            let rect = convert(envelope - sliver, 0.0, sliver, across);
            let id = out.add(Target::Extra(*extra));
            f.wash(id, rect, 0.0, true);
            let inset = f.px(4.0);
            divide(&mut f, horizontal, inset, across - inset, envelope - sliver);
            f.hit(id, rect, Role::Button, label, true, None);
            continue;
        }
        let k = count + i;
        if i == 0 && extras_divider && !islands {
            let (from, to) = across_line(&f);
            divide(&mut f, horizontal, from, to, between(k));
        }
        // The extras magnify with the apps, growing away from the screen edge.
        let s = (size * zoomed(k).0).round();
        let b = if forward { edge } else { edge + size - s };
        let rect = convert(span(k).0.round(), b.round(), s, s);
        let id = out.add(Target::Extra(*extra));
        let state = f.state(id, true);
        let sink = (rect.width() * (1.0 - state.press_scale()) / 2.0).round();
        let shown = rect.inflate(-sink);
        match &extra_glyphs[i] {
            Glyph::Image(image) => {
                f.push(Op::Image {
                    rect: fit(image, shown),
                    image: image.clone(),
                    opacity: if state == State::Pressed { 0.72 } else { 1.0 },
                    source: None,
                });
            }
            Glyph::Icon(icon) => {
                // The trash as a glyph on the plate, half the icon size.
                let ink = if *extra == Extra::Trash && model.trash_full {
                    c.accent
                } else {
                    c.label
                };
                let ink = if state == State::Pressed {
                    c.secondary
                } else {
                    ink
                };
                f.icon(shown, *icon, d.size() as f32 * 0.5 * zoomed(k).0, ink);
            }
            _ => {}
        }
        let radius = f.px(theme.radius.small + 2.0);
        f.focus_ring(id, rect, radius);
        f.hit(id, rect, Role::Button, label, true, None);
    }
    let mut list = f.list;
    for (i, icon_rect) in badges {
        crate::integrations::dock_badge(&mut list, ui, &apps[i], icon_rect, scale);
    }
    let mut frame = out.finish(list, dimensions);
    frame.rest = rest;
    frame
}

/// `app`'s icon at `px` device pixels (its packaged logo, tinted `ink` when monochrome, else its
/// exe's icon); shared by the dock and its window previews.
pub fn app_icon(
    ui: &Ui,
    images: &mut Images,
    app: &Item,
    ink: Rgba,
    px: u32,
) -> Option<Arc<Image>> {
    images
        .app(app)
        .and_then(|(source, mono)| images.sized(ui, &source, mono.then_some(ink), px))
        .or_else(|| {
            app.icon
                .as_deref()
                .and_then(|s| images.sized(ui, s, None, px))
        })
}

/// Where `image` draws in `slot`: its longer side the slot's, centred on whole pixels. Images
/// come at the slot's size ([`Images::sized`]), so this is 1:1; an app that sent a smaller
/// tray icon is scaled up to match its neighbours, as Windows does.
fn fit(image: &Image, slot: Rect) -> Rect {
    let k = slot.width().min(slot.height()) / image.width.max(image.height).max(1) as f32;
    slot.place(
        (image.width as f32 * k).round(),
        (image.height as f32 * k).round(),
        Align::Center,
        Align::Center,
    )
}

/// A hairline across the dock at `at` along it, from `from` to `to` across.
fn divide(f: &mut kit::Frame, horizontal: bool, from: f32, to: f32, at: f32) {
    if horizontal {
        f.separator_v(from, to, at);
    } else {
        f.separator(from, to, at);
    }
}
