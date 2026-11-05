//! The command palette (Ctrl K): one field over every setting, wallpaper, widget, integration,
//! action and place. Enter goes there (a setting opens its page and More, scrolls to it and
//! flashes it); Space flips a switch in place. Results are grouped Settings, Actions,
//! Wallpapers, Widgets, Integrations, Go to; with nothing typed, recent ones come first.

use mypaper_ui::controls::{Align, Font, key};
use mypaper_ui::{Rect, Role};
use serde_json::{Value, json};

use crate::actions::{Action, Edit, FieldId, Page, Section};
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op};
use crate::settings::{Kind, SETTINGS, Setting};
use crate::theme::glyph;
use crate::ui::{B, text_field};

/// One result.
#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub section: String,
    pub glyph: char,
    pub group: &'static str,
    pub action: Action,
    /// A switch setting: Space flips it, and the row shows its state.
    pub toggle: Option<&'static Setting>,
    /// What it's remembered as in Recent.
    pub id: String,
}

const GROUPS: [&str; 6] = ["Settings", "Actions", "Wallpapers", "Widgets", "Integrations", "Go to"];
const MAX: usize = 9;

fn all(app: &App) -> Vec<Entry> {
    let mut out = Vec::new();
    for s in SETTINGS {
        out.push(Entry {
            title: s.label.into(),
            section: s.place.crumb().into(),
            glyph: setting_glyph(s),
            group: "Settings",
            action: Action::PaletteRun(usize::MAX),
            toggle: matches!(s.kind, Kind::Switch { .. }).then_some(s),
            id: format!("setting:{}", s.id),
        });
    }
    let paused = app.get(DocId::Config, "/paused").and_then(Value::as_bool).unwrap_or(false);
    for (title, g, action) in [
        (if paused { "Resume the wallpaper" } else { "Pause the wallpaper" }, if paused { glyph::PLAY } else { glyph::PAUSE }, Action::Pause),
        ("Peek at the desktop", glyph::EYE, Action::Peek),
        ("Arrange widgets on the desktop", glyph::MOVE, Action::ArrangeOnDesktop),
        ("Add a widget", glyph::ADD, Action::AddWidgetSheet),
        ("Add a wallpaper", glyph::ADD, Action::AddWallpaper),
        ("Restart the shell", glyph::RESET, Action::Restart("shell")),
        ("Restart the engine", glyph::RESET, Action::Restart("engine")),
        ("Restore the Windows desktop", glyph::ALERT, Action::RestoreDesktop),
        ("Start from a preset", glyph::STAR, Action::FirstRunPick(usize::MAX)),
        ("Copy diagnostics", glyph::COPY, Action::CopyDiagnostics),
        ("Undo", glyph::UNDO, Action::Undo),
        ("Redo", glyph::REDO, Action::Redo),
    ] {
        out.push(Entry {
            title: title.into(),
            section: String::new(),
            glyph: g,
            group: "Actions",
            action,
            toggle: None,
            id: format!("action:{title}"),
        });
    }
    for (i, w) in app.library.items.iter().enumerate() {
        out.push(Entry {
            title: w.name.clone(),
            section: w.category().into(),
            glyph: glyph::WALLPAPERS,
            group: "Wallpapers",
            action: Action::UseWallpaper(i),
            toggle: None,
            id: format!("wallpaper:{}", w.apply_as),
        });
    }
    for (id, installed) in &app.registry.widgets {
        out.push(Entry {
            title: format!("{} widget", installed.manifest.name),
            section: "Add to the desktop".into(),
            glyph: crate::pages::widgets::glyph_of(id),
            group: "Widgets",
            action: Action::AddWidget(id.clone()),
            toggle: None,
            id: format!("widget:{id}"),
        });
    }
    for (i, integration) in crate::pages::integrations::LIST.iter().enumerate() {
        out.push(Entry {
            title: integration.name.into(),
            section: integration.adds.into(),
            glyph: integration.glyph,
            group: "Integrations",
            action: Action::SelectIntegration(i),
            toggle: None,
            id: format!("integration:{}", integration.name),
        });
    }
    for page in Page::ALL {
        out.push(Entry {
            title: page.title().into(),
            section: "Page".into(),
            glyph: page.glyph(),
            group: "Go to",
            action: Action::Nav(page),
            toggle: None,
            id: format!("page:{}", page.key()),
        });
    }
    for section in Section::ALL {
        out.push(Entry {
            title: format!("Settings › {}", section.title()),
            section: String::new(),
            glyph: section.glyph(),
            group: "Go to",
            action: Action::Section(section),
            toggle: None,
            id: format!("section:{}", section.title()),
        });
    }
    out
}

fn setting_glyph(s: &Setting) -> char {
    match s.place {
        crate::settings::Place::Look { windows: true } => glyph::APP,
        crate::settings::Place::Look { .. } => glyph::LOOK,
        crate::settings::Place::Bars(_) | crate::settings::Place::BarsPanel(_) => glyph::BARS,
        crate::settings::Place::Settings(section) => section.glyph(),
        crate::settings::Place::Page(page) => page.glyph(),
    }
}

