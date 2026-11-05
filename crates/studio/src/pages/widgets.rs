//! Widgets: the display's widgets on their real grid, drawn by their native views. Drag to
//! move, drag the corner to resize (drops onto another widget are refused), arrows move a cell
//! and Shift+arrows resize. The inspector lists what's on the display, or shows a widget's own
//! options from its manifest (variant selects as tiles drawn by the widget's real view), its size
//! and its own look. Add widget is a sheet with the catalogue.

use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Point, Rect, Role};
use mypaper_widgets::layout::{Instance, Layout};
use mypaper_widgets::manifest::{SelectOption, Setting as WidgetSetting, WidgetManifest};
use serde_json::{Map, Value, json};

use crate::actions::{Action, Choices, Edit, FieldId, Page};
use crate::app::{App, Drag, Overlay};
use crate::docs::{DocId, Op, token};
use crate::stage::{Show, What};
use crate::theme::glyph;
use crate::ui::{B, Col, inspector, label, section, swatches, text_field};
use crate::view::split;

/// The layout as the file says.
fn layout(app: &App) -> Layout {
    let text = serde_json::to_string(&app.docs.doc(DocId::Widgets).raw).unwrap_or_default();
    Layout::parse(&text).unwrap_or_default()
}

fn device(app: &App) -> String {
    app.display().map_or_else(String::new, |m| m.device.clone())
}

/// The instances showing on the stage's display.
fn here(app: &App, layout: &Layout) -> Vec<Instance> {
    let devices: Vec<String> = app.monitors.iter().map(|m| m.device.clone()).collect();
    let primary = app.monitors.iter().position(|m| m.primary).unwrap_or(0);
    let device = device(app);
    layout
        .instances
        .iter()
        .filter(|i| mypaper_widgets::layout::show_on(&i.monitor, &devices, primary) == Some(device.as_str()))
        .cloned()
        .collect()
}

pub fn count_here(app: &App) -> usize {
    here(app, &layout(app)).len()
}

pub fn glyph_of(id: &str) -> char {
    match id {
        "clock" => glyph::CLOCK,
        "calendar" => glyph::CALENDAR,
        "countdown" => glyph::TIMER,
        "crypto" => glyph::COIN,
        "heatmap" => glyph::HEATMAP,
        "launcher" => glyph::LAUNCH,
        "media-pill" | "now-playing" => glyph::MUSIC,
        "notes" => glyph::NOTES,
        "photo" => glyph::PHOTO,
        "quick-toggles" => glyph::TOGGLES,
        "quote" => glyph::QUOTE,
        "rss" => glyph::RSS,
        "system-monitor" => glyph::CHIP,
        "timer" => glyph::TIMER,
        "todo" => glyph::TODO,
        "weather" => glyph::WEATHER,
        _ => glyph::WIDGETS,
    }
}

pub fn title(app: &App) -> (String, String) {
    ("Widgets".into(), format!("{} on Display {}", count_here(app), app.nav.display + 1))
}

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let w = b.button_width("Add widget", true, false);
    let mut right = right - w;
    b.button(key("add-widget"), Rect::new(right, y, w, b.px(32.0)), "Add widget", Some(glyph::ADD), ButtonKind::Primary, true, Action::AddWidgetSheet);
    let aw = b.button_width("Arrange on desktop", true, false);
    right -= aw + b.px(8.0);
    b.button(key("arrange"), Rect::new(right, y, aw, b.px(32.0)), "Arrange on desktop", Some(glyph::MOVE), ButtonKind::Secondary, true, Action::ArrangeOnDesktop);
    crate::view::edit_tools(app, b, bar, right - b.px(6.0))
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let (stage_side, side) = split(app, body);
    let mut banner_h = 0.0;
    if app.docs.error(DocId::Widgets).is_some() {
        let r = Rect::new(stage_side.left + b.px(24.0), stage_side.top + b.px(16.0), stage_side.width() - b.px(48.0), 0.0);
        banner_h = crate::ui::file_banner(app, b, r, DocId::Widgets);
    }
    let frame = crate::pages::stage_in(app, body).offset(0.0, banner_h / 2.0);
    let selected = app.nav.widget.clone();
    let items = crate::stage::draw(app, b, frame, Show::Widgets, |item| match item.map(|i| &i.what) {
        Some(What::Widget(id)) => Action::SelectWidget(Some(id.clone())),
        _ => Action::SelectWidget(None),
    });
    let layout = layout(app);
    let here = here(app, &layout);
    if here.is_empty() {
        let w = b.button_width("Add a widget", true, false);
        b.button(key("empty-add"), Rect::new(frame.left + (frame.width() - w) / 2.0, frame.top + (frame.height() - b.px(32.0)) / 2.0, w, b.px(32.0)), "Add a widget", Some(glyph::ADD), ButtonKind::Primary, true, Action::AddWidgetSheet);
    }
    // While dragging: the grid's dots.
    if matches!(app.drag, Some(Drag::Widget { .. })) {
        dots(app, b, frame);
    }
    if let Some(id) = &selected
        && let Some(item) = items.iter().find(|i| i.what == What::Widget(id.clone()))
    {
        crate::stage::outline(b, item.rect, item.radius, true);
        crate::stage::name_tag(b, item.rect, &item.label);
        // The resize handle (its own target, so a drag there resizes).
        let h = Rect::new(item.rect.right - b.px(6.0), item.rect.bottom - b.px(6.0), b.px(10.0), b.px(10.0));
        b.fill(h, b.px(3.0), mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 1.0));
        b.stroke(h, b.px(3.0), 1.0, mypaper_ui::Rgba::new(0.0, 0.0, 0.0, 0.6));
        b.hit(key(("resize", id.as_str())), h.inflate(b.px(4.0)), Role::Button, "Resize widget", Action::SelectWidget(Some(id.clone())), mypaper_ui::controls::Behavior::Drag);
    }
    let y = frame.bottom + b.px(14.0);
    let x = crate::pages::display_chips(app, b, frame.left + b.px(80.0), y);
    let caption = b.font(Font::Caption);
    b.text_in(Rect::new(x + b.px(4.0), y, b.px(300.0), b.px(28.0)), "Drag to move, or by a corner to resize", &caption, p.ink3, Align::Start);
    let instance = selected.as_ref().and_then(|id| layout.get(id)).cloned();
    let manifest = instance.as_ref().and_then(|i| app.registry.get(&i.widget)).map(|m| m.manifest.clone());
    inspector(app, b, side, key(("widget-inspector", selected.clone())), |b, col| match (instance, manifest) {
        (Some(instance), Some(manifest)) => widget_inspector(app, b, col, &layout, &instance, &manifest),
        (Some(instance), None) => {
            crate::ui::header(b, col, Some(("All widgets", Action::SelectWidget(None))), glyph::ALERT, &instance.widget, "Not installed", &[(glyph::DELETE, "Remove", Action::RemoveWidget)]);
            section(b, col, "", |b, c| {
                let caption = b.font(Font::Caption);
                c.y += b.paragraph(c.x, c.y, c.w, "mypaper doesn't have this widget any more. Remove it, or bring its folder back.", &caption, b.px(16.0), b.p.ink3);
            });
        }
        _ => overview(app, b, col, &here),
    });
}

