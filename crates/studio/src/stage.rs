//! The desktop previews (D3): the wallpaper, then the shell's own views — the top bar, the
//! dock, the widgets, the desktop menu — drawn headless at the preview's scale, exactly as the
//! shell builds them for the screen. Glass comes from the engine ([`crate::glass`]); while it
//! can't answer, a flat tint stands in. Building is cached: hovering in Studio reuses it.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mypaper_bars::BarsConfig;
use mypaper_shell::preview as shell;
use mypaper_ui::controls::{Align, Font, key};
use mypaper_ui::{DisplayList, Image, Op, Point, Rect, Rgba, Role, Ui};
use mypaper_widgets::layout::{self, Grid, Layout, RectF};
use serde_json::{Value, json};

use crate::actions::{Action, Bar, Page, Slot};
use crate::app::{App, theme_name};
use crate::docs::DocId;
use crate::glass::{Request, Shape};
use crate::ui::B;

/// What a stage shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Show {
    /// The desktop as it is: wallpaper, bars, widgets.
    Desktop,
    /// Widgets lead; the bars are dimmed.
    Widgets,
    /// The bars alone, at their real size.
    Bars,
    /// Look's specimen: a top bar, a calendar panel, the desktop menu, two widgets, the dock.
    Specimen,
}

/// Something on the stage the pointer can pick, in stage-local pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub what: What,
    pub rect: Rect,
    pub radius: f32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    Widget(String),
    Bar(Bar),
    Module(Slot),
}

/// A built stage, in stage-local pixels.
#[derive(Default, Clone)]
pub struct Content {
    pub size: (u32, u32),
    /// Shell content, drawn above the wallpaper.
    pub ops: Vec<Op>,
    /// Content drawn dimmed (the bars on the Widgets page).
    pub dim: Vec<Op>,
    pub shapes: Vec<Shape>,
    pub items: Vec<Item>,
    pub still: Option<PathBuf>,
    /// The grid of the display, for widget drags: origin and cell in stage pixels.
    pub grid: Option<(Grid, f32)>,
}

/// One design's thumbnail for the Bars inspector: its bar, drawn by the shell's view on a
/// 400-pixel-wide screen scaled to the tile (room for its modules, large enough to read its
/// shape), in tile-local pixels.
pub struct Thumb {
    pub design: &'static str,
    pub ops: Vec<Op>,
    pub shapes: Vec<Shape>,
}

/// The design thumbnails. A tile asks for its size while the frame is built (`want`); the next
/// frame builds them ([`prepare_designs`]), so they are always the tile's exact size.
#[derive(Default)]
pub struct Designs {
    pub want: Option<(Bar, (u32, u32))>,
    /// What `thumbs` were built for: the bar, the size, the documents' version.
    built: Option<(Bar, (u32, u32), u64)>,
    pub thumbs: Vec<Thumb>,
}

impl Designs {
    /// A tile asked for thumbnails that aren't built yet.
    pub fn pending(&self, version: u64) -> bool {
        self.want.is_some_and(|(bar, size)| self.built != Some((bar, size, version)))
    }

    /// `design`'s thumbnail, if built for this bar and size.
    pub fn get(&self, bar: Bar, size: (u32, u32), design: &str) -> Option<&Thumb> {
        let built = self.built.is_some_and(|(b, s, _)| b == bar && s == size);
        built.then(|| self.thumbs.iter().find(|t| t.design == design)).flatten()
    }
}

#[derive(Default)]
pub struct Cache {
    key: u64,
    pub content: Content,
    pub designs: Designs,
    images: Option<shell::bars::Images>,
    /// The shell's app-icon service (names and icons of pinned apps), started on first use.
    icons: Option<(mypaper_bars::icons::Icons, std::sync::mpsc::Receiver<mypaper_bars::wake::Msg>)>,
}

impl Cache {
    /// Takes what the icon service found; returns whether the stage must be rebuilt.
    pub fn icons_arrived(&mut self) -> bool {
        let Some((icons, rx)) = &mut self.icons else { return false };
        let mut any = false;
        while let Ok(msg) = rx.try_recv() {
            if let mypaper_bars::wake::Msg::Described(source, described) = msg {
                icons.on_described(source, described);
                any = true;
            }
        }
        if any {
            self.key = 0;
        }
        any
    }
}

/// Where the stage sits in `area`: the display's shape, as large as fits.
pub fn frame(app: &App, area: Rect) -> Rect {
    let (mw, mh) = app
        .display()
        .map_or((2560.0, 1440.0), |m| (m.rect.width as f32, m.rect.height as f32));
    let scale = (area.width() / mw).min(area.height() / mh);
    let (w, h) = ((mw * scale).floor(), (mh * scale).floor());
    Rect::new(
        (area.left + (area.width() - w) / 2.0).round(),
        (area.top + (area.height() - h) / 2.0).round(),
        w,
        h,
    )
}