/// How well `entry` matches `query` (higher is better), or `None`.
fn score(entry: &Entry, keywords: &str, query: &str) -> Option<u32> {
    let title = entry.title.to_lowercase();
    if title.starts_with(query) {
        return Some(300);
    }
    if title.split([' ', '›']).any(|w| w.starts_with(query)) {
        return Some(200);
    }
    if title.contains(query) {
        return Some(150);
    }
    let rest = format!("{} {}", entry.section.to_lowercase(), keywords);
    if rest.split(' ').any(|w| w.starts_with(query)) {
        return Some(100);
    }
    rest.contains(query).then_some(50)
}

/// The results for what's typed, in display order.
pub fn results(app: &App) -> Vec<Entry> {
    let query = app.text(&FieldId::Palette).trim().to_lowercase();
    let entries = all(app);
    if query.is_empty() {
        let recent: Vec<String> = app
            .get(DocId::Studio, "/recent")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let mut out: Vec<Entry> = recent
            .iter()
            .filter_map(|id| entries.iter().find(|e| &e.id == id).cloned())
            .map(|mut e| {
                e.group = "Recent";
                e
            })
            .take(5)
            .collect();
        out.extend(entries.into_iter().filter(|e| e.group == "Go to").take(MAX));
        return out;
    }
    // Every word must match somewhere.
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(usize, u32, Entry)> = entries
        .into_iter()
        .filter_map(|e| {
            let keywords = e.toggle.map_or("", |s| s.keywords).to_owned()
                + SETTINGS
                    .iter()
                    .find(|s| e.id == format!("setting:{}", s.id))
                    .map_or("", |s| s.keywords);
            let mut total = 0;
            for w in &words {
                total += score(&e, &keywords, w)?;
            }
            Some((GROUPS.iter().position(|g| *g == e.group).unwrap_or(9), total, e))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    // A few per group, so one kind doesn't push out the rest.
    let mut out = Vec::new();
    let mut per_group = std::collections::HashMap::new();
    for (_, _, e) in scored {
        let n = per_group.entry(e.group).or_insert(0);
        if *n < 4 {
            *n += 1;
            out.push(e);
        }
    }
    out.truncate(12);
    out
}

/// Runs result `i` (Enter), or flips it (`Space` on a switch).
pub fn run(app: &mut App, i: usize) {
    let list = results(app);
    let Some(entry) = list.get(i).cloned() else {
        return;
    };
    remember(app, &entry.id);
    app.blur_field();
    app.overlay = None;
    if let Some(id) = entry.id.strip_prefix("setting:") {
        if let Some(s) = Setting::get(id) {
            app.reveal(s);
        }
        return;
    }
    if entry.action == Action::FirstRunPick(usize::MAX) {
        app.overlay = Some(Overlay::FirstRun {
            step: 0,
            starter: 0,
            taskbar: 0,
        });
        return;
    }
    app.run(entry.action);
}

/// Space on a switch result: flip it, keep the palette open.
pub fn toggle(app: &mut App, i: usize) -> bool {
    let Some(s) = results(app).get(i).and_then(|e| e.toggle) else {
        return false;
    };
    let on = app.value(s).as_bool().unwrap_or(false);
    app.set(&Edit::new(s.doc, s.path, s.label), json!(!on));
    true
}

fn remember(app: &mut App, id: &str) {
    let mut recent: Vec<Value> = app
        .get(DocId::Studio, "/recent")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    recent.retain(|v| v.as_str() != Some(id));
    recent.insert(0, json!(id));
    recent.truncate(8);
    app.apply("Recent", vec![Op {
        doc: DocId::Studio,
        path: "/recent".into(),
        value: Some(Value::Array(recent)),
    }]);
}

pub fn view(app: &App, b: &mut B, selected: usize) {
    let p = *b.p;
    let full = Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32);
    b.scrim(key("palette-scrim"), full, Action::Dismiss);
    let list = results(app);
    let row = b.px(36.0);
    let groups = {
        let mut g: Vec<&str> = list.iter().map(|e| e.group).collect();
        g.dedup();
        g.len()
    };
    let body_h = if list.is_empty() { b.px(64.0) } else { list.len() as f32 * row + groups as f32 * b.px(26.0) + b.px(8.0) };
    let w = b.px(640.0).min(app.size.0 as f32 - b.px(48.0));
    let h = b.px(52.0) + body_h + b.px(36.0);
    let regions = crate::view::regions(app);
    let rect = Rect::new(regions.main.left + (regions.main.width() - w) / 2.0 - b.px(40.0), b.px(132.0), w, h);
    let rect = Rect::new(rect.left.max(b.px(24.0)), rect.top, w, h);
    b.elevated(rect, b.px(12.0));
    b.hit(key("palette-body"), rect, Role::Dialog, "Search everything", Action::None, mypaper_ui::controls::Behavior::Click);
    // The input.
    let input = Rect::new(rect.left + b.px(8.0), rect.top + b.px(10.0), rect.width() - b.px(16.0), b.px(32.0));
    b.icon(Rect::new(rect.left + b.px(16.0), rect.top, b.px(18.0), b.px(52.0)), glyph::SEARCH, 15.0, p.ink3);
    // The field draws without its own frame: the palette is the frame.
    let field = Rect::new(rect.left + b.px(40.0), rect.top + b.px(16.0), rect.width() - b.px(56.0), b.px(20.0));
    let _ = input;
    text_field_bare(app, b, field);
    b.hline(rect.left, rect.right, rect.top + b.px(52.0), p.line);
    // Results.
    let mut y = rect.top + b.px(52.0) + b.px(4.0);
    if list.is_empty() {
        let caption = b.font(Font::Caption);
        b.text_in(Rect::new(rect.left + b.px(16.0), y, rect.width() - b.px(32.0), b.px(56.0)), "No matches. Try a setting's name, a wallpaper or a widget.", &caption, p.ink3, Align::Start);
        y += b.px(60.0);
    }
    let mut last_group = "";
    b.group(key("palette-results"), |b| {
        for (i, e) in list.iter().enumerate() {
            if e.group != last_group {
                last_group = e.group;
                let micro = b.font(Font::Micro);
                b.text_at(rect.left + b.px(16.0), y + b.px(10.0), e.group, &micro, p.ink3);
                y += b.px(26.0);
            }
            let r = Rect::new(rect.left + b.px(6.0), y, rect.width() - b.px(12.0), row);
            let k = key(("palette", i));
            if i == selected {
                b.fill(r, b.px(7.0), p.selected);
            } else if b.hovered(k) {
                b.fill(r, b.px(7.0), p.hover);
            }
            b.icon(Rect::new(r.left + b.px(10.0), r.top, b.px(16.0), row), e.glyph, 14.0, p.ink3);
            let body = b.font(Font::Body);
            let caption = b.font(Font::Caption);
            let section_w = b.measure(&e.section, &caption).0.min(r.width() * 0.45);
            b.text_in(Rect::new(r.left + b.px(38.0), r.top, r.width() - b.px(56.0) - section_w, row), &e.title, &body, p.ink1, Align::Start);
            let mut right = r.right - b.px(10.0);
            if let Some(s) = e.toggle {
                let on = app.value(s).as_bool().unwrap_or(false);
                let state = if on { "On" } else { "Off" };
                let (sw, _) = b.measure(state, &caption);
                b.text_in(Rect::new(right - sw, r.top, sw + b.px(2.0), row), state, &caption, p.ink2, Align::End);
                right -= sw + b.px(12.0);
            }
            b.text_in(Rect::new(right - section_w, r.top, section_w, row), &e.section, &caption, p.ink3, Align::End);
            b.hit(k, r, Role::ListItem, &format!("{} {}", e.title, e.section), Action::PaletteRun(i), mypaper_ui::controls::Behavior::Click);
            y += row;
        }
    });
    // Footer keycaps.
    let foot = Rect::new(rect.left, rect.bottom - b.px(36.0), rect.width(), b.px(36.0));
    b.hline(foot.left, foot.right, foot.top, p.line);
    let caption = b.font(Font::Caption);
    let mut x = foot.left + b.px(16.0);
    for (keys, label) in [(&["↑", "↓"][..], "Move"), (&["Enter"][..], "Open"), (&["Space"][..], "Toggle"), (&["Esc"][..], "Close")] {
        x += b.keys(x, foot.top + b.px(8.0), keys) + b.px(6.0);
        let (lw, _) = b.measure(label, &caption);
        b.text_in(Rect::new(x, foot.top, lw + b.px(2.0), foot.height()), label, &caption, p.ink3, Align::Start);
        x += lw + b.px(16.0);
    }
}

/// The palette's field: no frame of its own, `title-2`-ish text.
fn text_field_bare(app: &App, b: &mut B, rect: Rect) {
    let id = FieldId::Palette;
    let live = app.field.as_ref().filter(|f| f.id == id);
    match live {
        Some(f) => {
            f.field.set_rect(rect);
            b.list.ops.push(mypaper_ui::Op::TextField {
                rect,
                field: f.field.clone(),
                revision: f.field.revision(),
            });
        }
        None => {
            let shown = app.text(&id);
            let style = b.font(Font::Title2).scaled(1.0);
            let (text, color) = if shown.is_empty() { ("Search settings, wallpapers, widgets…", b.p.ink4) } else { (shown, b.p.ink1) };
            b.text_in(rect, text, &style, color, Align::Start);
        }
    }
    b.hit(crate::app::field_key(&id), rect, Role::SearchInput, "Search", Action::Field(id), mypaper_ui::controls::Behavior::Field);
    let _ = text_field;
}

/// Up and Down in the palette.
pub fn step(app: &mut App, delta: i32) {
    let n = results(app).len().max(1) as i32;
    if let Some(Overlay::Palette { selected }) = &mut app.overlay {
        *selected = (*selected as i32 + delta).rem_euclid(n) as usize;
    }
    app.dirty = true;
}