fn dots(app: &App, b: &mut B, frame: Rect) {
    let cache = app.stage.borrow();
    let Some((grid, scale)) = cache.content.grid else { return };
    let d = mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.35);
    for row in 0..=grid.rows {
        for c in 0..=grid.cols {
            let x = frame.left + (grid.origin[0] + f64::from(c) * (grid.cell[0] + grid.gap)) as f32 * scale;
            let y = frame.top + (grid.origin[1] + f64::from(row) * (grid.cell[1] + grid.gap)) as f32 * scale;
            b.fill(Rect::new(x - 1.0, y - 1.0, 2.0, 2.0), 1.0, d);
        }
    }
}

/// Nothing selected: always on top, what's on this display, and where their look is.
fn overview(app: &App, b: &mut B, col: &mut Col, here: &[Instance]) {
    crate::ui::header(b, col, None, glyph::WIDGETS, "Widgets", &format!("{} on Display {}", here.len(), app.nav.display + 1), &[]);
    section(b, col, "", |b, c| crate::ui::setting(app, b, c, "always_on_top"));
    section(b, col, "On this display", |b, c| {
        if here.is_empty() {
            let caption = b.font(Font::Caption);
            b.text_at(c.x, c.y, "No widgets here yet.", &caption, b.p.ink3);
            c.y += b.px(24.0);
        }
        b.group(key("widget-list"), |b| {
            for i in here {
                let name = app.registry.get(&i.widget).map_or(i.widget.clone(), |m| m.manifest.name.clone());
                crate::ui::nav_row(b, c, glyph_of(&i.widget), &name, &format!("{}×{} cells · column {}, row {}", i.size[0], i.size[1], i.cell[0] + 1, i.cell[1] + 1), Action::SelectWidget(Some(i.instance.clone())));
            }
        });
    });
    section(b, col, "", |b, c| {
        crate::ui::nav_row(b, c, glyph::LOOK, "How widgets look", "Look › Different for › Widgets", Action::Nav(Page::Look));
    });
}

