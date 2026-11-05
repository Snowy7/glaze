//! Bars: the top bar and dock at their real size on a readable width. Nothing selected shows
//! the bars' switches, presets (the old pairings), the Windows taskbar and the panels. A bar
//! shows its design, the common few, its modules (Start, Middle, End lists: reorder with the
//! keyboard or by dragging, add, remove) or its pinned apps, then More (layout, clock, look,
//! motion, displays). A module shows its clicks and its look; your own module its command.

use mypaper_bars::{BarsConfig, ModuleRef};
use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Point, Rect, Role};
use serde_json::{Value, json};

use crate::actions::{Action, Bar, Choices, Edit, FieldId, Page, Panel, Pins, Slot};
use crate::app::{App, Drag, Overlay};
use crate::docs::{DocId, Op, token};
use crate::settings::Place;
use crate::stage::{Show, What};
use crate::theme::glyph;
use crate::ui::{B, Col, inspector, label, section, setting};
use crate::view::split;

/// Presets shown as tiles (Neon glass loads as Floating: its decoration is gone).
const PRESETS: [(&str, &str); 7] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("windows11", "Windows 11"),
    ("windows10", "Windows 10"),
    ("chromeos", "ChromeOS"),
    ("islands", "Islands"),
    ("floating", "Floating"),
];

const TOP_DESIGNS: [(&str, &str); 8] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("fluent", "Fluent"),
    ("capsule", "Capsule"),
    ("minimal", "Minimal"),
    ("islands", "Islands"),
    ("floating", "Floating"),
    ("neon", "Neon"),
];

const DOCK_DESIGNS: [(&str, &str); 9] = [
    ("monochrome", "Monochrome"),
    ("macos", "macOS"),
    ("windows11", "Windows 11"),
    ("windows10", "Windows 10"),
    ("chromeos", "ChromeOS"),
    ("fluent", "Fluent"),
    ("islands", "Islands"),
    ("floating", "Floating"),
    ("neon", "Neon"),
];

/// Modules, as Studio names them.
const MODULES: [(&str, &str, char); 15] = [
    ("start", "Start", glyph::START),
    ("search", "Search", glyph::SEARCH),
    ("workspaces", "Desktops", glyph::LAYERS),
    ("app", "App", glyph::APP),
    ("clock", "Clock", glyph::CLOCK),
    ("media", "Media", glyph::MUSIC),
    ("tray", "Tray", glyph::CHEVRON_DOWN),
    ("network", "Network", glyph::WIFI),
    ("volume", "Volume", glyph::SOUND),
    ("battery", "Battery", glyph::BATTERY),
    ("quick", "Quick settings", glyph::TOGGLES),
    ("power", "Power", glyph::POWER),
    ("stats", "CPU and memory", glyph::CHIP),
    ("language", "Keyboard language", glyph::KEYBOARD),
    ("notifications", "Notifications", glyph::BELL),
];

fn config(app: &App) -> Option<BarsConfig> {
    BarsConfig::parse(&serde_json::to_string(&app.docs.doc(DocId::Bars).raw).ok()?).ok()
}

fn module_key(m: &ModuleRef) -> String {
    serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

pub fn module_name(m: &ModuleRef) -> String {
    let k = module_key(m);
    if let Some(id) = k.strip_prefix("custom:") {
        return format!("{id} (yours)");
    }
    if k == "|" {
        return "Gap".into();
    }
    MODULES.iter().find(|(id, ..)| *id == k).map_or(k.clone(), |(_, n, _)| (*n).to_owned())
}

fn module_glyph(k: &str) -> char {
    if k.starts_with("custom:") {
        return glyph::TERMINAL;
    }
    MODULES.iter().find(|(id, ..)| *id == k).map_or(glyph::GRIP, |(.., g)| *g)
}

/// "macOS menu bar", "Floating dock" (Home and the overview).
pub fn design_name(app: &App, bar: Bar) -> String {
    let path = format!("/{}/design", bar.key());
    let design = app.get(DocId::Bars, &path).and_then(Value::as_str).unwrap_or("monochrome").to_owned();
    let enabled = app.get(DocId::Bars, &format!("/{}/enabled", bar.key())).and_then(Value::as_bool).unwrap_or(false);
    if !enabled {
        return "Off".into();
    }
    let designs: &[(&str, &str)] = if bar == Bar::Top { &TOP_DESIGNS } else { &DOCK_DESIGNS };
    let name = designs.iter().find(|(id, _)| *id == design).map_or(design.clone(), |(_, n)| (*n).to_owned());
    match bar {
        Bar::Top => format!("{name} menu bar"),
        Bar::Dock => {
            let style = app.get(DocId::Bars, "/dock/style").and_then(Value::as_str).unwrap_or("floating");
            let magnify = app.get(DocId::Bars, "/dock/magnify/enabled").and_then(Value::as_bool).unwrap_or(false);
            format!("{name} {}{}", if style == "taskbar" { "taskbar" } else { "dock" }, if magnify { " · magnify" } else { "" })
        }
    }
}

pub fn title(app: &App) -> (String, String) {
    let preset = app.get(DocId::Bars, "/preset").and_then(Value::as_str).unwrap_or("monochrome");
    let name = PRESETS.iter().find(|(id, _)| *id == preset).map_or(preset, |(_, n)| n);
    ("Bars".into(), format!("{name} preset"))
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let (stage_side, side) = split(app, body);
    let mut top = 0.0;
    if app.docs.error(DocId::Bars).is_some() {
        top = crate::ui::file_banner(app, b, Rect::new(stage_side.left + b.px(24.0), stage_side.top + b.px(16.0), stage_side.width() - b.px(48.0), 0.0), DocId::Bars);
    } else if !app.system.parts[2].running && !app.fixture {
        top = crate::ui::banner(b, Rect::new(stage_side.left + b.px(24.0), stage_side.top + b.px(16.0), stage_side.width() - b.px(48.0), 0.0), "The bars aren't running. Changes apply when they start.", &[("Start", Action::StartBars)]) + b.px(12.0);
    }
    let frame = crate::pages::stage_in(app, body).offset(0.0, top / 2.0);
    let items = crate::stage::draw(app, b, frame, Show::Bars, |item| match item.map(|i| &i.what) {
        Some(What::Bar(bar)) => Action::SelectBar(Some(*bar)),
        Some(What::Module(slot)) => Action::SelectModule(Some(slot.clone())),
        _ => Action::SelectBar(None),
    });
    for item in &items {
        let on = match &item.what {
            What::Bar(bar) => app.nav.module.is_none() && app.nav.bar == Some(*bar),
            What::Module(slot) => app.nav.module.as_ref() == Some(slot),
            What::Widget(_) => false,
        };
        if on {
            crate::stage::outline(b, item.rect, item.radius, true);
            crate::stage::name_tag(b, Rect::new(item.rect.left, item.rect.bottom + b.px(26.0), item.rect.width(), 0.0), &item.label);
        }
    }
    let y = frame.bottom + b.px(14.0);
    let x = crate::pages::display_chips(app, b, frame.left + b.px(56.0), y);
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(x + b.px(4.0), y, b.px(320.0), b.px(28.0)), "Drag a module to move it, or off the bar to remove it", &caption, p.ink3, Align::Start);
    let Some(config) = config(app) else {
        inspector(app, b, side, key("bars-inspector-broken"), |b, col| {
            section(b, col, "Bars", |b, c| {
                let caption = b.font(Font::Caption);
                c.y += b.paragraph(c.x, c.y, c.w, "Your bar settings can't be read. Start over keeps a backup.", &caption, b.px(16.0), b.p.ink3);
            });
        });
        return;
    };
    let view = key(("bars-inspector", format!("{:?}{:?}{:?}", app.nav.bar, app.nav.module, app.nav.panel)));
    inspector(app, b, side, view, |b, col| {
        if let Some(slot) = app.nav.module.clone() {
            module(app, b, col, &config, &slot);
        } else if let Some(panel) = app.nav.panel {
            panel_inspector(app, b, col, panel);
            if panel == Panel::Start {
                pins(app, b, col, &config, Pins::Start);
            }
        } else if let Some(bar) = app.nav.bar {
            bar_inspector(app, b, col, &config, bar);
        } else {
            overview(app, b, col, &config);
        }
    });
}