/// Builds the stage's content when what it shows changed (needs the whole `Ui`: bar views
/// load their icons through it).
pub fn prepare(app: &App, ui: &mut Ui, size: (u32, u32), show: Show) {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.docs.version, size, show, app.nav.display, theme_name(&app.docs)).hash(&mut h);
    (app.nav.widget.as_deref(), app.fixture).hash(&mut h);
    if !app.fixture {
        // A clock on the stage follows the minute.
        let now = shell::widgets::Stamp::now();
        (now.hour, now.minute).hash(&mut h);
    }
    let key = h.finish();
    let mut cache = app.stage.borrow_mut();
    if cache.key == key && cache.content.size == size {
        return;
    }
    cache.key = key;
    if cache.icons.is_none() && !app.fixture && app.thread != 0 {
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = mypaper_bars::files::local("icons").unwrap_or_default();
        let waker = mypaper_bars::wake::Waker::new(tx, app.thread);
        cache.icons = Some((mypaper_bars::icons::Icons::new(waker, dir), rx));
    }
    let Cache { images, icons, .. } = &mut *cache;
    let images = images.get_or_insert_with(shell::bars::Images::default);
    let icons = icons.as_mut().map(|(i, _)| i);
    cache.content = build(app, ui, images, icons, size, show);
}

fn build(
    app: &App,
    ui: &mut Ui,
    images: &mut shell::bars::Images,
    icons: Option<&mut mypaper_bars::icons::Icons>,
    size: (u32, u32),
    show: Show,
) -> Content {
    let Some(monitor) = app.display().cloned() else {
        return Content::default();
    };
    let k = size.0 as f32 / monitor.rect.width.max(1) as f32;
    let dpi = monitor.dpi as f32 / 96.0;
    // How large the shell's views draw: the display's real proportions; Look's specimen as a
    // 1280-wide desktop (legible at any size); the Bars page's bars at their real size.
    let scale = match show {
        Show::Specimen => size.0 as f32 / 1280.0,
        Show::Bars => app.scale,
        Show::Desktop | Show::Widgets => dpi * k,
    };
    let mut out = Content {
        size,
        still: app.library.active().and_then(|i| {
            let w = &app.library.items[i];
            w.preview.clone().or_else(|| w.video.clone())
        }),
        ..Content::default()
    };
    let theme_dir = theme_name(&app.docs).and_then(|t| mypaper_theme::locate(Some(&t)).ok());
    let loaded = theme_dir.as_deref().and_then(|d| mypaper_theme::LoadedTheme::load(d).ok());
    images.configure(
        app.get(DocId::Bars, "/icons").and_then(Value::as_str),
        theme_dir.clone().unwrap_or_default(),
    );
    // Bars.
    if let Ok(config) = bars_config(app, &monitor) {
        let (appearance, system) = bar_base(app, &config, loaded.as_ref());
        let model = shell::bars::sample_model(clock(app), "File Explorer");
        let mut bar_ops = Vec::new();
        if config.top.enabled && config.top.monitors.includes(&monitor.device, monitor.primary) {
            let tokens = shell::bars::Tokens::resolve(&config.top.look, &appearance, &system);
            let frame = shell::bars::top(size.0, &config, &model, &tokens, scale, ui, images, None);
            let y = if config.top.position == mypaper_bars::config::TopPosition::Bottom {
                size.1 as f32 - frame.size.1 as f32
            } else {
                0.0
            };
            place_bar(&mut out, &mut bar_ops, &frame, (0.0, y), &tokens, Bar::Top, &config);
        }
        if config.dock.enabled && config.dock.monitors.includes(&monitor.device, monitor.primary) {
            let tokens = shell::bars::Tokens::resolve(&config.dock.look, &appearance, &system);
            let apps = dock_apps(&config, icons);
            let frame = shell::bars::dock(&config, &model, &apps, &tokens, scale, size, &[], &[], ui, images, None);
            let (w, h) = (frame.size.0 as f32, frame.size.1 as f32);
            let margin = config.dock.margin.unwrap_or(6.0) as f32 * scale;
            let (x, y) = match config.dock.position {
                mypaper_bars::config::DockPosition::Bottom => ((size.0 as f32 - w) / 2.0, size.1 as f32 - h - margin),
                mypaper_bars::config::DockPosition::Top => ((size.0 as f32 - w) / 2.0, margin),
                mypaper_bars::config::DockPosition::Left => (margin, (size.1 as f32 - h) / 2.0),
                mypaper_bars::config::DockPosition::Right => (size.0 as f32 - w - margin, (size.1 as f32 - h) / 2.0),
            };
            place_bar(&mut out, &mut bar_ops, &frame, (x.round(), y.round()), &tokens, Bar::Dock, &config);
        }
        if show == Show::Widgets {
            out.dim = bar_ops;
            out.items.retain(|i| !matches!(i.what, What::Bar(_) | What::Module(_)));
        } else {
            out.ops = bar_ops;
        }
    }
    match show {
        Show::Bars => {}
        Show::Specimen => specimen(app, ui, &mut out, scale, loaded.as_ref()),
        Show::Desktop | Show::Widgets => widgets(app, ui, &mut out, &monitor, scale, loaded.as_ref()),
    }
    out
}