fn widget_inspector(app: &App, b: &mut B, col: &mut Col, layout: &Layout, instance: &Instance, manifest: &WidgetManifest) {
    let index = layout.instances.iter().position(|i| i.instance == instance.instance).unwrap_or(0);
    let base = format!("/instances/{index}");
    crate::ui::header(
        b,
        col,
        Some(("All widgets", Action::SelectWidget(None))),
        glyph_of(&manifest.id),
        &manifest.name,
        &format!("{}×{} cells · Display {}", instance.size[0], instance.size[1], app.nav.display + 1),
        &[(glyph::COPY, "Duplicate", Action::DuplicateWidget), (glyph::DELETE, "Remove", Action::RemoveWidget)],
    );
    let resolved = manifest.resolve_settings(&instance.settings);
    // The manifest's groups, in order.
    let mut groups: Vec<String> = Vec::new();
    for s in &manifest.settings {
        let g = group_of(s).unwrap_or("Options").to_owned();
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    for group in &groups {
        section(b, col, group, |b, c| {
            for s in manifest.settings.iter().filter(|s| group_of(s).unwrap_or("Options") == group) {
                widget_setting(app, b, c, &base, &resolved, s, instance, manifest);
            }
            // One Look section: the widget's own look joins its manifest's.
            if group == "Look" {
                own_look(app, b, c, &base, instance);
            }
        });
    }
    if !groups.iter().any(|g| g == "Look") {
        section(b, col, "Look", |b, c| own_look(app, b, c, &base, instance));
    }
    section(b, col, "Size", |b, c| {
        let half = (c.w - b.px(8.0)) / 2.0;
        stepper(b, Rect::new(c.x, c.y, half, b.px(32.0)), "Wide", instance.size[0], Action::ResizeWidget(-1, 0), Action::ResizeWidget(1, 0));
        stepper(b, Rect::new(c.x + half + b.px(8.0), c.y, half, b.px(32.0)), "Tall", instance.size[1], Action::ResizeWidget(0, -1), Action::ResizeWidget(0, 1));
        c.y += b.px(40.0);
    });
}

/// "Its own look": a widget's appearance over Look's, with its background and tint.
fn own_look(app: &App, b: &mut B, c: &mut Col, base: &str, instance: &Instance) {
    let own = !instance.appearance.eq(&Default::default());
    let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
    label(b, row, "Its own look", "", if own { None } else { Some("Follows Look") });
    let edit = Edit::new(DocId::Widgets, format!("{base}/appearance/surface"), "Its own look");
    let action = if own {
        Action::Set(Edit::new(DocId::Widgets, format!("{base}/appearance"), "Follows Look"), Value::Null)
    } else {
        Action::Set(edit, json!("glass"))
    };
    b.switch(key("own-look"), Point::new(c.x + c.w - b.px(36.0), c.y + b.px(8.0)), own, "Its own look", action);
    c.y += b.px(40.0);
    if own {
        let surface = instance.appearance.surface.map_or("glass", |s| match s {
            mypaper_theme::Surface::None => "none",
            mypaper_theme::Surface::Glass => "glass",
            mypaper_theme::Surface::Solid => "solid",
        });
        let values = ["glass", "solid", "none"];
        let e = Edit::new(DocId::Widgets, format!("{base}/appearance/surface"), "Background");
        b.segmented(key("own-surface"), Rect::new(c.x, c.y, c.w, b.px(32.0)), &["Glass", "Solid", "None"], values.iter().position(|v| *v == surface), "Background", true, |i| Action::Set(e.clone(), json!(values[i])));
        c.y += b.px(40.0);
        let tint = Edit::new(DocId::Widgets, format!("{base}/appearance/tint"), "Tint");
        let current = app.get(DocId::Widgets, &format!("{base}/appearance/tint")).cloned();
        swatches(app, b, c, &tint, current.as_ref(), None, &[[0x34, 0x3a, 0x42], [0x8b, 0x9e, 0xb7], [0xc8, 0x8d, 0x3c], [0x7f, 0xae, 0x8e], [0xe6, 0xe6, 0xe6]]);
    }
}

fn group_of(s: &WidgetSetting) -> Option<&str> {
    match s {
        WidgetSetting::Boolean { group, .. }
        | WidgetSetting::Number { group, .. }
        | WidgetSetting::Slider { group, .. }
        | WidgetSetting::String { group, .. }
        | WidgetSetting::Select { group, .. }
        | WidgetSetting::Color { group, .. }
        | WidgetSetting::File { group, .. }
        | WidgetSetting::Image { group, .. }
        | WidgetSetting::Font { group, .. }
        | WidgetSetting::List { group, .. }
        | WidgetSetting::Place { group, .. } => group.as_deref(),
    }
}

fn option(o: &SelectOption) -> (String, String) {
    match o {
        SelectOption::Plain(v) => (v.clone(), capitalize(v)),
        SelectOption::Detailed(d) => (d.value.clone(), d.label.clone().unwrap_or_else(|| capitalize(&d.value))),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// One manifest setting as a row.
#[allow(clippy::too_many_arguments)]
fn widget_setting(app: &App, b: &mut B, c: &mut Col, base: &str, resolved: &Map<String, Value>, s: &WidgetSetting, instance: &Instance, manifest: &WidgetManifest) {
    let key_of = |k: &str| format!("{base}/settings/{}", token(k));
    match s {
        WidgetSetting::Boolean { key: k, label: l, .. } => {
            let on = resolved.get(k).and_then(Value::as_bool).unwrap_or(false);
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            label(b, row, l, "", None);
            b.switch(key(("ws", k.as_str())), Point::new(c.x + c.w - b.px(36.0), c.y + b.px(8.0)), on, l, Action::Toggle(Edit::new(DocId::Widgets, key_of(k), l.as_str()), on));
            c.y += b.px(40.0);
        }
        WidgetSetting::Number { key: k, label: l, min: None, .. } => {
            let v = resolved.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            label(b, row, l, "", None);
            let w = b.px(120.0);
            stepper(b, Rect::new(c.x + c.w - w, c.y + b.px(2.0), w, b.px(32.0)), "", v as u32, Action::Set(Edit::new(DocId::Widgets, key_of(k), l.as_str()), json!(v - 1.0)), Action::Set(Edit::new(DocId::Widgets, key_of(k), l.as_str()), json!(v + 1.0)));
            c.y += b.px(40.0);
        }
        WidgetSetting::Number { key: k, label: l, min, max, step, .. } => {
            let (min, max) = (min.unwrap_or(0.0), max.unwrap_or(100.0));
            slider(b, c, resolved, k, l, (min, max, step.unwrap_or(1.0)), "", key_of(k));
        }
        WidgetSetting::Slider { key: k, label: l, min, max, step, unit, .. } => {
            slider(b, c, resolved, k, l, (*min, *max, step.unwrap_or(1.0)), unit.as_deref().unwrap_or(""), key_of(k));
        }
        WidgetSetting::Select { key: k, label: l, options, .. } => {
            let current = resolved.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            let opts: Vec<(String, String)> = options.iter().map(option).collect();
            let edit = Edit::new(DocId::Widgets, key_of(k), l.as_str());
            let variant = opts.len() >= 3 && matches!(k.as_str(), "variant" | "face" | "layout" | "style" | "look" | "shape" | "display" | "frame");
            if variant {
                let items: Vec<(&str, bool, Action)> = opts.iter().map(|(v, name)| (name.as_str(), *v == current, Action::Set(edit.clone(), json!(v)))).collect();
                crate::ui::tiles(b, c, key(("variants", k.as_str())), 3, false, &items, |b, i, pic| variant_tile(b, instance, manifest, k, &opts[i].0, pic));
            } else if opts.len() <= 3 && opts.iter().map(|o| o.1.len()).sum::<usize>() < 26 {
                let labels: Vec<&str> = opts.iter().map(|o| o.1.as_str()).collect();
                let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
                label(b, row, l, "", None);
                let style = b.font(Font::Strong).scaled(12.5 / 13.0);
                let w: f32 = labels.iter().map(|s| b.measure(s, &style).0 + b.px(20.0)).sum::<f32>() + b.px(8.0);
                let at = opts.iter().position(|o| o.0 == current);
                b.segmented(key(("ws", k.as_str())), Rect::new(c.x + c.w - w, c.y + b.px(3.0), w, b.px(30.0)), &labels, at, l, false, |i| Action::Set(edit.clone(), json!(opts[i].0)));
                c.y += b.px(40.0);
            } else {
                let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
                label(b, row, l, "", None);
                let shown = opts.iter().find(|o| o.0 == current).map_or(current.clone(), |o| o.1.clone());
                let w = b.select_width(&shown).max(b.px(110.0));
                b.select(key(("ws", k.as_str())), Rect::new(c.x + c.w - w, c.y + b.px(3.0), w, b.px(30.0)), &shown, l, Action::Choose(Choices {
                    edit,
                    options: opts.iter().map(|(v, n)| (json!(v), n.clone())).collect(),
                    current: Some(json!(current)),
                }));
                c.y += b.px(40.0);
            }
        }
        WidgetSetting::String { key: k, label: l, .. } | WidgetSetting::Font { key: k, label: l, .. } => {
            let caption = b.font(Font::Body);
            b.text_at(c.x, c.y, l, &caption, b.p.ink1);
            let current = resolved.get(k).and_then(Value::as_str).unwrap_or("");
            text_field(app, b, Rect::new(c.x, c.y + b.px(24.0), c.w, b.px(32.0)), FieldId::Text(DocId::Widgets, key_of(k).into()), None, "", current, l);
            c.y += b.px(64.0);
        }
        WidgetSetting::Color { key: k, label: l, .. } => {
            let caption = b.font(Font::Body);
            b.text_at(c.x, c.y, l, &caption, b.p.ink1);
            c.y += b.px(28.0);
            let edit = Edit::new(DocId::Widgets, key_of(k), l.as_str());
            swatches(app, b, c, &edit, resolved.get(k), None, &[[0xff, 0xff, 0xff], [0x5b, 0x8d, 0xef], [0x7f, 0xae, 0x8e], [0xc8, 0x8d, 0x3c], [0xc9, 0x6b, 0x73]]);
        }
        WidgetSetting::File { key: k, label: l, .. } | WidgetSetting::Image { key: k, label: l, .. } => {
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            let current = resolved.get(k).and_then(Value::as_str).unwrap_or("");
            let shown = if current.is_empty() { "None" } else { current.rsplit(['/', '\\']).next().unwrap_or(current) };
            label(b, Rect::new(c.x, c.y, c.w - b.px(120.0), b.px(36.0)), l, shown, None);
            let w = b.button_width("Choose", false, true);
            b.button(key(("pick", k.as_str())), Rect::new(row.right - w, c.y + b.px(4.0), w, b.px(28.0)), "Choose", None, ButtonKind::Secondary, true, Action::PickFile("image".into(), key_of(k)));
            c.y += b.px(44.0);
        }
        WidgetSetting::List { key: k, label: l, fields, max_items, .. } => {
            let caption = b.font(Font::Body);
            b.text_at(c.x, c.y, l, &caption, b.p.ink1);
            c.y += b.px(26.0);
            let items = resolved.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
            let first = fields.first().map(list_key).unwrap_or_default();
            let list = Rect::new(c.x, c.y, c.w, items.len() as f32 * b.px(36.0));
            b.fill(list, b.px(8.0), b.p.chrome);
            for (n, item) in items.iter().enumerate() {
                let text = item.get(&first).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).unwrap_or_default();
                let r = Rect::new(c.x, c.y + n as f32 * b.px(36.0), c.w, b.px(36.0));
                crate::ui::list_row(b, r, key(("list", k.as_str(), n)), None, &text, Action::None, Some(Action::ListRemove(key_of(k), n)), n + 1 == items.len());
            }
            b.stroke(list, b.px(8.0), 1.0, b.p.line);
            c.y = list.bottom + b.px(8.0);
            let full = max_items.is_some_and(|m| items.len() >= m);
            let field_w = c.w - b.px(80.0);
            text_field(app, b, Rect::new(c.x, c.y, field_w, b.px(32.0)), FieldId::Text(DocId::Studio, format!("/list_new{}", key_of(k)).into()), None, &format!("Add {}", fields.first().map_or("an item".into(), |f| list_label(f).to_lowercase())), "", "New item");
            b.button(key(("list-add", k.as_str())), Rect::new(c.x + field_w + b.px(8.0), c.y, b.px(72.0), b.px(32.0)), "Add", None, ButtonKind::Secondary, !full, Action::ListAdd(key_of(k)));
            c.y += b.px(40.0);
        }
        WidgetSetting::Place { key: k, .. } => place(app, b, c, resolved.get(k), &key_of(k)),
    }
}

#[allow(clippy::too_many_arguments)]
fn slider(b: &mut B, c: &mut Col, resolved: &Map<String, Value>, k: &str, l: &str, range: (f64, f64, f64), unit: &str, path: String) {
    let v = resolved.get(k).and_then(Value::as_f64).unwrap_or(range.0);
    let lw = b.px(100.0);
    label(b, Rect::new(c.x, c.y, lw, b.px(40.0)), l, "", None);
    let text = format!("{}{}{unit}", trim(v), if unit.is_empty() { "" } else { " " });
    b.slider(key(("ws", k)), Rect::new(c.x + lw, c.y, c.w - lw, b.px(40.0)), v, range, &text, b.px(52.0), l, Action::Slider(Edit::new(DocId::Widgets, path, l)));
    c.y += b.px(40.0);
}

fn trim(v: f64) -> String {
    if v.fract().abs() < 1e-6 { format!("{v:.0}") } else { format!("{v:.1}") }
}

/// A − value + stepper.
fn stepper(b: &mut B, rect: Rect, name: &str, value: u32, minus: Action, plus: Action) {
    let p = *b.p;
    b.fill(rect, b.px(6.0), p.panel);
    b.stroke(rect, b.px(6.0), 1.0, p.control);
    let caption = b.font(Font::Caption);
    let strong = b.font(Font::Strong);
    let mut x = rect.left + b.px(10.0);
    if !name.is_empty() {
        let (w, _) = b.measure(name, &caption);
        b.text_in(Rect::new(x, rect.top, w + b.px(2.0), rect.height()), name, &caption, p.ink3, Align::Start);
        x += w + b.px(10.0);
    }
    b.text_in(Rect::new(x, rect.top, b.px(30.0), rect.height()), &value.to_string(), &strong, p.ink1, Align::Start);
    let s = b.px(26.0);
    b.icon_button(key(("minus", name, rect.left as i32)), Rect::new(rect.right - s * 2.0 - b.px(4.0), rect.top + (rect.height() - s) / 2.0, s, s), '\u{E738}', &format!("Less {name}"), false, true, minus);
    b.icon_button(key(("plus", name, rect.left as i32)), Rect::new(rect.right - s - b.px(2.0), rect.top + (rect.height() - s) / 2.0, s, s), glyph::ADD, &format!("More {name}"), false, true, plus);
}

/// The place setting: automatic or a town, with search.
fn place(app: &App, b: &mut B, c: &mut Col, value: Option<&Value>, path: &str) {
    let p = *b.p;
    let town = value.and_then(|v| v.get("name")).and_then(Value::as_str);
    b.segmented(key(("place-mode", path)), Rect::new(c.x, c.y, c.w, b.px(32.0)), &["Automatic", "A town"], Some(usize::from(town.is_some())), "Location", true, |i| if i == 0 { Action::PlaceAuto } else { Action::Field(FieldId::PlaceSearch) });
    c.y += b.px(40.0);
    if let Some(name) = town {
        let card = Rect::new(c.x, c.y, c.w, b.px(50.0));
        b.fill(card, b.px(8.0), p.panel);
        b.stroke(card, b.px(8.0), 1.0, p.edge);
        b.icon(Rect::new(card.left + b.px(10.0), card.top, b.px(18.0), card.height()), glyph::PLACE, 14.0, p.ink2);
        let strong = b.font(Font::Strong);
        let caption = b.font(Font::Caption);
        b.text_at(card.left + b.px(38.0), card.top + b.px(8.0), name, &strong, p.ink1);
        let region = [value.and_then(|v| v.get("region")), value.and_then(|v| v.get("country"))]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        b.text_at(card.left + b.px(38.0), card.top + b.px(27.0), &region, &caption, p.ink3);
        c.y += b.px(58.0);
    }
    text_field(app, b, Rect::new(c.x, c.y, c.w, b.px(32.0)), FieldId::PlaceSearch, Some(glyph::SEARCH), if town.is_some() { "Find another town" } else { "Find a town" }, "", "Find a town");
    c.y += b.px(38.0);
    for (i, found) in app.places.iter().take(5).enumerate() {
        let name = found.get("name").and_then(Value::as_str).unwrap_or("");
        let detail = [found.get("admin1"), found.get("country")].into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
        crate::ui::nav_row(b, c, glyph::PLACE, name, &detail, Action::PlacePick(i));
    }
    if !app.text(&FieldId::PlaceSearch).is_empty() && app.places.is_empty() {
        let caption = b.font(Font::Caption);
        b.text_at(c.x, c.y, "No towns found yet.", &caption, p.ink3);
        c.y += b.px(22.0);
    }
}

/// A variant tile: the widget's own view with that variant, at tile size.
fn variant_tile(b: &mut B, instance: &Instance, manifest: &WidgetManifest, setting: &str, value: &str, pic: Rect) {
    b.fill(pic, b.px(8.0), mypaper_ui::Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0));
    let mut settings = instance.settings.clone();
    settings.insert(setting.to_owned(), json!(value));
    let placed = mypaper_widgets::layout::Placed {
        instance: format!("{}-tile", instance.instance),
        widget: instance.widget.clone(),
        cell: [0, 0],
        size: instance.size,
        settings: manifest.resolve_settings(&settings),
        appearance: mypaper_widgets::look::json_of(&mypaper_widgets::look::defaults()),
        own: Map::new(),
    };
    let mut storage = mypaper_widgets::storage::Storage::new(std::env::temp_dir().join("mypaper-studio-tiles"));
    let now = mypaper_shell::preview::widgets::Stamp::now();
    let Ok(model) = mypaper_shell::preview::widgets::Model::new(placed.clone(), &mut storage, now) else { return };
    let tokens = mypaper_shell::preview::widgets::tokens(&placed.appearance);
    let inner = pic.inflate(-b.px(6.0));
    let aspect = instance.size[0] as f32 / instance.size[1].max(1) as f32;
    let (w, h) = if inner.width() / inner.height() > aspect { (inner.height() * aspect, inner.height()) } else { (inner.width(), inner.width() / aspect) };
    let scale = w / (instance.size[0] as f32 * 100.0);
    let list = mypaper_shell::preview::widgets::view(&model, &tokens, mypaper_shell::preview::widgets::Frame {
        size: (w.max(1.0) as u32, h.max(1.0) as u32),
        scale,
        now,
        instant: std::time::Instant::now(),
        editing: false,
    }, b.text);
    let (x, y) = (inner.left + (inner.width() - w) / 2.0, inner.top + (inner.height() - h) / 2.0);
    b.fill(Rect::new(x, y, w, h), 22.0 * scale, mypaper_ui::Rgba::rgb8([0x34, 0x3a, 0x42], 0.6));
    b.list.ops.extend(crate::stage::offset(&list.ops, x.round(), y.round()));
}

/// The instance's index in the file.
fn index_of(app: &App, instance: &str) -> Option<usize> {
    layout(app).instances.iter().position(|i| i.instance == instance)
}

/// Whether `cell`+`size` is free on this display, apart from `except`.
fn free(app: &App, layout: &Layout, cell: [u32; 2], size: [u32; 2], except: &str) -> bool {
    let grid = app.stage.borrow().content.grid.map(|g| g.0);
    if let Some(g) = grid
        && (cell[0] + size[0] > g.cols || cell[1] + size[1] > g.rows)
    {
        return false;
    }
    here(app, layout).iter().filter(|i| i.instance != except).all(|i| {
        cell[0] + size[0] <= i.cell[0] || i.cell[0] + i.size[0] <= cell[0] || cell[1] + size[1] <= i.cell[1] || i.cell[1] + i.size[1] <= cell[1]
    })
}

/// The first free cell for `size` on this display, row by row.
fn first_free(app: &App, layout: &Layout, size: [u32; 2]) -> Option<[u32; 2]> {
    let (cols, rows) = app.stage.borrow().content.grid.map_or((24, 12), |(g, _)| (g.cols, g.rows));
    (0..rows.saturating_sub(size[1] - 1))
        .flat_map(|r| (0..cols.saturating_sub(size[0] - 1)).map(move |c| [c, r]))
        .find(|&cell| free(app, layout, cell, size, ""))
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::SelectWidget(id) => {
            app.go(Page::Widgets);
            app.nav.widget = id;
        }
        Action::AddWidgetSheet => {
            app.go(Page::Widgets);
            app.texts.remove(&FieldId::WidgetSearch);
            app.overlay = Some(Overlay::AddWidget);
        }
        Action::AddWidget(id) => add(app, &id),
        Action::DuplicateWidget => {
            let Some(id) = app.nav.widget.clone() else { return };
            let l = layout(app);
            let Some(i) = l.get(&id).cloned() else { return };
            match first_free(app, &l, i.size) {
                Some(cell) => {
                    let mut copy = serde_json::to_value(&i).unwrap_or_default();
                    copy["instance"] = json!(l.new_id(&i.widget));
                    copy["cell"] = json!(cell);
                    let new_id = copy["instance"].as_str().unwrap_or_default().to_owned();
                    app.apply("Duplicate widget", vec![Op { doc: DocId::Widgets, path: format!("/instances/{}", l.instances.len()), value: Some(copy) }]);
                    app.nav.widget = Some(new_id);
                }
                None => app.alert(format!("No room for another on Display {}", app.nav.display + 1)),
            }
        }
        Action::RemoveWidget => {
            let Some(id) = app.nav.widget.take() else { return };
            if let Some(n) = index_of(app, &id) {
                let name = app.registry.get(&layout(app).instances[n].widget).map_or("Widget".into(), |m| m.manifest.name.clone());
                app.apply("Remove widget", vec![Op { doc: DocId::Widgets, path: format!("/instances/{n}"), value: None }]);
                app.toast_undo(format!("Removed {name}"));
            }
        }
        Action::ResizeWidget(dw, dh) => nudge(app, 0, 0, dw, dh),
        Action::ArrangeOnDesktop => arrange(),
        Action::ListAdd(path) => {
            let new_id = FieldId::Text(DocId::Studio, format!("/list_new{path}").into());
            let text = app.text(&new_id).trim().to_owned();
            if text.is_empty() {
                app.focus_field(new_id);
                return;
            }
            let (instance, k) = split_setting(&path);
            let l = layout(app);
            let Some(i) = l.instances.get(instance) else { return };
            let Some(manifest) = app.registry.get(&i.widget).map(|m| m.manifest.clone()) else { return };
            let resolved = manifest.resolve_settings(&i.settings);
            let mut items = resolved.get(&k).and_then(Value::as_array).cloned().unwrap_or_default();
            let first = manifest.settings.iter().find_map(|s| match s {
                WidgetSetting::List { key, fields, .. } if *key == k => fields.first().map(list_key),
                _ => None,
            });
            if let Some(first) = first {
                items.push(json!({ first: text }));
                app.apply("Add item", vec![Op { doc: DocId::Widgets, path, value: Some(Value::Array(items)) }]);
                app.texts.remove(&new_id);
            }
        }
        Action::ListRemove(path, n) => {
            let (instance, k) = split_setting(&path);
            let l = layout(app);
            let Some(i) = l.instances.get(instance) else { return };
            let Some(manifest) = app.registry.get(&i.widget).map(|m| m.manifest.clone()) else { return };
            let mut items = manifest.resolve_settings(&i.settings).get(&k).and_then(Value::as_array).cloned().unwrap_or_default();
            if n < items.len() {
                items.remove(n);
                app.apply("Remove item", vec![Op { doc: DocId::Widgets, path, value: Some(Value::Array(items)) }]);
            }
        }
        Action::PlacePick(n) => {
            let Some(found) = app.places.get(n).cloned() else { return };
            let Some(path) = place_path(app) else { return };
            let value = json!({
                "name": found.get("name").cloned().unwrap_or(Value::Null),
                "lat": found.get("lat").cloned().unwrap_or(Value::Null),
                "lon": found.get("lon").cloned().unwrap_or(Value::Null),
                "region": found.get("admin1").cloned().unwrap_or(Value::Null),
                "country": found.get("country").cloned().unwrap_or(Value::Null),
            });
            let value: Map<String, Value> = value.as_object().cloned().unwrap_or_default().into_iter().filter(|(_, v)| !v.is_null()).collect();
            app.apply("Location", vec![Op { doc: DocId::Widgets, path, value: Some(Value::Object(value)) }]);
            app.places.clear();
            app.texts.remove(&FieldId::PlaceSearch);
            app.blur_field();
        }
        Action::PlaceAuto => {
            if let Some(path) = place_path(app) {
                app.apply("Location", vec![Op { doc: DocId::Widgets, path, value: Some(Value::Null) }]);
            }
        }
        Action::PickFile(kind, path) => {
            let (folder, filters) = if kind == "cursors" { (true, &[][..]) } else { (false, crate::dialogs::PICTURES) };
            if let Some(file) = crate::dialogs::pick(app.window, if folder { "Choose a folder of pointers" } else { "Choose a picture" }, folder, filters) {
                let doc = if kind == "cursors" { DocId::Config } else { DocId::Widgets };
                app.apply("Picture", vec![Op { doc, path, value: Some(json!(file.display().to_string())) }]);
            }
        }
        _ => {}
    }
}