fn overview(app: &App, b: &mut B, col: &mut Col, config: &BarsConfig) {
    crate::ui::header(b, col, None, glyph::BARS, "Bars", "Click a bar or a module to change it", &[]);
    section(b, col, "Bars", |b, c| {
        for (bar, name, g, enabled) in [(Bar::Top, "Top bar", glyph::BARS, config.top.enabled), (Bar::Dock, "Dock", glyph::DESKTOP, config.dock.enabled)] {
            let row = Rect::new(c.x, c.y, c.w, b.px(46.0));
            let tile = Rect::new(c.x, row.top + b.px(7.0), b.px(32.0), b.px(32.0));
            b.fill(tile, b.px(8.0), b.p.hover);
            b.icon(tile, g, 15.0, b.p.ink2);
            let k = key(("bar-row", name));
            let strong = b.font(Font::Strong);
            let caption = b.font(Font::Caption);
            b.text_at(tile.right + b.px(12.0), row.top + b.px(6.0), name, &strong, b.p.ink1);
            b.text_at(tile.right + b.px(12.0), row.top + b.px(25.0), &design_name(app, bar), &caption, b.p.ink3);
            b.hit(k, Rect::new(c.x, row.top, c.w - b.px(52.0), row.height()), Role::Button, name, Action::SelectBar(Some(bar)), mypaper_ui::controls::Behavior::Click);
            b.switch(key(("bar-on", name)), Point::new(c.x + c.w - b.px(36.0), row.top + b.px(13.0)), enabled, name, Action::Toggle(Edit::new(DocId::Bars, format!("/{}/enabled", bar.key()), name), enabled));
            c.y += row.height() + b.px(4.0);
        }
    });
    section(b, col, "Presets", |b, c| {
        let current = app.get(DocId::Bars, "/preset").and_then(Value::as_str).unwrap_or("monochrome").to_owned();
        let items: Vec<(&str, bool, Action)> = PRESETS.iter().map(|(id, n)| (*n, current == *id, Action::Preset(id))).collect();
        crate::ui::anchor(app, "preset", c.view, c.y);
        crate::ui::tiles(b, c, key("presets"), 3, false, &items, |b, i, pic| silhouette(app, b, PRESETS[i].0, pic));
    });
    section(b, col, "Windows taskbar", |b, c| {
        setting(app, b, c, "taskbar_mode");
        setting(app, b, c, "taskbar_alignment");
    });
    section(b, col, "Panels", |b, c| {
        let start = app.get(DocId::Bars, "/launcher/layout").and_then(Value::as_str).unwrap_or("win11");
        let zones = config.calendar.zones.len();
        crate::ui::nav_row(b, c, glyph::START, "Start menu", &format!("{} layout{}", if start == "grid" { "Grid" } else { "Windows 11" }, if config.launcher.win_key { " · Windows key opens it" } else { "" }), Action::SelectPanel(Some(Panel::Start)));
        crate::ui::nav_row(b, c, glyph::CALENDAR, "Calendar", &format!("{zones} other clock{}", if zones == 1 { "" } else { "s" }), Action::SelectPanel(Some(Panel::Calendar)));
        crate::ui::nav_row(b, c, glyph::SOUND, "Sound", if config.audio.switch_output { "Can switch the output device" } else { "Volume only" }, Action::SelectPanel(Some(Panel::Sound)));
        crate::ui::nav_row(b, c, glyph::CHEVRON_DOWN, "Tray", if config.tray.host { "Collects the tray icons" } else { "Windows keeps them" }, Action::SelectPanel(Some(Panel::Tray)));
        crate::ui::nav_row(b, c, glyph::BELL, "Notifications", "Win+N and Win+A open mypaper's", Action::SelectPanel(Some(Panel::Notifications)));
    });
    let overrides: Vec<String> = app.get(DocId::Bars, "/overrides").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    if !overrides.is_empty() {
        section(b, col, "Per display", |b, c| {
            for name in &overrides {
                let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
                label(b, row, name, "Its own settings", None);
                let w = b.button_width("Remove", false, true);
                b.button(key(("override", name.as_str())), Rect::new(row.right - w, row.top + b.px(4.0), w, b.px(28.0)), "Remove", None, ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Bars, format!("/overrides/{}", token(name)), "Per display"), Value::Null));
                c.y += b.px(40.0);
            }
        });
    }
}