/// What every bar view needs besides its config: the base appearance (the file's over the
/// wallpaper's) and the system accent.
fn bar_base(
    app: &App,
    config: &BarsConfig,
    loaded: Option<&mypaper_theme::LoadedTheme>,
) -> (mypaper_theme::Appearance, mypaper_bars::accent::System) {
    let theme = loaded
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|s| s.appearance.clone())
        .unwrap_or_default();
    let system = if app.fixture { fixed_accent() } else { mypaper_bars::accent::read() };
    (config.appearance.over(&theme), system)
}

/// Builds the design thumbnails a tile asked for, if they aren't already (see [`Designs`]).
pub fn prepare_designs(app: &App, ui: &mut Ui) {
    let mut cache = app.stage.borrow_mut();
    let Some((bar, size)) = cache.designs.want.take() else { return };
    let version = app.docs.version;
    if cache.designs.built == Some((bar, size, version)) {
        return;
    }
    let Some(monitor) = app.display().cloned() else { return };
    let Ok(base) = bars_config(app, &monitor) else { return };
    let theme_dir = theme_name(&app.docs).and_then(|t| mypaper_theme::locate(Some(&t)).ok());
    let loaded = theme_dir.as_deref().and_then(|d| mypaper_theme::LoadedTheme::load(d).ok());
    let Cache { images, icons, designs, .. } = &mut *cache;
    let images = images.get_or_insert_with(shell::bars::Images::default);
    let mut icons = icons.as_mut().map(|(i, _)| i);
    let scale = size.0 as f32 / 400.0;
    let model = shell::bars::sample_model(clock(app), "File Explorer");
    let list: &[(&'static str, &str)] = match bar {
        Bar::Top => &crate::pages::bars::TOP_DESIGNS,
        Bar::Dock => &crate::pages::bars::DOCK_DESIGNS,
    };
    designs.thumbs.clear();
    for &(design, _) in list {
        // The file as it is, with this design: the bars resolve it exactly as on the desktop.
        let mut raw = app.docs.doc(DocId::Bars).raw.clone();
        let op = crate::docs::Op { doc: DocId::Bars, path: format!("/{}/design", bar.key()), value: Some(json!(design)) };
        crate::docs::apply_op(&mut raw, &op);
        let Ok(mut config) = BarsConfig::parse(&raw.to_string()).and_then(|c| c.for_monitor(&monitor.device, monitor.primary)) else {
            continue;
        };
        config.dock.pinned = base.dock.pinned.clone();
        config.resolve(shell::bars::default_pins);
        let (appearance, system) = bar_base(app, &config, loaded.as_ref());
        let (frame, at, tokens) = match bar {
            Bar::Top => {
                let tokens = shell::bars::Tokens::resolve(&config.top.look, &appearance, &system);
                let frame = shell::bars::top(size.0, &config, &model, &tokens, scale, ui, images, None);
                let y = if config.top.position == mypaper_bars::config::TopPosition::Bottom { size.1 as f32 - frame.size.1 as f32 } else { 0.0 };
                (frame, (0.0, y), tokens)
            }
            Bar::Dock => {
                let tokens = shell::bars::Tokens::resolve(&config.dock.look, &appearance, &system);
                let apps = dock_apps(&config, icons.as_deref_mut());
                let frame = shell::bars::dock(&config, &model, &apps, &tokens, scale, size, &[], &[], ui, images, None);
                let (w, h) = (frame.size.0 as f32, frame.size.1 as f32);
                let margin = config.dock.margin.unwrap_or(6.0) as f32 * scale;
                let at = match config.dock.position {
                    mypaper_bars::config::DockPosition::Bottom => ((size.0 as f32 - w) / 2.0, size.1 as f32 - h - margin),
                    mypaper_bars::config::DockPosition::Top => ((size.0 as f32 - w) / 2.0, margin),
                    mypaper_bars::config::DockPosition::Left => (margin, (size.1 as f32 - h) / 2.0),
                    mypaper_bars::config::DockPosition::Right => (size.0 as f32 - w - margin, (size.1 as f32 - h) / 2.0),
                };
                (frame, at, tokens)
            }
        };
        designs.thumbs.push(Thumb {
            design,
            ops: offset(&frame.list.ops, at.0, at.1).collect(),
            shapes: frame.shapes.iter().map(|(r, radius)| shape(r.offset(at.0, at.1), *radius, &tokens)).collect(),
        });
    }
    designs.built = Some((bar, size, version));
}

/// The bars' config as this display sees it (overrides applied, pins resolved).
fn bars_config(app: &App, monitor: &mypaper_desktop::Monitor) -> anyhow::Result<BarsConfig> {
    let text = serde_json::to_string(&app.docs.doc(DocId::Bars).raw)?;
    let mut config = BarsConfig::parse(&text)?.for_monitor(&monitor.device, monitor.primary)?;
    config.resolve(shell::bars::default_pins);
    Ok(config)
}

/// The pinned apps as dock items (no windows: a calm preview).
fn dock_apps(config: &BarsConfig, mut icons: Option<&mut mypaper_bars::icons::Icons>) -> Vec<mypaper_bars::apps::Item> {
    config
        .dock
        .pinned
        .clone()
        .unwrap_or_default()
        .into_iter()
        .take(12)
        .map(|pin| {
            let described = icons
                .as_deref_mut()
                .and_then(|i| i.get(&mypaper_bars::icons::Source::Path(pin.path.clone())).cloned());
            let name = pin
                .name
                .clone()
                .or_else(|| described.as_ref().and_then(|d| d.name.clone()))
                .unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path));
            mypaper_bars::apps::Item {
                key: mypaper_bars::apps::pin_key(&pin.path),
                name,
                path: pin.path,
                icon: described.and_then(|d| d.icon),
                pinned: true,
                windows: vec![],
                aumid: None,
                exe: None,
            }
        })
        .collect()
}