/// A list column's key and label (the manifest keeps them per variant).
fn list_key(f: &mypaper_widgets::manifest::ListField) -> String {
    serde_json::to_value(f).ok().and_then(|v| v.get("key")?.as_str().map(str::to_owned)).unwrap_or_default()
}

fn list_label(f: &mypaper_widgets::manifest::ListField) -> String {
    serde_json::to_value(f).ok().and_then(|v| v.get("label")?.as_str().map(str::to_owned)).unwrap_or_default()
}

/// `/instances/<n>/settings/<key>` → `(n, key)`.
fn split_setting(path: &str) -> (usize, String) {
    let parts: Vec<&str> = path.split('/').collect();
    let n = parts.get(2).and_then(|n| n.parse().ok()).unwrap_or(usize::MAX);
    let k = parts.get(4).map(|k| k.replace("~1", "/").replace("~0", "~")).unwrap_or_default();
    (n, k)
}

/// The selected widget's place setting.
fn place_path(app: &App) -> Option<String> {
    let id = app.nav.widget.clone()?;
    let l = layout(app);
    let n = l.instances.iter().position(|i| i.instance == id)?;
    let manifest = &app.registry.get(&l.instances[n].widget)?.manifest;
    let k = manifest.settings.iter().find_map(|s| match s {
        WidgetSetting::Place { key, .. } => Some(key.clone()),
        _ => None,
    })?;
    Some(format!("/instances/{n}/settings/{}", token(&k)))
}