/// A preset's silhouette: where its bars sit.
fn silhouette(app: &App, b: &mut B, preset: &str, pic: Rect) {
    if let Some(still) = app.library.active().and_then(|i| app.library.items[i].preview.clone())
        && let Some(image) = app.images.borrow_mut().get(&still, (pic.width() as u32 * 2, pic.height() as u32 * 2))
    {
        crate::stage::image_cover(b, pic, &image, 1.0);
    } else {
        b.fill(pic, b.px(8.0), mypaper_ui::Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0));
    }
    b.stroke(pic.inflate(b.px(3.0)), b.px(11.0), b.px(6.0), b.p.chrome);
    let glass = mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.22);
    let rim = mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.55);
    let (w, h) = (pic.width(), pic.height());
    let mut bar = |r: Rect, radius: f32| {
        b.fill(r, radius, glass);
        b.stroke(r, radius, 1.0, rim);
    };
    match preset {
        "windows11" | "windows10" | "chromeos" => {
            bar(Rect::new(pic.left, pic.bottom - h * 0.12, w, h * 0.12), 0.0);
            if preset == "chromeos" {
                bar(Rect::new(pic.right - w * 0.18, pic.bottom - h * 0.1, w * 0.14, h * 0.08), 3.0);
            }
        }
        "islands" => {
            for x in [0.06, 0.4, 0.78] {
                bar(Rect::new(pic.left + w * x, pic.top + h * 0.06, w * 0.16, h * 0.1), 3.0);
            }
            bar(Rect::new(pic.left + w * 0.32, pic.bottom - h * 0.16, w * 0.36, h * 0.12), 4.0);
        }
        _ => {
            let floating = preset == "floating";
            let m = if floating { w * 0.04 } else { 0.0 };
            bar(Rect::new(pic.left + m, pic.top + if floating { h * 0.05 } else { 0.0 }, w - m * 2.0, h * 0.09), if floating { 3.0 } else { 0.0 });
            bar(Rect::new(pic.left + w * 0.3, pic.bottom - h * 0.17, w * 0.4, h * 0.12), 4.0);
        }
    }
}

fn bar_inspector(app: &App, b: &mut B, col: &mut Col, config: &BarsConfig, bar: Bar) {
    let (name, detail) = match bar {
        Bar::Top => ("Top bar", design_name(app, Bar::Top)),
        Bar::Dock => ("Dock", design_name(app, Bar::Dock)),
    };
    crate::ui::header(b, col, Some(("Bars", Action::SelectBar(None))), if bar == Bar::Top { glyph::BARS } else { glyph::DESKTOP }, name, &detail, &[(glyph::RESET, "Reset its look and motion", Action::ResetBar(bar))]);
    section(b, col, "", |b, c| {
        let on = if bar == Bar::Top { config.top.enabled } else { config.dock.enabled };
        let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
        label(b, row, &format!("Show the {}", name.to_lowercase()), "", None);
        b.switch(key(("show-bar", name)), Point::new(c.x + c.w - b.px(36.0), c.y + b.px(8.0)), on, name, Action::Toggle(Edit::new(DocId::Bars, format!("/{}/enabled", bar.key()), name), on));
        c.y += b.px(40.0);
    });
    section(b, col, "Design", |b, c| {
        let designs: &[(&str, &str)] = if bar == Bar::Top { &TOP_DESIGNS } else { &DOCK_DESIGNS };
        let current = app.get(DocId::Bars, &format!("/{}/design", bar.key())).and_then(Value::as_str).unwrap_or("monochrome").to_owned();
        let edit = Edit::new(DocId::Bars, format!("/{}/design", bar.key()), "Design");
        let items: Vec<(&str, bool, Action)> = designs.iter().map(|(id, n)| (*n, *id == current, Action::Set(edit.clone(), json!(id)))).collect();
        crate::ui::anchor(app, if bar == Bar::Top { "top_design" } else { "dock_design" }, c.view, c.y);
        crate::ui::tiles(b, c, key(("designs", name)), 3, false, &items, |b, i, pic| design_tile(app, b, bar, designs[i].0, pic));
    });
    section(b, col, "Layout", |b, c| {
        let ids: &[&str] = if bar == Bar::Top { &["top_edge", "top_height", "top_shape"] } else { &["dock_style", "dock_edge", "dock_size", "dock_autohide", "dock_magnify"] };
        for id in ids {
            setting(app, b, c, id);
        }
    });
    if bar == Bar::Dock {
        pins(app, b, col, config, Pins::Dock);
    }
    if bar == Bar::Top || config.dock.style == mypaper_bars::config::DockStyle::Taskbar {
        modules(app, b, col, config, bar);
    }
    let place = Place::Bars(Some(bar));
    let groups: &[(&str, &[&str])] = if bar == Bar::Top {
        &[
            ("Layout", &["top_margin", "top_padding", "top_spacing"]),
            ("Clock", &["top_clock_seconds", "top_clock_format"]),
            ("Look", &["top_style", "top_scheme", "top_radius", "top_font_size"]),
            ("Motion", &["top_motion", "top_motion_style", "top_motion_curve", "top_motion_duration"]),
            ("Displays", &["top_displays"]),
        ]
    } else {
        &[
            ("Layout", &["dock_margin", "dock_padding", "dock_spacing", "dock_magnify_scale", "dock_magnify_range"]),
            ("Taskbar", &["dock_align", "dock_titles", "dock_button", "dock_clock_seconds"]),
            ("Apps", &["dock_running", "dock_split", "dock_counter", "dock_previews", "dock_preview_delay", "dock_bounce", "dock_attention", "dock_indicator"]),
            ("Look", &["dock_style_own", "dock_radius"]),
            ("Motion", &["dock_motion", "dock_motion_style"]),
            ("Displays", &["dock_displays"]),
        ]
    };
    for (group, ids) in groups {
        if *group == "Taskbar" && config.dock.style != mypaper_bars::config::DockStyle::Taskbar {
            continue;
        }
        if crate::ui::more(app, b, col, place, group, "") {
            section(b, col, "", |b, c| {
                for id in *ids {
                    setting(app, b, c, id);
                }
                if *group == "Apps" {
                    extras(app, b, c, config);
                }
            });
        }
    }
}