/// Adds a bar's content and glass shapes at `(x, y)`, and its stage items.
fn place_bar(
    out: &mut Content,
    ops: &mut Vec<Op>,
    frame: &shell::bars::Frame,
    (x, y): (f32, f32),
    tokens: &shell::bars::Tokens,
    bar: Bar,
    config: &BarsConfig,
) {
    ops.extend(offset(&frame.list.ops, x, y));
    for (rect, radius) in &frame.shapes {
        let rect = rect.offset(x, y);
        out.shapes.push(shape(rect, *radius, tokens));
    }
    let bounds = frame
        .shapes
        .iter()
        .map(|(r, _)| *r)
        .reduce(Rect::union)
        .unwrap_or(Rect::new(0.0, 0.0, frame.size.0 as f32, frame.size.1 as f32))
        .offset(x, y);
    out.items.push(Item {
        what: What::Bar(bar),
        rect: bounds,
        radius: frame.shapes.first().map_or(0.0, |s| s.1),
        label: if bar == Bar::Top { "Top bar".into() } else { "Dock".into() },
    });
    // Modules: the hit items whose targets are modules, in list order.
    if bar == Bar::Top {
        for hit in &frame.list.hits {
            if let Some(shell::bars::Target::Module(module)) = frame.targets.get(hit.id as usize - 1)
                && let Some(slot) = slot_of(config, module)
            {
                out.items.push(Item {
                    what: What::Module(slot),
                    rect: hit.rect.offset(x, y),
                    radius: 5.0,
                    label: crate::pages::bars::module_name(module),
                });
            }
        }
    }
}

/// Where a module sits in the top bar's lists.
fn slot_of(config: &BarsConfig, module: &mypaper_bars::ModuleRef) -> Option<Slot> {
    for (list, items) in [("left", &config.top.left), ("center", &config.top.center), ("right", &config.top.right)] {
        if let Some(index) = items.iter().position(|m| m == module) {
            return Some(Slot { bar: Bar::Top, list, index });
        }
    }
    None
}

/// A surface's glass: its material, or a tint for solid styles.
fn shape(rect: Rect, radius: f32, tokens: &shell::bars::Tokens) -> Shape {
    match tokens.material {
        Some(m) => Shape {
            rect,
            radius,
            clarity: m.clarity,
            blur: m.blur,
            refraction: m.refraction,
            highlight: m.highlight,
            tint: m.tint,
        },
        None => Shape {
            rect,
            radius,
            clarity: 1.0,
            blur: 0.0,
            refraction: 0.0,
            highlight: 0.0,
            tint: Rgba::TRANSPARENT,
        },
    }
}