fn add(app: &mut App, id: &str) {
    let Some(manifest) = app.registry.get(id).map(|m| m.manifest.clone()) else { return };
    app.go(Page::Widgets);
    let l = layout(app);
    let size = manifest.size.default;
    match first_free(app, &l, size) {
        Some(cell) => {
            let instance = l.new_id(id);
            let value = json!({ "instance": instance, "widget": id, "monitor": device(app), "cell": cell, "size": size });
            app.apply(&format!("Add {}", manifest.name), vec![Op { doc: DocId::Widgets, path: format!("/instances/{}", l.instances.len()), value: Some(value) }]);
            app.overlay = None;
            app.blur_field();
            app.nav.widget = Some(instance);
            app.toast_undo(format!("Added {}", manifest.name));
        }
        None => {
            app.overlay = None;
            let largest = here(app, &l).into_iter().max_by_key(|i| i.size[0] * i.size[1]).map(|i| i.instance);
            app.toast(format!("No room for {} on Display {}.", manifest.name, app.nav.display + 1), largest.map(|id| ("Make room".to_owned(), Action::SelectWidget(Some(id)))));
        }
    }
}

/// Moves (dx, dy cells) or resizes (dw, dh) the selected widget, if the new place is free.
fn nudge(app: &mut App, dx: i32, dy: i32, dw: i32, dh: i32) {
    let Some(id) = app.nav.widget.clone() else { return };
    let l = layout(app);
    let Some(n) = l.instances.iter().position(|i| i.instance == id) else { return };
    let i = &l.instances[n];
    let Some(manifest) = app.registry.get(&i.widget).map(|m| m.manifest.clone()) else { return };
    let cell = [(i.cell[0] as i32 + dx).max(0) as u32, (i.cell[1] as i32 + dy).max(0) as u32];
    let size = manifest.clamp_size([(i.size[0] as i32 + dw).max(1) as u32, (i.size[1] as i32 + dh).max(1) as u32]);
    if (cell, size) == (i.cell, i.size) || !free(app, &l, cell, size, &id) {
        return;
    }
    let label = if dw != 0 || dh != 0 { "Resize widget" } else { "Move widget" };
    app.apply(label, vec![
        Op { doc: DocId::Widgets, path: format!("/instances/{n}/cell"), value: Some(json!(cell)) },
        Op { doc: DocId::Widgets, path: format!("/instances/{n}/size"), value: Some(json!(size)) },
    ]);
}