/// The dock's extras after the apps: Show desktop, Recycle Bin.
fn extras(app: &App, b: &mut B, c: &mut Col, config: &BarsConfig) {
    for (id, name) in [("desktop", "Show desktop"), ("trash", "Recycle Bin")] {
        let on = config.dock.extras.iter().any(|e| serde_json::to_value(e).ok().and_then(|v| v.as_str().map(str::to_owned)).as_deref() == Some(id));
        let mut list: Vec<Value> = config.dock.extras.iter().filter_map(|e| serde_json::to_value(e).ok()).collect();
        if on {
            list.retain(|v| v.as_str() != Some(id));
        } else {
            list.push(json!(id));
        }
        let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
        label(b, row, name, "After the apps", None);
        b.switch(key(("extra", id)), Point::new(c.x + c.w - b.px(36.0), c.y + b.px(8.0)), on, name, Action::Set(Edit::new(DocId::Bars, "/dock/extras", name), Value::Array(list)));
        c.y += b.px(40.0);
    }
    crate::ui::anchor(app, "dock_extras", c.view, c.y);
}

/// A design's thumbnail: the design's own bar, real view, over the wallpaper.
fn design_tile(app: &App, b: &mut B, bar: Bar, design: &str, pic: Rect) {
    silhouette(app, b, "none", pic);
    let glass = mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.18);
    let rim = mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.5);
    let (w, h) = (pic.width(), pic.height());
    // Each design's character: strip, floating, islands, capsule, taskbar.
    let shapes: Vec<(Rect, f32)> = match (bar, design) {
        (Bar::Top, "islands") => [0.04, 0.38, 0.74].iter().map(|x| (Rect::new(pic.left + w * x, pic.top + h * 0.08, w * 0.2, h * 0.12), 4.0)).collect(),
        (Bar::Top, "capsule") => vec![(Rect::new(pic.left + w * 0.25, pic.top + h * 0.08, w * 0.5, h * 0.12), 8.0)],
        (Bar::Top, "floating" | "macos" | "neon") => vec![(Rect::new(pic.left + w * 0.05, pic.top + h * 0.07, w * 0.9, h * 0.12), 4.0)],
        (Bar::Top, "minimal") => vec![(Rect::new(pic.left + w * 0.42, pic.top + h * 0.08, w * 0.16, h * 0.1), 3.0)],
        (Bar::Top, _) => vec![(Rect::new(pic.left, pic.top, w, h * 0.12), 0.0)],
        (Bar::Dock, "windows11" | "windows10" | "chromeos") => vec![(Rect::new(pic.left, pic.bottom - h * 0.14, w, h * 0.14), 0.0)],
        (Bar::Dock, _) => vec![(Rect::new(pic.left + w * 0.28, pic.bottom - h * 0.2, w * 0.44, h * 0.14), 5.0)],
    };
    for (r, radius) in shapes {
        b.fill(r, radius, glass);
        b.stroke(r, radius, 1.0, rim);
    }
}

/// The module lists (Start, Middle, End; the dock's taskbar style: Start, End).
fn modules(app: &App, b: &mut B, col: &mut Col, config: &BarsConfig, bar: Bar) {
    let lists: Vec<(&'static str, &str, &[ModuleRef])> = match bar {
        Bar::Top => vec![("left", "Start", &config.top.left), ("center", "Middle", &config.top.center), ("right", "End", &config.top.right)],
        Bar::Dock => vec![("left", "Start", &config.dock.left), ("right", "End", &config.dock.right)],
    };
    section(b, col, "Modules", |b, c| {
        crate::ui::heading_action(b, c, c.y - b.px(28.0), "Add", Some(glyph::ADD), Action::AddModule(bar));
        crate::ui::anchor(app, if bar == Bar::Top { "top_modules" } else { "dock_modules" }, c.view, c.y);
        for (list, name, items) in lists {
            let caption = b.font(Font::Caption);
            b.text_at(c.x, c.y, name, &caption, b.p.ink3);
            c.y += b.px(22.0);
            let rect = Rect::new(c.x, c.y, c.w, (items.len().max(1)) as f32 * b.px(36.0));
            b.fill(rect, b.px(8.0), b.p.chrome);
            if items.is_empty() {
                b.text_in(rect.inflate(-b.px(12.0)), "Empty: drag a module here", &caption, b.p.ink4, Align::Start);
            }
            b.group(key(("modules", bar.key(), list)), |b| {
                for (i, m) in items.iter().enumerate() {
                    let k = module_key(m);
                    let slot = Slot { bar, list, index: i };
                    let r = Rect::new(c.x, c.y + i as f32 * b.px(36.0), c.w, b.px(36.0));
                    crate::ui::list_row(b, r, key(("module", bar.key(), list, i)), Some(module_glyph(&k)), &module_name(m), Action::SelectModule(Some(slot.clone())), Some(Action::RemoveModule(slot)), i + 1 == items.len());
                }
            });
            b.stroke(rect, b.px(8.0), 1.0, b.p.line);
            // A drop caret while dragging a module over this list.
            if let Some(Drag::Module { moved: true, .. }) = &app.drag
                && let Some(pt) = b.input.pointer
                && rect.inflate(b.px(8.0)).contains(pt)
            {
                let i = (((pt.y - rect.top) / b.px(36.0)).round() as usize).min(items.len());
                let y = rect.top + i as f32 * b.px(36.0);
                b.fill(Rect::new(rect.left, y - 1.0, rect.width(), b.px(2.0)), 1.0, b.p.ink1);
            }
            c.y = rect.bottom + b.px(10.0);
        }
    });
}