/// The widgets on this display.
fn widgets(
    app: &App,
    ui: &mut Ui,
    out: &mut Content,
    monitor: &mypaper_desktop::Monitor,
    scale: f32,
    theme: Option<&mypaper_theme::LoadedTheme>,
) {
    let text = serde_json::to_string(&app.docs.doc(DocId::Widgets).raw).unwrap_or_default();
    let Ok(layout) = Layout::parse(&text) else { return };
    let devices: Vec<String> = app.monitors.iter().map(|m| m.device.clone()).collect();
    let primary = app.monitors.iter().position(|m| m.primary).unwrap_or(0);
    let grid = grid_for(&layout, monitor);
    let skin = mypaper_widgets::look::load_skin(theme.map(|t| t.dir.as_path()));
    let base = mypaper_widgets::look::base(&skin);
    let mut storage = mypaper_widgets::storage::Storage::new(if app.fixture {
        std::env::temp_dir().join("mypaper-studio-fixture")
    } else {
        mypaper_widgets::files::user("widget-data").unwrap_or_default()
    });
    
    out.grid = Some((grid, scale));
    for instance in &layout.instances {
        if layout::show_on(&instance.monitor, &devices, primary) != Some(monitor.device.as_str()) {
            continue;
        }
        let Some(manifest) = app.registry.get(&instance.widget).map(|i| &i.manifest) else {
            // Not installed: a placeholder at its cells, still selectable (and removable).
            let r = grid.rect(instance.cell, instance.size);
            let rect = Rect::new(r.x as f32 * scale, r.y as f32 * scale, r.width as f32 * scale, r.height as f32 * scale);
            let radius = 22.0 * scale;
            out.ops.push(Op::Fill { rect, radius, color: Rgba::new(0.0, 0.0, 0.0, 0.35) });
            out.ops.push(Op::Stroke { rect, radius, width: 1.0, color: Rgba::new(1.0, 1.0, 1.0, 0.35) });
            out.items.push(Item {
                what: What::Widget(instance.instance.clone()),
                rect,
                radius,
                label: format!("{} (not installed)", instance.widget),
            });
            continue;
        };
        let appearance = mypaper_widgets::look::resolve(&instance.appearance, &layout.appearance, &base, mypaper_widgets::look::accent(None));
        let placed = layout::place(instance, manifest, &grid, &appearance);
        let r = grid.rect(placed.cell, placed.size);
        let rect = Rect::new(r.x as f32 * scale, r.y as f32 * scale, r.width as f32 * scale, r.height as f32 * scale);
        let label = manifest.name.clone();
        let instance_id = placed.instance.clone();
        widget(ui, out, placed, &mut storage, rect, scale, &skin, None, app.fixture);
        out.items.push(Item {
            what: What::Widget(instance_id),
            rect,
            radius: 22.0 * scale,
            label,
        });
    }
}