/// Arrows move the selected widget a cell, Shift+arrows resize it; Delete removes it.
pub fn on_key(app: &mut App, vk: u16, shift: bool) -> bool {
    if app.nav.widget.is_none() || app.field.is_some() || app.overlay.is_some() {
        return false;
    }
    // Only while the canvas has the focus (a stage item), not the inspector's controls.
    let on_stage = app.input.focus.is_some_and(|k| app.frame.targets.iter().any(|t| t.key == k && matches!(t.action, Action::SelectWidget(Some(_)))));
    if !on_stage {
        return false;
    }
    let (dx, dy) = match vk {
        0x25 => (-1, 0),
        0x27 => (1, 0),
        0x26 => (0, -1),
        0x28 => (0, 1),
        0x2E => {
            app.run(Action::RemoveWidget);
            return true;
        }
        _ => return false,
    };
    if shift { nudge(app, 0, 0, dx, dy) } else { nudge(app, dx, dy, 0, 0) }
    true
}

pub fn drag_to(app: &mut App, drag: Drag, p: Point) {
    let Drag::Widget { instance, from, resize, .. } = drag else { return };
    let Some((grid, scale)) = app.stage.borrow().content.grid else { return };
    let (cw, ch) = ((grid.cell[0] + grid.gap) as f32 * scale, (grid.cell[1] + grid.gap) as f32 * scale);
    let (dx, dy) = (((p.x - from.x) / cw).round() as i32, ((p.y - from.y) / ch).round() as i32);
    if dx == 0 && dy == 0 {
        return;
    }
    app.nav.widget = Some(instance.clone());
    let before = layout(app).get(&instance).map(|i| (i.cell, i.size));
    if resize { nudge(app, 0, 0, dx, dy) } else { nudge(app, dx, dy, 0, 0) }
    let after = layout(app).get(&instance).map(|i| (i.cell, i.size));
    // The drag continues from where it took effect, so each cell is one step.
    if before != after
        && let Some(Drag::Widget { from: f, moved, .. }) = &mut app.drag
    {
        f.x += dx as f32 * cw;
        f.y += dy as f32 * ch;
        *moved = true;
    }
}