/// A pinned-apps list: its own, or what it follows (the dock: Windows' pins; Start: the dock's).
fn pins(app: &App, b: &mut B, col: &mut Col, config: &BarsConfig, which: Pins) {
    let (anchor, follows) = match which {
        Pins::Dock => ("dock_pinned", "Windows' pins"),
        Pins::Start => ("start_pinned", "The dock's"),
    };
    section(b, col, "Pinned apps", |b, c| {
        let own = app.docs.is_set(DocId::Bars, which.path());
        if own {
            crate::ui::heading_action(b, c, c.y - b.px(28.0), "Add", Some(glyph::ADD), Action::PinAdd(which));
        }
        crate::ui::anchor(app, anchor, c.view, c.y);
        b.segmented(key(("pin-source", which)), Rect::new(c.x, c.y, c.w, b.px(32.0)), &[follows, "Your own"], Some(usize::from(own)), "Pinned apps", true, |i| {
            if i == 0 { Action::Set(Edit::new(DocId::Bars, which.path(), "Pinned apps"), Value::Null) } else { Action::PinsOwn(which) }
        });
        c.y += b.px(40.0);
        if !own {
            return;
        }
        let pins = own_pins(config, which);
        let rect = Rect::new(c.x, c.y, c.w, pins.len().max(1) as f32 * b.px(36.0));
        b.fill(rect, b.px(8.0), b.p.chrome);
        b.group(key(("pins", which)), |b| {
            for (i, pin) in pins.iter().enumerate() {
                let name = pin.name.clone().unwrap_or_else(|| mypaper_bars::apps::file_stem(&pin.path));
                let r = Rect::new(c.x, c.y + i as f32 * b.px(36.0), c.w, b.px(36.0));
                crate::ui::list_row(b, r, key(("pin", which, i)), Some(glyph::APP), &name, Action::PinRemove(which, i), Some(Action::PinRemove(which, i)), i + 1 == pins.len());
            }
        });
        b.stroke(rect, b.px(8.0), 1.0, b.p.line);
        c.y = rect.bottom + b.px(8.0);
    });
}

fn module(app: &App, b: &mut B, col: &mut Col, config: &BarsConfig, slot: &Slot) {
    let list: &[ModuleRef] = match (slot.bar, slot.list) {
        (Bar::Top, "left") => &config.top.left,
        (Bar::Top, "center") => &config.top.center,
        (Bar::Top, _) => &config.top.right,
        (Bar::Dock, "left") => &config.dock.left,
        (Bar::Dock, _) => &config.dock.right,
    };
    let Some(m) = list.get(slot.index) else { return };
    let k = module_key(m);
    let back = if slot.bar == Bar::Top { "Top bar" } else { "Dock" };
    crate::ui::header(b, col, Some((back, Action::SelectModule(None))), module_glyph(&k), &module_name(m), &format!("In the {} list", match slot.list { "left" => "Start", "center" => "Middle", _ => "End" }), &[(glyph::DELETE, "Remove from this bar", Action::RemoveModule(slot.clone()))]);
    let base = format!("/modules/{}", token(&k));
    section(b, col, "When clicked", |b, c| click(app, b, c, &base, "left", "Click"));
    if crate::ui::more(app, b, col, Place::Bars(Some(slot.bar)), "Other clicks and scrolling", "") {
        section(b, col, "", |b, c| {
            for (trigger, name) in [("middle", "Middle click"), ("right", "Right click"), ("wheel_up", "Scroll up"), ("wheel_down", "Scroll down")] {
                click(app, b, c, &base, trigger, name);
            }
        });
    }
    if crate::ui::more(app, b, col, Place::Bars(Some(slot.bar)), "Icon and look", "") {
        section(b, col, "", |b, c| {
            for (path, name, range) in [("style/padding", "Padding", (0.0, 24.0)), ("style/radius", "Corners", (0.0, 16.0)), ("style/size", "Text size", (9.0, 24.0)), ("style/border/width", "Edge", (0.0, 8.0))] {
                let full = format!("{base}/{path}");
                let v = app.get(DocId::Bars, &full).and_then(Value::as_f64).unwrap_or(range.0);
                let lw = b.px(100.0);
                label(b, Rect::new(c.x, c.y, lw, b.px(40.0)), name, "", None);
                b.slider(key(("module-style", path)), Rect::new(c.x + lw, c.y, c.w - lw, b.px(40.0)), v, (range.0, range.1, 1.0), &format!("{v:.0} px"), b.px(44.0), name, Action::Slider(Edit::new(DocId::Bars, full, name)));
                c.y += b.px(40.0);
            }
            weight(app, b, c, &base);
            let body = b.font(Font::Body);
            // Colours as the bars write them: "#rrggbb", "accent", "text" or "transparent".
            for (path, name, placeholder) in [
                ("icon", "Icon", "An icon-pack name or a picture"),
                ("style/color", "Text color", "Follows the bar, or #rrggbb, accent, text"),
                ("style/background", "Background", "None, or #rrggbb, accent"),
                ("style/border/color", "Edge color", "Follows the bar, or #rrggbb"),
            ] {
                let full = format!("{base}/{path}");
                b.text_at(c.x, c.y, name, &body, b.p.ink1);
                let v = app.get(DocId::Bars, &full).and_then(Value::as_str).unwrap_or("");
                crate::ui::text_field(app, b, Rect::new(c.x, c.y + b.px(24.0), c.w, b.px(32.0)), FieldId::Text(DocId::Bars, full.into()), None, placeholder, v, name);
                c.y += b.px(64.0);
            }
        });
    }
    if let Some(id) = k.strip_prefix("custom:") {
        section(b, col, "What it shows", |b, c| {
            let body = b.font(Font::Body);
            for (field, name, placeholder) in [("command", "Program", "C:\\path\\to\\program.exe"), ("interval", "Run every (seconds)", "60")] {
                b.text_at(c.x, c.y, name, &body, b.p.ink1);
                let v = app.get(DocId::Bars, &format!("{base}/{field}")).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).unwrap_or_default();
                crate::ui::text_field(app, b, Rect::new(c.x, c.y + b.px(24.0), c.w, b.px(32.0)), FieldId::Text(DocId::Bars, format!("{base}/{field}").into()), None, placeholder, &v, name);
                c.y += b.px(64.0);
            }
            // Arguments as one line, split like a command line (`intercept` stores the list).
            b.text_at(c.x, c.y, "Arguments", &body, b.p.ink1);
            let args: Vec<String> = app.get(DocId::Bars, &format!("{base}/args")).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            let shown = args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ");
            let field = FieldId::Text(DocId::Studio, format!("/module_args/{}", token(&k)).into());
            crate::ui::text_field(app, b, Rect::new(c.x, c.y + b.px(24.0), c.w, b.px(32.0)), field, None, "No arguments", &shown, "Arguments");
            c.y += b.px(64.0);
            let w = b.button_width("Delete everywhere", true, true);
            b.button(key("delete-custom"), Rect::new(c.x, c.y, w, b.px(28.0)), "Delete everywhere", Some(glyph::DELETE), ButtonKind::Secondary, true, Action::DeleteCustom(id.to_owned()));
            c.y += b.px(32.0);
        });
    }
}