/// Draws one widget's real view at `rect` (and its glass shape), with `data` when given.
#[allow(clippy::too_many_arguments)]
fn widget(
    ui: &mut Ui,
    out: &mut Content,
    placed: mypaper_widgets::layout::Placed,
    storage: &mut mypaper_widgets::storage::Storage,
    rect: Rect,
    scale: f32,
    skin: &mypaper_widgets::look::Skin,
    data: Option<Value>,
    fixture: bool,
) {
    let now = if fixture { fixed_stamp() } else { shell::widgets::Stamp::now() };
    let appearance = placed.appearance.clone();
    let Ok(mut model) = shell::widgets::Model::new(placed, storage, now) else {
        return;
    };
    model.visible = true;
    if let Some(data) = data {
        model.data = data;
    }
    let tokens = shell::widgets::tokens(&appearance);
    let size = (rect.width().round().max(1.0) as u32, rect.height().round().max(1.0) as u32);
    let list = shell::widgets::view(
        &model,
        &tokens,
        shell::widgets::Frame {
            size,
            scale,
            now,
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    );
    out.ops.extend(offset(&list.ops, rect.left.round(), rect.top.round()));
    let surface = appearance.get("surface").and_then(Value::as_str).unwrap_or("glass");
    if !matches!(surface, "none" | "solid" | "tint") {
        let liquid = mypaper_widgets::look::liquid(skin);
        let tint = appearance
            .get("tint")
            .and_then(Value::as_str)
            .and_then(mypaper_config::Color::parse)
            .unwrap_or(mypaper_widgets::look::DEFAULT_TINT);
        let radius = appearance.get("radius").and_then(Value::as_f64).unwrap_or(22.0) as f32 * scale;
        out.shapes.push(Shape {
            rect,
            radius,
            clarity: 1.0 - liquid.tint.unwrap_or(0.06),
            blur: liquid.blur.unwrap_or(12.0) * scale,
            refraction: liquid.refraction.unwrap_or(0.6),
            highlight: liquid.highlight.unwrap_or(0.55),
            tint: Rgba::rgb8(tint.0, liquid.tint.unwrap_or(0.06)),
        });
    }
}

/// Look's specimen: two widgets, a calendar panel and the desktop menu, over the bars.
#[allow(clippy::too_many_arguments)]
fn specimen(
    app: &App,
    ui: &mut Ui,
    out: &mut Content,
    scale: f32,
    theme: Option<&mypaper_theme::LoadedTheme>,
) {
    // A 1280-wide virtual display, so the specimen reads at any size.
    let monitor = &mypaper_desktop::Monitor {
        device: "specimen".into(),
        rect: mypaper_desktop::Rect { x: 0, y: 0, width: 1280, height: (1280.0 * out.size.1 as f32 / out.size.0.max(1) as f32) as i32 },
        work: mypaper_desktop::Rect { x: 0, y: 0, width: 1280, height: (1280.0 * out.size.1 as f32 / out.size.0.max(1) as f32) as i32 },
        dpi: 96,
        primary: true,
    };
    let skin = mypaper_widgets::look::load_skin(theme.map(|t| t.dir.as_path()));
    let base = mypaper_widgets::look::base(&skin);
    let text = serde_json::to_string(&app.docs.doc(DocId::Widgets).raw).unwrap_or_default();
    let user = Layout::parse(&text).map(|l| l.appearance).unwrap_or_default();
    let appearance = mypaper_widgets::look::resolve(&Default::default(), &user, &base, mypaper_widgets::look::accent(None));
    let mut storage = mypaper_widgets::storage::Storage::new(std::env::temp_dir().join("mypaper-studio-specimen"));
    let grid = grid_for(&Layout::default(), monitor);
    let cell = |c: u32, r: u32, w: u32, h: u32| -> (Rect, [u32; 2], [u32; 2]) {
        let rr = grid.rect([c, r], [w, h]);
        (
            Rect::new(rr.x as f32 * scale, rr.y as f32 * scale, rr.width as f32 * scale, rr.height as f32 * scale),
            [c, r],
            [w, h],
        )
    };
    let placed = |id: &str, at: [u32; 2], size: [u32; 2]| -> Option<mypaper_widgets::layout::Placed> {
        let manifest = &app.registry.get(id)?.manifest;
        let mut l = Layout::default();
        let instance = l.add(id, &monitor.device, at, size);
        Some(layout::place(l.get(&instance)?, manifest, &grid, &appearance))
    };
    let weather = json!({"current":{"location":{"name":"Hong Kong"},"temperature":22,"condition":"Light rain","code":61},
        "forecast":{"days":[{"date":"2026-11-05","max":24,"min":19},{"date":"2026-11-06","max":23,"min":19},{"date":"2026-11-07","max":24,"min":20}]}});
    let media = json!({"title":"Slow Light","artist":"Lumen Fields","playing":true,"position":84,"duration":232});
    for (id, (rect, at, sz), data) in [
        ("weather", cell(0, 0, 3, 2), Some(weather)),
        ("now-playing", cell(0, 2, 3, 1), Some(media)),
    ] {
        if let Some(p) = placed(id, at, sz) {
            widget(ui, out, p, &mut storage, rect, scale, &skin, data, app.fixture);
        }
    }
    // The calendar panel (the calendar widget's month, as the flyout shares its grid).
    let cols = grid.cols.max(4);
    let (rect, at, sz) = cell(cols.saturating_sub(4), 0, 4, 4);
    if let Some(p) = placed("calendar", at, sz) {
        widget(ui, out, p, &mut storage, rect, scale, &skin, None, app.fixture);
    }
    // The desktop menu, open in the middle.
    let mut menu_tokens = shell::menu::tokens(&theme_name(&app.docs).unwrap_or_default(), &ui.text);
    menu_tokens.shadow = false;
    let items = shell::menu::build(&mypaper_config::DesktopMenu::default(), shell::menu::ViewState { icons_visible: true, ..Default::default() });
    let layout = mypaper_ui::menu::layout(&items, &menu_tokens, scale, &mut ui.text);
    let list: DisplayList = mypaper_ui::menu::view(&items, &layout, Some(7), &menu_tokens);
    let (mx, my) = (out.size.0 as f32 * 0.42, out.size.1 as f32 * 0.32);
    out.ops.extend(offset(&list.ops, mx.round(), my.round()));
}

/// A widget grid for a display, as the shell computes it.
pub fn grid_for(layout: &Layout, monitor: &mypaper_desktop::Monitor) -> Grid {
    let scale = monitor.dpi as f64 / 96.0;
    Grid::new(
        &layout.grid,
        icon_cell(),
        RectF {
            x: f64::from(monitor.work.x - monitor.rect.x) / scale,
            y: f64::from(monitor.work.y - monitor.rect.y) / scale,
            width: f64::from(monitor.work.width) / scale,
            height: f64::from(monitor.work.height) / scale,
        },
    )
}

/// The desktop icon cell in DIPs (Windows' icon spacing), as the shell reads it.
fn icon_cell() -> [f64; 2] {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let read = |name: windows::core::PCWSTR| {
        let mut buffer = [0u16; 32];
        let mut bytes = size_of_val(&buffer) as u32;
        // SAFETY: a bounded buffer and its size.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Control Panel\Desktop\WindowMetrics"),
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
            .ok()
            .ok()?;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len]).trim().parse::<i32>().ok()
    };
    [
        layout::icon_spacing_dip(read(w!("IconSpacing"))),
        layout::icon_spacing_dip(read(w!("IconVerticalSpacing"))),
    ]
}

/// Goldens: a fixed accent (no machine settings).
fn fixed_accent() -> mypaper_bars::accent::System {
    mypaper_bars::accent::System {
        accent: std::array::from_fn(|_| "#5b8def".to_owned()),
        scheme: mypaper_bars::accent::Scheme::Dark,
        animations: true,
    }
}

/// Goldens: Thursday 5 November 2026, 10:42.
fn fixed_stamp() -> shell::widgets::Stamp {
    shell::widgets::Stamp {
        date: mypaper_ui::calendar::Date { year: 2026, month: 11, day: 5 },
        hour: 10,
        minute: 42,
        second: 0,
        ms: 1_793_875_320_000,
    }
}