pub fn drop_drag(app: &mut App, drag: Drag) {
    if let Drag::Widget { instance, moved: false, .. } = drag {
        app.nav.widget = Some(instance);
    }
    app.dirty = true;
}

/// Toggles the shell's widget edit mode (a registered message to its widget host).
fn arrange() {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW};
    use windows::core::{PCWSTR, w};
    // SAFETY: lookups and a post; a missing host does nothing.
    unsafe {
        let message = RegisterWindowMessageW(w!("mypaper.widgets.edit"));
        if let Ok(host) = FindWindowExW(Some(HWND_MESSAGE), None::<HWND>, w!("mypaper.widgets.host"), PCWSTR::null())
            && message != 0
        {
            let _ = PostMessageW(Some(host), message, WPARAM(2), LPARAM(0));
        }
    }
}

/// The Add widget sheet: search, then the catalogue by category.
pub fn add_sheet(app: &App, b: &mut B) {
    let p = *b.p;
    let rect = crate::view::sheet(app, b, "Add a widget", b.px(880.0), b.px(560.0));
    text_field(app, b, Rect::new(rect.left, rect.top, b.px(300.0), b.px(32.0)), FieldId::WidgetSearch, Some(glyph::SEARCH), "Search widgets", "", "Search widgets");
    let needle = app.text(&FieldId::WidgetSearch).trim().to_lowercase();
    let categories: [(&str, &[&str]); 5] = [
        ("Time", &["clock", "calendar", "countdown", "timer"]),
        ("Media", &["now-playing", "media-pill", "photo"]),
        ("Info", &["weather", "system-monitor", "rss", "crypto", "quote", "heatmap"]),
        ("Productivity", &["todo", "notes"]),
        ("Launch", &["launcher", "quick-toggles"]),
    ];
    let view = key("add-widget-list");
    let area = Rect::new(rect.left, rect.top + b.px(44.0), rect.width(), rect.height() - b.px(56.0));
    b.scroll(view, area, app.nav.scroll(view), Action::None, |b, top| {
        let mut y = top;
        let gap = b.px(12.0);
        let w = (area.width() - b.px(10.0) - gap * 2.0) / 3.0;
        let h = b.px(76.0);
        for (category, ids) in categories {
            let found: Vec<&WidgetManifest> = ids
                .iter()
                .filter_map(|id| app.registry.get(id).map(|m| &m.manifest))
                .filter(|m| needle.is_empty() || format!("{} {}", m.name, m.description.as_deref().unwrap_or("")).to_lowercase().contains(&needle))
                .collect();
            if found.is_empty() {
                continue;
            }
            let micro = b.font(Font::Micro);
            b.text_at(area.left, y + b.px(4.0), category, &micro, p.ink3);
            y += b.px(24.0);
            b.group(key(("catalog", category)), |b| {
                for (n, m) in found.iter().enumerate() {
                    if n > 0 && n % 3 == 0 {
                        y += h + gap;
                    }
                    let r = Rect::new(area.left + (n % 3) as f32 * (w + gap), y, w, h);
                    let k = key(("catalog-card", m.id.as_str()));
                    b.fill(r, b.px(10.0), if b.hovered(k) { p.hover } else { p.bg });
                    b.stroke(r, b.px(10.0), 1.0, if b.hovered(k) { p.control } else { p.edge });
                    let tile = Rect::new(r.left + b.px(14.0), r.top + b.px(14.0), b.px(32.0), b.px(32.0));
                    b.fill(tile, b.px(8.0), p.hover);
                    b.icon(tile, glyph_of(&m.id), 15.0, p.ink1);
                    let strong = b.font(Font::Strong);
                    let caption = b.font(Font::Caption);
                    b.text_in(Rect::new(tile.right + b.px(12.0), r.top + b.px(12.0), r.width() - b.px(72.0), b.px(18.0)), &m.name, &strong, p.ink1, Align::Start);
                    let lines = b.text.lines(m.description.as_deref().unwrap_or(""), &caption, r.width() - b.px(72.0), 2);
                    for (i, l) in lines.iter().take(2).enumerate() {
                        b.text_in(Rect::new(tile.right + b.px(12.0), r.top + b.px(32.0) + i as f32 * b.px(16.0), r.width() - b.px(72.0), b.px(16.0)), l, &caption, p.ink3, Align::Start);
                    }
                    b.hit(k, r, Role::Button, &format!("Add {}", m.name), Action::AddWidget(m.id.clone()), mypaper_ui::controls::Behavior::Click);
                    b.focus_ring(k, r, b.px(10.0));
                }
            });
            y += h + b.px(20.0);
        }
        y
    });
}