/// The module's text weight: the bar's, or one of the usual few.
fn weight(app: &App, b: &mut B, c: &mut Col, base: &str) {
    let path = format!("{base}/style/weight");
    let options: Vec<(Value, String)> = [(Value::Null, "Follows the bar"), (json!(300), "Light"), (json!(400), "Regular"), (json!(500), "Medium"), (json!(600), "Semibold"), (json!(700), "Bold")]
        .into_iter()
        .map(|(v, n)| (v, n.to_owned()))
        .collect();
    let current = app.get(DocId::Bars, &path).cloned().unwrap_or(Value::Null);
    let shown = options.iter().find(|(v, _)| *v == current).map_or_else(|| current.to_string(), |(_, n)| n.clone());
    let h = b.px(40.0);
    let w = b.select_width(&shown).max(b.px(130.0));
    label(b, Rect::new(c.x, c.y, c.w - w, h), "Weight", "", None);
    b.select(key("module-weight"), Rect::new(c.x + c.w - w, c.y + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), &shown, "Weight", Action::Choose(Choices {
        edit: Edit::new(DocId::Bars, path, "Weight"),
        options,
        current: Some(current),
    }));
    c.y += h + b.px(4.0);
}

/// A click trigger's action: the module's own, nothing, a built-in action or a program.
fn click(app: &App, b: &mut B, c: &mut Col, base: &str, trigger: &str, name: &str) {
    let path = format!("{base}/click/{trigger}");
    let current = app.get(DocId::Bars, &path).cloned();
    let builtins = [
        ("default", "Its own"),
        ("none", "Nothing"),
        ("start", "Start menu"),
        ("search", "Search"),
        ("calendar", "Calendar"),
        ("notifications", "Notifications"),
        ("quick-settings", "Quick settings"),
        ("mixer", "Volume mixer"),
        ("network", "Network"),
        ("task-manager", "Task Manager"),
        ("settings", "Windows Settings"),
        ("show-desktop", "Show desktop"),
        ("media-toggle", "Play or pause"),
        ("lock", "Lock"),
        ("explorer", "File Explorer"),
    ];
    let shown = match &current {
        None => "Its own".to_owned(),
        Some(Value::String(s)) => builtins.iter().find(|(id, _)| id == s).map_or(s.clone(), |(_, n)| (*n).to_owned()),
        Some(v) => v.get("exec").or(v.get("open")).and_then(Value::as_str).map_or("A program".into(), |s| s.rsplit(['\\', '/']).next().unwrap_or(s).to_owned()),
    };
    let h = b.px(40.0);
    let w = b.select_width(&shown).max(b.px(130.0));
    label(b, Rect::new(c.x, c.y, c.w - w, h), name, "", None);
    b.select(key(("click", trigger)), Rect::new(c.x + c.w - w, c.y + (h - b.px(30.0)) / 2.0, w, b.px(30.0)), &shown, name, Action::Choose(Choices {
        edit: Edit::new(DocId::Bars, path, name),
        options: builtins.iter().map(|(id, n)| (if *id == "default" { Value::Null } else { json!(id) }, (*n).to_owned())).collect(),
        current: Some(current.unwrap_or(Value::Null)),
    }));
    c.y += h + b.px(4.0);
}