fn clock(app: &App) -> windows::Win32::Foundation::SYSTEMTIME {
    if app.fixture {
        return windows::Win32::Foundation::SYSTEMTIME {
            wYear: 2026,
            wMonth: 11,
            wDay: 5,
            wDayOfWeek: 4,
            wHour: 10,
            wMinute: 42,
            ..Default::default()
        };
    }
    // SAFETY: a clock read.
    unsafe { windows::Win32::System::SystemInformation::GetLocalTime() }
}

/// `ops` moved by `(dx, dy)`.
pub fn offset(ops: &[Op], dx: f32, dy: f32) -> impl Iterator<Item = Op> + '_ {
    let r = move |r: Rect| r.offset(dx, dy);
    let p = move |p: Point| Point::new(p.x + dx, p.y + dy);
    ops.iter().filter_map(move |op| {
        Some(match op.clone() {
            Op::Arc { center, radius, sweep, width, color } => Op::Arc { center: p(center), radius, sweep, width, color },
            Op::Image { rect, image, opacity, source } => Op::Image { rect: r(rect), image, opacity, source },
            Op::RoundedImage { rect, image, opacity, source, radius } => Op::RoundedImage { rect: r(rect), image, opacity, source, radius },
            Op::TextField { .. } => return None,
            Op::Fill { rect, radius, color } => Op::Fill { rect: r(rect), radius, color },
            Op::Continuous { rect, radius, smoothing, width, color } => Op::Continuous { rect: r(rect), radius, smoothing, width, color },
            Op::Icon { rect, icon, weight, color } => Op::Icon { rect: r(rect), icon, weight, color },
            Op::Stroke { rect, radius, width, color } => Op::Stroke { rect: r(rect), radius, width, color },
            Op::Line { from, to, width, color } => Op::Line { from: p(from), to: p(to), width, color },
            Op::Text { rect, text, style, color } => Op::Text { rect: r(rect), text, style, color },
            Op::Emphasis {rect,text,style,color,italic,strike} => Op::Emphasis {rect:r(rect),text,style,color,italic,strike},
            Op::Clipped {rect,ops}=>Op::Clipped {rect:r(rect),ops:offset(&ops,dx,dy).collect::<Vec<_>>().into()},
            Op::Shadow { rect, radius, spread, drop, color } => Op::Shadow { rect: r(rect), radius, spread, drop, color },
            Op::Clip { rect } => Op::Clip { rect: r(rect) },
            Op::Unclip => Op::Unclip,
        })
    })
}

/// The flat stand-in for engine glass: the tint, a little stronger than clarity says (there's
/// no blur under it), and the rim's hairline. `shapes` move by `(dx, dy)`.
pub fn stand_in(b: &mut B, shapes: &[Shape], dx: f32, dy: f32) {
    for s in shapes {
        let alpha = 0.28 + (1.0 - s.clarity).clamp(0.0, 1.0) * 0.55;
        let rect = s.rect.offset(dx, dy);
        b.fill(rect, s.radius, s.tint.with_alpha(alpha.min(0.9)));
        b.stroke(rect, s.radius, 1.0, Rgba::new(1.0, 1.0, 1.0, 0.14));
    }
}

/// Draws the stage at `frame` and registers its items; returns them in window pixels. `pick`
/// says what clicking an item does (and `None` for the bare wallpaper).
pub fn draw(app: &App, b: &mut B, frame: Rect, show: Show, pick: impl Fn(Option<&Item>) -> Action) -> Vec<Item> {
    let p = *b.p;
    let cache = app.stage.borrow();
    let c = &cache.content;
    let size = (frame.width() as u32, frame.height() as u32);
    // The wallpaper: the engine's answer (wallpaper and glass), else the still with a tint.
    let request = Request {
        size,
        shapes: c.shapes.clone(),
    };
    let glass = if app.fixture { None } else { app.glass.borrow_mut().image(&request) };
    let engine = glass.is_some();
    let r = b.px(12.0);
    match glass {
        Some(image) => image_round(b, frame, &image, 1.0, r),
        None => match c.still.as_ref().and_then(|s| app.images.borrow_mut().get(s, size)) {
            Some(still) => image_round(b, frame, &still, 1.0, r),
            None => b.fill(frame, r, Rgba::rgb8([0x1d, 0x22, 0x2c], 1.0)),
        },
    }
    if show == Show::Widgets {
        // The bars dim so the widgets lead.
        b.list.ops.extend(offset(&c.dim, frame.left, frame.top).map(|op| dim(op, 0.42)));
    }
    if !engine {
        stand_in(b, &c.shapes, frame.left, frame.top);
    }
    b.list.ops.extend(offset(&c.ops, frame.left, frame.top));
    // The picture is rounded; bars and widgets drawn over it can reach its corners, so a ring
    // in the page colour trims them, starting a pixel inside the edge (no antialiased seam).
    let t = b.px(6.0);
    b.stroke(frame.inflate(t - 0.5), r + t - 0.5, t * 2.0 + 1.0, p.bg);
    // Items: the bare wallpaper first (so items win), then each.
    let items: Vec<Item> = c
        .items
        .iter()
        .map(|i| Item {
            rect: i.rect.offset(frame.left, frame.top),
            ..i.clone()
        })
        .collect();
    drop(cache);
    b.hit(key("stage-wallpaper"), frame, Role::Image, "Wallpaper", pick(None), mypaper_ui::controls::Behavior::Click);
    for item in &items {
        let k = key(("stage", format!("{:?}", item.what)));
        let behavior = if matches!(item.what, What::Widget(_) | What::Module(_)) && show != Show::Desktop {
            mypaper_ui::controls::Behavior::Drag
        } else {
            mypaper_ui::controls::Behavior::Click
        };
        b.hit(k, item.rect, Role::Button, &item.label, pick(Some(item)), behavior);
        if b.hovered(k) && show != Show::Specimen {
            outline(b, item.rect, item.radius, false);
        }
    }
    items
}