fn panel_inspector(app: &App, b: &mut B, col: &mut Col, panel: Panel) {
    let (name, g, ids): (&str, char, &[&str]) = match panel {
        Panel::Start => ("Start menu", glyph::START, &["start_layout", "start_files", "start_win_key"]),
        Panel::Calendar => ("Calendar", glyph::CALENDAR, &["calendar_first_day"]),
        Panel::Sound => ("Sound", glyph::SOUND, &["sound_switch"]),
        Panel::Tray => ("Tray", glyph::CHEVRON_DOWN, &["tray_host"]),
        Panel::Notifications => ("Notifications", glyph::BELL, &[]),
    };
    crate::ui::header(b, col, Some(("Bars", Action::SelectPanel(None))), g, name, "A panel the bars open", &[]);
    section(b, col, "", |b, c| {
        for id in ids {
            setting(app, b, c, id);
        }
        match panel {
            Panel::Calendar => {
                crate::ui::anchor(app, "calendar_zones", c.view, c.y);
                let zones = app.get(DocId::Bars, "/calendar/zones").and_then(Value::as_array).cloned().unwrap_or_default();
                let body = b.font(Font::Body);
                b.text_at(c.x, c.y + b.px(6.0), "Other time zones", &body, b.p.ink1);
                c.y += b.px(30.0);
                for (i, z) in zones.iter().enumerate() {
                    let zone = z.get("zone").and_then(Value::as_str).unwrap_or("");
                    let r = Rect::new(c.x, c.y, c.w, b.px(36.0));
                    let mut rest = zones.clone();
                    rest.remove(i);
                    crate::ui::list_row(b, r, key(("zone", i)), Some(glyph::CLOCK), zone, Action::None, Some(Action::Set(Edit::new(DocId::Bars, "/calendar/zones", "Time zones"), Value::Array(rest))), i + 1 == zones.len());
                    c.y += b.px(36.0);
                }
                crate::ui::text_field(app, b, Rect::new(c.x, c.y + b.px(6.0), c.w, b.px(32.0)), FieldId::Text(DocId::Studio, "/new_zone".into()), None, "Add a zone, like Europe/London", "", "Add a time zone");
                c.y += b.px(46.0);
            }
            Panel::Notifications => {
                let caption = b.font(Font::Caption);
                c.y += b.paragraph(c.x, c.y, c.w, "mypaper's notification center opens with Win+N and Win+A, and from the bell module. It lists what Windows' notifications say, Discord and chat apps included.", &caption, b.px(16.0), b.p.ink3);
            }
            _ => {}
        }
    });
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::SelectBar(bar) => {
            app.go(Page::Bars);
            app.nav.bar = bar;
            app.nav.module = None;
            app.nav.panel = None;
        }
        Action::SelectModule(slot) => {
            app.go(Page::Bars);
            if let Some(s) = &slot {
                app.nav.bar = Some(s.bar);
            }
            app.nav.module = slot;
            app.nav.panel = None;
        }
        Action::SelectPanel(panel) => {
            app.go(Page::Bars);
            app.nav.panel = panel;
            app.nav.module = None;
            if panel.is_some() {
                app.nav.bar = None;
            }
        }
        Action::Preset(id) => {
            // A preset decides both bars' designs and which are on: clear the bars' own choices.
            let mut ops = vec![Op { doc: DocId::Bars, path: "/preset".into(), value: Some(json!(id)) }];
            for bar in ["top", "dock"] {
                for field in ["design", "enabled"] {
                    ops.push(Op { doc: DocId::Bars, path: format!("/{bar}/{field}"), value: None });
                }
            }
            let name = PRESETS.iter().find(|(p, _)| *p == id).map_or(id, |(_, n)| n);
            app.apply(&format!("{name} preset"), ops);
            app.toast_undo(format!("Bars: {name}"));
        }
        Action::AddModule(bar) => app.overlay = Some(Overlay::AddModule { bar }),
        Action::InsertModule(bar, list, module) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, bar, list);
            items.push(json!(module));
            app.apply("Add module", vec![Op { doc: DocId::Bars, path: format!("/{}/{list}", bar.key()), value: Some(Value::Array(items)) }]);
            app.overlay = None;
            app.toast_undo(format!("Added {}", module_name(&serde_json::from_value(json!(module)).unwrap_or(ModuleRef::Break))));
        }
        Action::RemoveModule(slot) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, slot.bar, slot.list);
            if slot.index < items.len() {
                items.remove(slot.index);
                app.apply("Remove module", vec![Op { doc: DocId::Bars, path: format!("/{}/{}", slot.bar.key(), slot.list), value: Some(Value::Array(items)) }]);
                app.nav.module = None;
                app.toast_undo("Removed the module");
            }
        }
        Action::MoveModule(slot, delta) => {
            let Some(config) = config(app) else { return };
            let mut items = list_of(&config, slot.bar, slot.list);
            let to = slot.index as i32 + delta;
            if to >= 0 && (to as usize) < items.len() {
                items.swap(slot.index, to as usize);
                app.apply("Move module", vec![Op { doc: DocId::Bars, path: format!("/{}/{}", slot.bar.key(), slot.list), value: Some(Value::Array(items)) }]);
                let moved = Slot { index: to as usize, ..slot };
                app.input.focus = Some(key(("module", moved.bar.key(), moved.list, moved.index)));
                app.input.focus_visible = true;
            }
        }
        Action::DeleteCustom(id) => {
            let Some(config) = config(app) else { return };
            let k = format!("custom:{id}");
            let mut ops = vec![Op { doc: DocId::Bars, path: format!("/modules/{}", token(&k)), value: None }];
            for (bar, lists) in [(Bar::Top, &["left", "center", "right"][..]), (Bar::Dock, &["left", "right"][..])] {
                for list in lists {
                    let items: Vec<Value> = list_of(&config, bar, list).into_iter().filter(|v| v.as_str() != Some(&k)).collect();
                    ops.push(Op { doc: DocId::Bars, path: format!("/{}/{list}", bar.key()), value: Some(Value::Array(items)) });
                }
            }
            app.apply("Delete module", ops);
            app.nav.module = None;
        }
        Action::ResetBar(bar) => {
            app.apply("Reset the bar", vec![
                Op { doc: DocId::Bars, path: format!("/{}/look", bar.key()), value: None },
                Op { doc: DocId::Bars, path: format!("/{}/animation", bar.key()), value: None },
            ]);
            app.toast_undo("Back to its design");
        }
        Action::StartBars => app.run(Action::StartMypaper),
        Action::PinsOwn(which) => {
            let Some(config) = config(app) else { return };
            let start = match which {
                Pins::Dock => mypaper_shell::preview::bars::default_pins(),
                Pins::Start => config.dock.pinned.unwrap_or_else(mypaper_shell::preview::bars::default_pins),
            };
            set_pins(app, which, "Your own pins", start.into_iter().filter_map(|p| serde_json::to_value(p).ok()).collect());
        }
        Action::PinAdd(which) => {
            if let Some(path) = crate::dialogs::pick(app.window, "Choose an app to pin", false, crate::dialogs::PROGRAMS) {
                let Some(config) = config(app) else { return };
                let mut pins = pin_values(&config, which);
                pins.push(json!({ "path": path.display().to_string() }));
                set_pins(app, which, "Pin an app", pins);
            }
        }
        Action::PinRemove(which, i) => {
            let Some(config) = config(app) else { return };
            let mut pins = pin_values(&config, which);
            if i < pins.len() && app.docs.is_set(DocId::Bars, which.path()) {
                pins.remove(i);
                set_pins(app, which, "Unpin", pins);
            }
        }
        Action::PinMove(which, i, delta) => {
            let Some(config) = config(app) else { return };
            let mut pins = pin_values(&config, which);
            let to = i as i32 + delta;
            if to >= 0 && (to as usize) < pins.len() {
                pins.swap(i, to as usize);
                set_pins(app, which, "Move pin", pins);
            }
        }
        _ => {}
    }
}

/// A list's own pins (empty while it follows another list).
fn own_pins(config: &BarsConfig, which: Pins) -> Vec<mypaper_bars::config::Pinned> {
    match which {
        Pins::Dock => config.dock.pinned.clone(),
        Pins::Start => config.launcher.pinned.clone(),
    }
    .unwrap_or_default()
}

fn pin_values(config: &BarsConfig, which: Pins) -> Vec<Value> {
    own_pins(config, which).into_iter().filter_map(|p| serde_json::to_value(p).ok()).collect()
}

fn set_pins(app: &mut App, which: Pins, label: &str, pins: Vec<Value>) {
    app.apply(label, vec![Op { doc: DocId::Bars, path: which.path().into(), value: Some(Value::Array(pins)) }]);
}

fn list_of(config: &BarsConfig, bar: Bar, list: &str) -> Vec<Value> {
    let items: &[ModuleRef] = match (bar, list) {
        (Bar::Top, "left") => &config.top.left,
        (Bar::Top, "center") => &config.top.center,
        (Bar::Top, _) => &config.top.right,
        (Bar::Dock, "left") => &config.dock.left,
        (Bar::Dock, _) => &config.dock.right,
    };
    items.iter().filter_map(|m| serde_json::to_value(m).ok()).collect()
}