/// The selection outline (2 px white over a 1 px dark line: readable on any wallpaper) and a
/// name tag above it.
pub fn outline(b: &mut B, rect: Rect, radius: f32, tag: bool) {
    b.stroke(rect.inflate(3.0), radius + 3.0, 1.0, Rgba::new(0.0, 0.0, 0.0, 0.6));
    b.stroke(rect.inflate(2.0), radius + 2.0, 2.0, Rgba::new(1.0, 1.0, 1.0, 1.0));
    let _ = tag;
}

/// A name tag (micro on solid) at the top-left of `rect`.
pub fn name_tag(b: &mut B, rect: Rect, label: &str) {
    let style = b.font(Font::Micro);
    let (w, _) = b.measure(label, &style);
    let tag = Rect::new(rect.left, rect.top - b.px(22.0), w + b.px(12.0), b.px(18.0));
    let tag = if tag.top < 0.0 { tag.offset(0.0, rect.height() + b.px(26.0)) } else { tag };
    b.fill(tag, b.px(4.0), b.p.solid);
    b.text_in(tag, label, &style, b.p.on_solid, Align::Center);
}

fn dim(op: Op, by: f32) -> Op {
    let fade = |c: Rgba| c.with_alpha(c.a * by);
    match op {
        Op::Fill { rect, radius, color } => Op::Fill { rect, radius, color: fade(color) },
        Op::Stroke { rect, radius, width, color } => Op::Stroke { rect, radius, width, color: fade(color) },
        Op::Text { rect, text, style, color } => Op::Text { rect, text, style, color: fade(color) },
        Op::Emphasis {rect,text,style,color,italic,strike}=>Op::Emphasis {rect,text,style,color:fade(color),italic,strike},
        Op::Clipped {rect,ops}=>Op::Clipped {rect,ops:ops.iter().cloned().map(|op|dim(op,by)).collect::<Vec<_>>().into()},
        Op::Line { from, to, width, color } => Op::Line { from, to, width, color: fade(color) },
        Op::Image { rect, image, opacity, source } => Op::Image { rect, image, opacity: opacity * by, source },
        Op::RoundedImage { rect, image, opacity, source, radius } => Op::RoundedImage { rect, image, opacity: opacity * by, source, radius },
        other => other,
    }
}

/// The centred crop of `image` that covers `rect` without stretching.
fn cover_source(rect: Rect, image: &Image) -> Rect {
    let (iw, ih) = (image.width as f32, image.height as f32);
    let scale = (rect.width() / iw).max(rect.height() / ih);
    let (sw, sh) = (rect.width() / scale, rect.height() / scale);
    Rect::new((iw - sw) / 2.0, (ih - sh) / 2.0, sw, sh)
}

/// `image` filling `rect`, cropped to its shape (cover).
pub fn image_cover(b: &mut B, rect: Rect, image: &Arc<Image>, opacity: f32) {
    b.list.ops.push(Op::Image {
        rect,
        image: image.clone(),
        opacity,
        source: Some(cover_source(rect, image)),
    });
}

/// [`image_cover`] with its corners rounded to `radius` (device px), clipped clean: pictures in
/// tiles and cards.
pub fn image_round(b: &mut B, rect: Rect, image: &Arc<Image>, opacity: f32, radius: f32) {
    b.list.ops.push(Op::RoundedImage {
        rect,
        image: image.clone(),
        opacity,
        source: Some(cover_source(rect, image)),
        radius,
    });
}

/// Which page a click on the Home stage opens.
pub fn home_pick(item: Option<&Item>) -> Action {
    match item.map(|i| &i.what) {
        None => Action::Nav(Page::Wallpapers),
        Some(What::Widget(id)) => Action::SelectWidget(Some(id.clone())),
        Some(What::Bar(bar)) => Action::SelectBar(Some(*bar)),
        Some(What::Module(slot)) => Action::SelectModule(Some(slot.clone())),
    }
}