/// Delete removes the selected module.
pub fn on_key(app: &mut App, vk: u16) -> bool {
    if vk == 0x2E
        && app.field.is_none()
        && let Some(slot) = app.nav.module.clone()
    {
        app.run(Action::RemoveModule(slot));
        return true;
    }
    false
}

pub fn drag_to(app: &mut App, drag: Drag, p: Point) {
    if let Drag::Module { from, .. } = drag
        && ((p.x - from.x).abs() > 4.0 || (p.y - from.y).abs() > 4.0)
        && let Some(Drag::Module { moved, .. }) = &mut app.drag
    {
        *moved = true;
        app.dirty = true;
    }
}

/// A dropped module: onto a list row (moved there) or off the bar (removed).
pub fn drop_drag(app: &mut App, drag: Drag) {
    let Drag::Module { slot, moved: true, .. } = drag else { return };
    let Some(p) = app.input.pointer else { return };
    // Over a list row of the inspector: the row's slot is where it goes.
    let target = app.frame.targets.iter().rev().find(|t| t.visible.is_some_and(|r| r.contains(p))).map(|t| t.action.clone());
    match target {
        Some(Action::SelectModule(Some(to))) if to.bar == slot.bar && to != slot => {
            let Some(config) = config(app) else { return };
            let mut from = list_of(&config, slot.bar, slot.list);
            if slot.index >= from.len() {
                return;
            }
            let item = from.remove(slot.index);
            let mut ops = Vec::new();
            if to.list == slot.list {
                from.insert(to.index.min(from.len()), item);
            } else {
                let mut dest = list_of(&config, to.bar, to.list);
                dest.insert(to.index.min(dest.len()), item);
                ops.push(Op { doc: DocId::Bars, path: format!("/{}/{}", to.bar.key(), to.list), value: Some(Value::Array(dest)) });
            }
            ops.push(Op { doc: DocId::Bars, path: format!("/{}/{}", slot.bar.key(), slot.list), value: Some(Value::Array(from)) });
            app.apply("Move module", ops);
        }
        Some(Action::SelectBar(None)) | Some(Action::None) | None => {
            // Dropped on the bare stage: off the bar.
            app.run(Action::RemoveModule(slot));
        }
        _ => {}
    }
}

/// Add a module: where, then the built-ins not shown, a gap, or your own.
pub fn add_sheet(app: &App, b: &mut B, bar: Bar) {
    let p = *b.p;
    let rect = crate::view::sheet(app, b, "Add a module", b.px(640.0), b.px(520.0));
    let Some(config) = config(app) else { return };
    let shown: Vec<String> = config.top.modules().chain(config.dock.modules()).map(module_key).collect();
    let lists: &[(&'static str, &str)] = if bar == Bar::Top { &[("left", "Start"), ("center", "Middle"), ("right", "End")] } else { &[("left", "Start"), ("right", "End")] };
    let target = app.text(&FieldId::Text(DocId::Studio, "/add_module_list".into())).to_owned();
    let at = lists.iter().position(|(id, _)| *id == target).unwrap_or(lists.len() - 1);
    let caption = b.font(Font::Caption);
    b.text_at(rect.left, rect.top, "Where", &caption, p.ink3);
    let labels: Vec<&str> = lists.iter().map(|(_, n)| *n).collect();
    b.segmented(key("add-module-where"), Rect::new(rect.left, rect.top + b.px(20.0), b.px(300.0), b.px(32.0)), &labels, Some(at), "Where", true, |i| Action::Set(Edit::new(DocId::Studio, "/add_module_list", "Where"), json!(lists[i].0)));
    let list = lists[at].0;
    let mut y = rect.top + b.px(68.0);
    b.text_at(rect.left, y, "Modules", &caption, p.ink3);
    y += b.px(22.0);
    let gap = b.px(8.0);
    let w = (rect.width() - gap * 2.0) / 3.0;
    let mut n: usize = 0;
    let available: Vec<(String, String, char)> = MODULES
        .iter()
        .filter(|(id, ..)| !shown.iter().any(|s| s == id))
        .map(|(id, name, g)| ((*id).to_owned(), (*name).to_owned(), *g))
        .chain(std::iter::once(("|".to_owned(), "A gap".to_owned(), glyph::GRIP)))
        .collect();
    b.group(key("add-module-list"), |b| {
        for (id, name, g) in &available {
            let r = Rect::new(rect.left + (n % 3) as f32 * (w + gap), y + (n / 3) as f32 * b.px(44.0), w, b.px(38.0));
            let k = key(("add-module", id.as_str()));
            b.fill(r, b.px(8.0), if b.hovered(k) { p.hover } else { p.bg });
            b.stroke(r, b.px(8.0), 1.0, p.edge);
            b.icon(Rect::new(r.left + b.px(10.0), r.top, b.px(18.0), r.height()), *g, 14.0, p.ink2);
            let body = b.font(Font::Body);
            b.text_in(Rect::new(r.left + b.px(36.0), r.top, r.width() - b.px(40.0), r.height()), name, &body, p.ink1, Align::Start);
            b.hit(k, r, Role::Button, &format!("Add {name}"), Action::InsertModule(bar, list, id.clone()), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, b.px(8.0));
            n += 1;
        }
    });
    y += (n.div_ceil(3)) as f32 * b.px(44.0) + b.px(12.0);
    b.text_at(rect.left, y, "From integrations", &caption, p.ink3);
    y += b.px(20.0);
    let small = b.font(Font::Caption);
    b.text_at(rect.left, y, "Integration modules (T3 Code, OBS) arrive with their integrations.", &small, p.ink4);
    y += b.px(30.0);
    b.text_at(rect.left, y, "Your own", &caption, p.ink3);
    let field_w = rect.width() - b.px(90.0);
    crate::ui::text_field(app, b, Rect::new(rect.left, y + b.px(20.0), field_w, b.px(32.0)), FieldId::Text(DocId::Studio, "/new_custom".into()), None, "A name, like weather-line", "", "Your own module's name");
    let custom = app.text(&FieldId::Text(DocId::Studio, "/new_custom".into())).trim().to_lowercase().replace(' ', "-");
    b.button(key("add-custom"), Rect::new(rect.left + field_w + b.px(8.0), y + b.px(20.0), b.px(82.0), b.px(32.0)), "Add", None, ButtonKind::Secondary, !custom.is_empty(), Action::InsertModule(bar, list, format!("custom:{custom}")));
}
