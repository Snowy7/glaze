//! Onboarding: welcome, taskbar, starter look, top bar, dock, wallpaper, extras, done. Every pick
//! applies to the real desktop at once (and undoes); everything after the taskbar is optional,
//! and Skip keeps whatever is already set.

use crate::panel::tokens::radius;
use super::{
    controls::{button, group, notice, row, text, title},
    editors::toggle,
};
use crate::{
    actions::Action,
    app::{App, Overlay},
    theme::glyph,
    ui::{B, Col},
};
use mypaper_ui::controls::{ButtonKind, Glyph, key};
use mypaper_ui::{Rect, controls::Align};

pub const WELCOME: u8 = 0;
pub const TASKBAR: u8 = 1;
pub const LOOK: u8 = 2;
pub const TOP: u8 = 3;
pub const DOCK: u8 = 4;
pub const WALLPAPER: u8 = 5;
pub const EXTRAS: u8 = 6;
pub const DONE: u8 = 7;

/// Whether `step` applies to this taskbar choice: bar steps need mypaper's bars, and the look
/// step needs bars or widgets to style.
fn shown(step: u8, taskbar: usize) -> bool {
    match step {
        LOOK => taskbar != 2,
        TOP | DOCK => taskbar == 0,
        _ => true,
    }
}

/// The steps after the welcome, in order, for this taskbar choice.
fn steps(taskbar: usize) -> Vec<u8> {
    (TASKBAR..=DONE).filter(|&step| shown(step, taskbar)).collect()
}

#[derive(Debug, Clone)]
pub struct Baseline {
    docs: [serde_json::Value; 3],
    applied: bool,
}

/// Capture once when this first-run session opens; every choice starts here.
pub fn begin(app: &App) {
    let mut baseline = app.panel.as_ref().unwrap().first_run.borrow_mut();
    baseline.get_or_insert_with(|| Baseline {
        docs: [
            crate::docs::DocId::Config,
            crate::docs::DocId::Bars,
            crate::docs::DocId::Widgets,
        ]
        .map(|doc| app.docs.doc(doc).raw.clone()),
        applied: false,
    });
}

/// The step before `step`, skipping steps that don't apply.
fn previous(step: u8, taskbar: usize) -> u8 {
    (WELCOME..step).rev().find(|&s| s == WELCOME || shown(s, taskbar)).unwrap_or(WELCOME)
}

fn following(step: u8, taskbar: usize) -> u8 {
    (step + 1..=DONE).find(|&s| shown(s, taskbar)).unwrap_or(DONE)
}

pub fn back(app: &mut App) {
    if let Some(Overlay::FirstRun { step, taskbar, .. }) = &mut app.overlay {
        *step = previous(*step, *taskbar);
    }
}

fn finish(app: &mut App) {
    use crate::docs::{DocId, Op};
    app.overlay = None;
    app.panel.as_ref().unwrap().first_run.borrow_mut().take();
    app.apply(
        "First run",
        vec![Op {
            doc: DocId::Studio,
            path: "/first_run_done".into(),
            value: Some(serde_json::json!(true)),
        }],
    );
}

pub fn run(app: &mut App, action: Action) {
    let Some(Overlay::FirstRun {
        step,
        starter,
        taskbar,
    }) = app.overlay
    else {
        app.panel.as_ref().unwrap().first_run.borrow_mut().take();
        app.overlay = Some(Overlay::FirstRun {
            step: WELCOME,
            starter: None,
            taskbar: 0,
        });
        begin(app);
        return;
    };
    begin(app);
    let apply = match action {
        Action::FirstRunPick(index) if crate::editing::first_run::panel_starter(index).is_some() => {
            app.overlay = Some(Overlay::FirstRun {
                step,
                starter: Some(index),
                taskbar,
            });
            Some((Some(index), taskbar))
        }
        Action::FirstRunTaskbar(choice) if choice < 3 => {
            app.overlay = Some(Overlay::FirstRun {
                step,
                starter,
                taskbar: choice,
            });
            Some((starter, choice))
        }
        Action::FirstRunNext if step >= DONE => return finish(app),
        Action::FirstRunNext => {
            app.overlay = Some(Overlay::FirstRun {
                step: following(step, taskbar),
                starter,
                taskbar,
            });
            // The taskbar is the one choice that always applies: leaving its step unpicked
            // applies the highlighted one. Look and wallpaper change nothing unless picked.
            let applied = app.panel.as_ref().unwrap().first_run.borrow().as_ref().unwrap().applied;
            (step == TASKBAR && !applied).then_some((starter, taskbar))
        }
        Action::FirstRunSkip => return finish(app),
        _ => None,
    };
    if let Some((starter, taskbar)) = apply {
        write_choice(app, starter, taskbar);
    }
}

/// Writes the taskbar choice and, if one is picked, the starter's style, bars and widgets. Every
/// write starts from the documents as onboarding found them, so picks never pile up.
fn write_choice(app: &mut App, starter: Option<usize>, taskbar: usize) {
    use crate::docs::{DocId, Op};
    use serde_json::json;
    let mut docs = app.panel.as_ref().unwrap().first_run.borrow().as_ref().unwrap().docs.clone();
    let op = |doc, path: String, value| Op { doc, path, value };
    let mut edits = vec![op(
        DocId::Bars,
        "/taskbar/mode".into(),
        Some(json!(if taskbar == 1 { "keep" } else { "hide" })),
    )];
    let picked = starter.and_then(|index| Some((index, crate::editing::first_run::panel_starter(index)?)));
    if let Some((index, (preset, widgets))) = picked {
        edits.push(op(DocId::Config, "/look/style".into(), Some(json!(["liquid", "fluent", "oneui", "mono"][index]))));
        edits.push(op(DocId::Bars, "/preset".into(), Some(json!(preset))));
        for bar in ["top", "dock"] {
            edits.push(op(DocId::Bars, format!("/{bar}/design"), None));
        }
        if taskbar != 2 {
            let monitor = app.display().map_or("", |display| display.device.as_str());
            let mut row = 0;
            let instances: Vec<_> = widgets
                .iter()
                .enumerate()
                .map(|(i, (widget, size))| {
                    let instance = json!({"instance":format!("{widget}-{}", i+1),"widget":widget,"monitor":monitor,"cell":[20,row],"size":size});
                    row += size[1];
                    instance
                })
                .collect();
            edits.push(op(DocId::Widgets, "/instances".into(), Some(json!(instances))));
        }
    }
    if taskbar == 2 {
        edits.push(op(DocId::Widgets, "/instances".into(), Some(json!([]))));
    }
    let preset = picked.map(|(_, (preset, _))| preset);
    for bar in ["top", "dock"] {
        let enabled = if taskbar != 0 {
            Some(json!(false))
        } else if bar == "dock" && matches!(preset, Some("macos" | "islands")) {
            Some(json!(true))
        } else {
            None
        };
        edits.push(op(DocId::Bars, format!("/{bar}/enabled"), enabled));
    }
    for edit in &edits {
        crate::docs::apply_op(
            &mut docs[match edit.doc {
                DocId::Config => 0,
                DocId::Bars => 1,
                _ => 2,
            }],
            edit,
        );
    }
    // Write only the fields onboarding controls, preserving unrelated settings changed elsewhere.
    let mut ops = Vec::new();
    for (doc, document, paths) in [
        (DocId::Config, &docs[0], &["/look/style"][..]),
        (
            DocId::Bars,
            &docs[1],
            &["/preset", "/taskbar/mode", "/top/design", "/top/enabled", "/dock/design", "/dock/enabled"][..],
        ),
        (DocId::Widgets, &docs[2], &["/instances"][..]),
    ] {
        ops.extend(paths.iter().map(|path| Op {
            doc,
            path: (*path).into(),
            value: document.pointer(path).cloned(),
        }));
    }
    app.apply("First-run choice", ops);
    app.panel.as_ref().unwrap().first_run.borrow_mut().as_mut().unwrap().applied = true;
}

fn contents(index: usize, taskbar: usize) -> String {
    let (_, widgets) = crate::editing::first_run::panel_starter(index).unwrap();
    let style = ["Liquid", "Fluent", "One UI", "Mono"][index];
    if taskbar == 2 {
        return style.into();
    }
    let bars = if taskbar == 1 {
        "Windows"
    } else {
        ["Dock + top", "Taskbar", "Islands", "Top bar"][index]
    };
    let count = widgets.len();
    format!(
        "{bars} · {count} {} · {style}",
        if count == 1 { "widget" } else { "widgets" }
    )
}

/// A starter's picture: its material, shown as one readable widget over your wallpaper.
fn starter_preview(app: &App, b: &mut B, index: usize, rect: Rect) {
    use mypaper_config::look::Style;
    let style = [Style::Liquid, Style::Fluent, Style::Oneui, Style::Mono][index.min(3)];
    super::specimens::material(app, b, rect, style);
}

fn current(app: &App) -> (u8, Option<usize>, usize) {
    match app.overlay {
        Some(Overlay::FirstRun {
            step,
            starter,
            taskbar,
        }) => (step, starter, taskbar),
        _ => (WELCOME, None, 0),
    }
}

/// The primary button's label: optional steps read "Skip" until something is picked.
pub fn next_label(app: &App) -> &'static str {
    match current(app) {
        (WELCOME, ..) => "Get started",
        (DONE, ..) => "Open Studio",
        (LOOK, None, _) => "Skip",
        _ => "Continue",
    }
}

/// Each step's heading and the sentence under it.
fn copy(step: u8) -> (&'static str, &'static str) {
    match step {
        WELCOME => (
            "Welcome to mypaper",
            "Live wallpapers, a top bar and dock, and widgets on glass. All native, all light.",
        ),
        TASKBAR => (
            "Your taskbar",
            "mypaper can replace Windows' taskbar with its own top bar and dock, or work \
             alongside it. You can switch any time.",
        ),
        LOOK => (
            "Pick a look",
            "Optional. A starting style for bars, menus and widgets. Skip it to keep things \
             as they are.",
        ),
        TOP => (
            "Your top bar",
            "Optional. Pick how the bar across the top of your screen looks, or turn it off. Its modules and panels are in Studio.",
        ),
        DOCK => (
            "Your dock",
            "Optional. Pick a dock, or turn it off. Pinned apps and magnification are in Studio.",
        ),
        EXTRAS => (
            "Extras",
            "Optional. mypaper does more than looks. Each of these is off until you turn it on, and every one turns off again in Studio.",
        ),
        WALLPAPER => (
            "Choose a wallpaper",
            "Optional. Live wallpapers pause when they're covered, so they cost nothing while \
             you work. Skip it to keep the one you have.",
        ),
        _ => (
            "You're all set",
            "Everything you picked is already on your desktop. Change any of it later in Studio.",
        ),
    }
}

/// A label on the left and its value on the right, for the summary. Not interactive.
fn fact(b: &mut B, c: &mut Col, icon: Glyph, label: &str, value: &str) {
    let h = super::controls::height(b, 44.0);
    b.icon(Rect::new(c.x, c.y, b.px(18.0), h), icon, 15.0, b.p.ink3);
    let x = c.x + b.px(28.0);
    let style = super::controls::font(b, 13.0, 500);
    let value_w = (b.measure(value, &style).0 + b.px(4.0)).min((c.right() - x) * 0.6);
    text(b, Rect::new(x, c.y, c.right() - x - value_w, h), label, 13.0, 400, b.p.ink2, Align::Start);
    text(b, Rect::new(c.right() - value_w, c.y, value_w, h), value, 13.0, 500, b.p.ink1, Align::End);
    c.y += h;
}

/// Shown wallpapers: the bundled ones that load, plus whatever is active.
fn wallpapers(app: &App) -> Vec<usize> {
    let items = &app.library.items;
    let mut shown: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].active || (!items[i].hidden && items[i].error.is_none()))
        .collect();
    // Your own active wallpaper leads; the bundled ones keep their curated order.
    shown.sort_by_key(|&i| {
        (
            !(items[i].active && items[i].editable()),
            items[i].origin != crate::library::Origin::Bundled,
        )
    });
    shown
}

fn photo(app: &App, b: &mut B, r: Rect, i: usize) {
    let item = &app.library.items[i];
    let image = item
        .preview
        .as_ref()
        .or(item.video.as_ref())
        .and_then(|path| app.images.borrow_mut().get(path, (r.width().ceil() as u32, r.height().ceil() as u32)));
    match image {
        Some(image) => crate::stage::image_cover(b, r, &image, 1.0),
        None => b.icon(r, glyph::WALLPAPERS, 22.0, b.p.ink3),
    }
}

/// The step's choices: rows, starter looks, wallpapers or the summary.
fn choices(app: &App, b: &mut B, c: &mut Col) {
    let (step, starter, taskbar) = current(app);
    match step {
        WELCOME => {}
        TASKBAR => group(b, c, "", |b, c| {
            for (i, (icon, label, caption)) in [
                (glyph::BARS, "Use mypaper's bars", "A top bar and dock · Windows' taskbar hides"),
                (glyph::APP, "Keep Windows' taskbar", "Add widgets to your Windows desktop"),
                (glyph::WALLPAPERS, "Just the wallpaper", "No bars or widgets"),
            ]
            .into_iter()
            .enumerate()
            {
                row(b, c, Some(icon), label, "", caption, Some(taskbar == i), Action::FirstRunTaskbar(i));
            }
        }),
        LOOK => {
            let starters = crate::editing::first_run::panel_starters();
            let picks: Vec<_> = starters
                .iter()
                .enumerate()
                .map(|(i, (name, _))| (*name, starter == Some(i), Action::FirstRunPick(i)))
                .collect();
            let details: Vec<_> = (0..starters.len()).map(|index| contents(index, taskbar)).collect();
            super::choosers::tiles(app, b, c, key("panel-starters"), &picks, &details, |b, i, r| {
                starter_preview(app, b, i, r)
            });
        }
        WALLPAPER => {
            let shown = wallpapers(app);
            let picks: Vec<_> = shown
                .iter()
                .map(|&i| {
                    let item = &app.library.items[i];
                    (item.name.as_str(), item.active, Action::UseWallpaper(i))
                })
                .collect();
            super::choosers::draw(app, b, c, key("first-run-wallpapers"), 0.0, &picks, |b, index, r| {
                if let Some(&i) = index.and_then(|index| shown.get(index)) {
                    photo(app, b, r, i);
                }
            });
        }
        TOP | DOCK => {
            let (bar, id) = if step == TOP { (crate::actions::Bar::Top, "top_enabled") } else { (crate::actions::Bar::Dock, "dock_enabled") };
            group(b, c, "", |b, c| {
                let s = crate::settings::Setting::get(id).unwrap();
                super::controls::setting(app, b, c, id, s.label, "");
            });
            c.y += b.px(8.0);
            super::editors::bar_design_cards(app, b, c, Some(bar));
        }
        EXTRAS => extras(app, b, c),
        _ => summary(app, b, c, starter, taskbar),
    }
}

/// Windows preferences mypaper can manage. Off removes the setting, so Windows' own value
/// (journaled when mypaper took over) comes back.
const TWEAKS: [(&str, &str, &str); 3] = [
    ("show_extensions", "Show file extensions", "report.pdf, not just report"),
    ("show_hidden", "Show hidden files", "Leaves protected system files hidden"),
    ("game_mode", "Game Mode", "Windows puts the game first while you play"),
];

fn extras(app: &App, b: &mut B, c: &mut Col) {
    use crate::actions::Edit;
    use crate::docs::DocId;
    group(b, c, "Files and search", |b, c| {
        for id in ["experimental_file_index", "experimental_files_default"] {
            let s = crate::settings::Setting::get(id).unwrap();
            super::controls::setting(app, b, c, id, s.label, s.hint);
        }
    });
    group(b, c, "Desktop", |b, c| {
        for id in ["desktop_menu", "classic_menu"] {
            let s = crate::settings::Setting::get(id).unwrap();
            super::controls::setting(app, b, c, id, s.label, s.hint);
        }
    });
    group(b, c, "Windows", |b, c| {
        for (field, label, caption) in TWEAKS {
            let path = format!("/system/{field}");
            let on = app.get(DocId::Config, &path).and_then(serde_json::Value::as_bool) == Some(true);
            let edit = Edit::new(DocId::Config, path, label);
            let action = if on { Action::Unset(edit) } else { Action::Set(edit, serde_json::json!(true)) };
            toggle(b, c, label, caption, on, action);
        }
    });
}

/// How many extras are on, for the summary.
fn extras_on(app: &App) -> usize {
    [
        "/experimental/file_index",
        "/experimental/files_default",
        "/classic_context_menu",
        "/system/show_extensions",
        "/system/show_hidden",
        "/system/game_mode",
    ]
    .into_iter()
    .filter(|path| app.get(crate::docs::DocId::Config, path).and_then(serde_json::Value::as_bool) == Some(true))
    .count()
}

fn summary(app: &App, b: &mut B, c: &mut Col, starter: Option<usize>, taskbar: usize) {
    group(b, c, "Your desktop", |b, c| {
        let bars = match taskbar {
            1 => "Windows taskbar".to_owned(),
            2 => "None".to_owned(),
            _ => super::editors::bar_design_name(app, None),
        };
        let look = starter
            .and_then(|index| crate::editing::first_run::panel_starters().get(index).map(|(name, _)| *name))
            .unwrap_or("Unchanged");
        let wallpaper = app.library.active().map_or("None", |i| app.library.items[i].name.as_str());
        fact(b, c, glyph::BARS, "Bars", &bars);
        fact(b, c, glyph::LOOK, "Look", look);
        fact(b, c, glyph::WALLPAPERS, "Wallpaper", wallpaper);
        let extras = match extras_on(app) {
            0 => "None".to_owned(),
            n => format!("{n} on"),
        };
        fact(b, c, glyph::SETTINGS, "Extras", &extras);
    });
    group(b, c, "Startup", |b, c| {
        let on = app.system.autostart.as_ref().is_some_and(|a| a.enabled);
        toggle(b, c, "Start with Windows", "Your desktop is ready when you sign in", on, Action::ToggleAutostart(!on));
    });
    group(b, c, "If something goes wrong", |b, c| {
        let style = super::controls::font(b, 12.0, 500);
        let mut x = c.x;
        for label in ["Ctrl", "Alt", "Shift", "Backspace"] {
            let width = b.measure(label, &style).0 + b.px(16.0);
            if x + width > c.right() {
                x = c.x;
                c.y += b.px(32.0);
            }
            let r = Rect::new(x, c.y + b.px(4.0), width, b.px(26.0));
            b.fill(r, b.px(radius::SMALL), b.p.fill);
            b.stroke(r, b.px(radius::SMALL), b.px(1.0), b.p.edge);
            text(b, r, label, 12.0, 500, b.p.ink1, Align::Center);
            x += width + b.px(6.0);
        }
        c.y += b.px(36.0);
        notice(b, c, "Puts the Windows desktop back, from anywhere.");
    });
}

/// One step in the docked panel: Back, the heading and its sentence, then the choices.
pub fn view(app: &App, b: &mut B, c: &mut Col) {
    begin(app);
    let step = current(app).0;
    if step > WELCOME {
        let rect = Rect::new(c.x, c.y, b.button_width("Back", true, true).min(c.w), super::controls::height(b, 30.0));
        b.button(
            key("first-run-back"),
            rect,
            "Back",
            Some(glyph::CHEVRON_LEFT),
            ButtonKind::Ghost,
            true,
            Action::Panel(super::Command::FirstRunBack),
        );
        c.y = rect.bottom + b.px(4.0);
    }
    let (heading, sentence) = copy(step);
    title(b, c, heading, "");
    notice(b, c, sentence);
    choices(app, b, c);
}

// Full screen -------------------------------------------------------------------------------

/// How long the welcome takes to build itself, in seconds.
const INTRO: f32 = 3.4;

/// Seconds since the full-screen onboarding opened; infinite (everything shown) in fixtures.
fn elapsed(app: &App) -> f32 {
    app.panel
        .as_ref()
        .and_then(|state| state.intro)
        .map_or(f32::INFINITY, |at| at.elapsed().as_secs_f32())
}

/// 0 before `start`, 1 after `start + length`, eased out in between.
fn ease(t: f32, start: f32, length: f32) -> f32 {
    let x = ((t - start) / length).clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// The next frame the full-screen onboarding wants: only while the welcome builds itself.
/// Afterwards every screen is still, so onboarding costs nothing while you read.
pub fn deadline(app: &App) -> Option<std::time::Instant> {
    let t = elapsed(app);
    if !t.is_finite() || t > INTRO || current(app).0 != WELCOME || !matches!(app.overlay, Some(Overlay::FirstRun { .. })) {
        return None;
    }
    Some(std::time::Instant::now() + std::time::Duration::from_millis(16))
}

fn centered(b: &mut B, r: Rect, label: &str, size: f32, weight: u16, ink: mypaper_ui::Rgba) {
    text(b, r, label, size, weight, ink, Align::Center);
}

/// A feature tile on the welcome: a picture, a name and one line, fading in at `alpha`.
fn feature(app: &App, b: &mut B, r: Rect, alpha: f32, index: usize) {
    let pic = Rect::new(r.left, r.top, r.width(), r.width() * 10.0 / 16.0);
    let (name, line) = [
        ("Live wallpapers", "Shaders, video and 3D scenes that pause when covered"),
        ("Widgets on glass", "Clock, weather, music, calendar and more"),
        ("A top bar and dock", "Replace the taskbar, or keep Windows' own"),
    ][index];
    if alpha <= 0.0 {
        return;
    }
    // The picture wipes in from the top while its words fade: no group opacity needed.
    let start = b.list.ops.len();
    super::controls::card_picture(b, pic, false, |b, pic| match index {
        0 => {
            let shown = wallpapers(app);
            let pick = ["seascape", "milky-way", "star-nest"]
                .iter()
                .find_map(|id| shown.iter().copied().find(|&i| app.library.items[i].id == *id))
                .or(shown.first().copied());
            if let Some(i) = pick {
                photo(app, b, pic, i);
            }
        }
        1 => super::specimens::material(app, b, pic, mypaper_config::look::Style::Liquid),
        _ => bars_sketch(app, b, pic),
    });
    if alpha < 1.0 {
        let ops = b.list.ops.split_off(start);
        b.list.ops.push(mypaper_ui::Op::Clipped {
            rect: Rect::new(pic.left, pic.top, pic.width(), pic.height() * alpha),
            ops: ops.into(),
        });
    }
    let top = super::controls::card_label_top(b, pic) + b.px(4.0);
    let line_h = super::controls::height(b, 22.0);
    text(b, Rect::new(r.left, top, r.width(), line_h), name, 15.0, 600, b.p.ink1.with_alpha(alpha), Align::Start);
    text(b, Rect::new(r.left, top + line_h, r.width(), line_h), line, 13.0, 400, b.p.ink3.with_alpha(alpha), Align::Start);
}

/// A wallpaper with a plain top bar and dock over it: large, simple shapes, nothing tiny.
fn bars_sketch(app: &App, b: &mut B, pic: Rect) {
    let shown = wallpapers(app);
    if let Some(i) = ["rain-city", "snow-city"]
        .iter()
        .find_map(|id| shown.iter().copied().find(|&i| app.library.items[i].id == *id))
        .or(shown.first().copied())
    {
        photo(app, b, pic, i);
    }
    let glass = mypaper_ui::Rgba::new(0.06, 0.07, 0.10, 0.55);
    let bar = Rect::new(pic.left, pic.top, pic.width(), pic.height() * 0.09);
    b.fill(bar, 0.0, glass);
    let dot = bar.height() * 0.32;
    for (i, x) in [0.04, 0.88, 0.93].into_iter().enumerate() {
        let w = if i == 0 { dot * 3.0 } else { dot };
        b.fill(Rect::new(pic.left + pic.width() * x, bar.top + (bar.height() - dot) / 2.0, w, dot), dot / 2.0, mypaper_ui::Rgba::new(1.0, 1.0, 1.0, 0.8));
    }
    let tile = pic.height() * 0.11;
    let gap = tile * 0.35;
    let count = 6.0;
    let dock_w = count * tile + (count + 1.0) * gap;
    let dock = Rect::new(pic.left + (pic.width() - dock_w) / 2.0, pic.bottom - tile - gap * 3.0, dock_w, tile + gap * 2.0);
    b.fill(dock, dock.height() * 0.35, glass);
    for i in 0..count as usize {
        let r = Rect::new(dock.left + gap + i as f32 * (tile + gap), dock.top + gap, tile, tile);
        let shade = [0.95, 0.75, 0.85, 0.65, 0.9, 0.7][i];
        b.fill(r, tile * 0.25, mypaper_ui::Rgba::new(shade, shade, shade, 0.9));
    }
}

fn welcome(app: &App, b: &mut B, full: Rect, t: f32) {
    let width = (full.width() - b.px(96.0)).min(b.px(1040.0));
    let left = full.left + (full.width() - width) / 2.0;
    let mut y = full.top + (full.height() * 0.13).max(b.px(40.0));
    // The mark grows in, then the words rise, then the features, then the button.
    let a = ease(t, 0.5, 0.8);
    let size = b.px(76.0) * (0.85 + 0.15 * a);
    let mark = Rect::new(full.left + (full.width() - size) / 2.0, y + (b.px(76.0) - size) / 2.0, size, size);
    b.fill(mark, size * 0.28, b.p.solid.with_alpha(a));
    b.icon(mark, glyph::WALLPAPERS, 38.0 * size / b.px(76.0), b.p.on_solid.with_alpha(a));
    y += b.px(76.0 + 32.0);
    let (heading, sentence) = copy(WELCOME);
    let a = ease(t, 1.1, 0.8);
    let rise = b.px(14.0) * (1.0 - a);
    centered(b, Rect::new(left, y + rise, width, b.px(60.0)), heading, 46.0, 700, b.p.ink1.with_alpha(a));
    y += b.px(68.0);
    let a = ease(t, 1.4, 0.8);
    centered(b, Rect::new(left, y + rise, width, b.px(28.0)), sentence, 17.0, 400, b.p.ink2.with_alpha(a));
    y += b.px(28.0 + 52.0);
    let gap = b.px(24.0);
    let tile = (width - gap * 2.0) / 3.0;
    for i in 0..3 {
        let a = ease(t, 1.9 + i as f32 * 0.15, 0.7);
        let r = Rect::new(left + i as f32 * (tile + gap), y + b.px(18.0) * (1.0 - a), tile, tile * 10.0 / 16.0 + b.px(64.0));
        feature(app, b, r, a, i);
    }
    y += tile * 10.0 / 16.0 + b.px(64.0 + 44.0);
    if t >= 2.6 {
        let w = b.px(220.0);
        let h = b.px(46.0);
        let at = Rect::new(full.left + (full.width() - w) / 2.0, y.min(full.bottom - h - b.px(56.0)), w, h);
        button(b, at, next_label(app), None, true, true, Action::FirstRunNext);
        centered(b, Rect::new(left, at.bottom + b.px(10.0), width, b.px(22.0)), "Takes about a minute", 12.0, 400, b.p.ink3);
    }
}

/// Step dots: one per step after the welcome, filled up to the current one.
fn progress(b: &mut B, r: Rect, step: u8, taskbar: usize) {
    let all = steps(taskbar);
    let count = all.len();
    let step = all.iter().position(|&s| s == step).map_or(count, |i| i + 1);
    let (w, gap, h) = (b.px(28.0), b.px(6.0), b.px(4.0));
    let top = r.top + (r.height() - h) / 2.0;
    for i in 0..count {
        let on = i < step;
        b.fill(Rect::new(r.left + i as f32 * (w + gap), top, w, h), h / 2.0, if on { b.p.ink1 } else { b.p.selected });
    }
    let x = r.left + count as f32 * (w + gap) + b.px(6.0);
    text(b, Rect::new(x, r.top, r.right - x, r.height()), &format!("Step {step} of {count}"), 12.0, 500, b.p.ink3, Align::Start);
}

/// A step, the way Windows' own setup lays it out: the words on the left, the choices on the
/// right as One UI cards on the page, Back and Continue beneath. Narrow screens stack the two.
fn step_screen(app: &App, b: &mut B, full: Rect, step: u8) {
    let width = (full.width() - b.px(128.0)).min(b.px(1180.0));
    let height = (full.height() - b.px(200.0)).min(b.px(720.0));
    let left = full.left + (full.width() - width) / 2.0;
    let top = full.top + (full.height() - height) / 2.0 - b.px(24.0);
    let stacked = width < b.px(860.0);
    let copy_w = if stacked { width } else { width * 0.36 };
    progress(b, Rect::new(left, top, copy_w, b.px(20.0)), step, current(app).2);
    let (heading, sentence) = copy(step);
    let mut y = top + b.px(44.0);
    let title_style = super::controls::font(b, 40.0, 700);
    y += b.paragraph(left, y, copy_w, heading, &title_style, b.px(50.0), b.p.ink1) + b.px(14.0);
    let style = super::controls::font(b, 15.0, 400);
    y += b.paragraph(left, y, copy_w, sentence, &style, b.px(24.0), b.p.ink2) + b.px(16.0);
    let mut banners = Col { x: left, y, w: copy_w, view: key("first-run-feedback") };
    super::saving::write_banners(app, b, &mut banners);
    let pane = if stacked {
        Rect::new(left, banners.y + b.px(8.0), width, (top + height - banners.y - b.px(8.0)).max(b.px(120.0)))
    } else {
        let x = left + copy_w + b.px(56.0);
        Rect::new(x, top, left + width - x, height)
    };
    let view = key(("first-run-step", step));
    b.scroll(view, pane, app.nav.scroll(view), Action::None, |b, y| {
        // Leave room for selection rings and focus outlines inside the clip.
        let mut c = Col { x: pane.left + b.px(6.0), y: y + b.px(6.0), w: pane.width() - b.px(16.0), view };
        super::controls::save_banner(app, b, &mut c, super::Page::FirstRun);
        choices(app, b, &mut c);
        super::controls::boundaries(b);
        c.y + b.px(24.0)
    });
    let h = b.px(42.0);
    let y = pane.bottom + b.px(28.0);
    let label = next_label(app);
    let style = super::controls::font(b, 13.0, 500);
    let next_w = (b.measure(label, &style).0 + b.px(56.0)).max(b.px(156.0));
    let enabled = !super::controls::relevant_docs(super::Page::FirstRun)
        .iter()
        .any(|id| app.docs.doc(*id).issue.is_some());
    button(b, Rect::new(left + width - next_w, y, next_w, h), label, None, true, enabled, Action::FirstRunNext);
    let back = Rect::new(left - b.px(8.0), y, b.button_width("Back", true, true), h);
    b.button(
        key("first-run-back"),
        back,
        "Back",
        Some(glyph::CHEVRON_LEFT),
        ButtonKind::Ghost,
        true,
        Action::Panel(super::Command::FirstRunBack),
    );
}

/// The whole-screen onboarding: Skip, and the welcome or a step, in One UI: a near-black
/// page, large rounded cards and pill controls.
pub fn fullscreen(app: &App, b: &mut B, full: Rect) {
    begin(app);
    b.kit.radius.control = None;
    b.kit.radius.card = 24.0;
    let step = current(app).0;
    let t = elapsed(app);
    // Studio's own One UI, dark: the near-black the curtains fade to, cards and pills on it.
    b.fill(full, 0.0, b.p.chrome);
    if step != DONE && (step != WELCOME || t >= 2.6) {
        let width = b.button_width("Skip setup", false, true);
        let r = Rect::new(full.right - width - b.px(24.0), full.top + b.px(20.0), width, b.px(32.0));
        b.button(key("first-run-skip"), r, "Skip setup", None, ButtonKind::Ghost, true, Action::FirstRunSkip);
    }
    if step == WELCOME {
        welcome(app, b, full, t);
    } else {
        step_screen(app, b, full, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::DocId;

    #[test]
    fn taskbar_precedes_starters_and_every_pick_respects_that_choice() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State {
            page: super::super::Page::FirstRun,
            ..Default::default()
        });
        app.size = (480, 800);
        run(&mut app, Action::FirstRunPick(0));
        // The welcome applies nothing; Continue reaches the taskbar.
        let version = app.docs.version;
        run(&mut app, Action::FirstRunNext);
        assert_eq!(app.docs.version, version);
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(
            frame
                .targets
                .iter()
                .any(|target| target.action == Action::FirstRunTaskbar(0))
        );
        assert!(
            !frame
                .targets
                .iter()
                .any(|target| matches!(target.action, Action::FirstRunPick(_)))
        );
        run(&mut app, Action::FirstRunTaskbar(1));
        run(&mut app, Action::FirstRunNext);
        let frame = crate::panel::build(&app, &mut ui.text);
        assert_eq!(
            frame
                .targets
                .iter()
                .filter(|target| matches!(target.action, Action::FirstRunPick(_)))
                .count(),
            4
        );
        for starter in 0..4 {
            run(&mut app, Action::FirstRunPick(starter));
            assert_eq!(
                app.get(DocId::Bars, "/taskbar/mode"),
                Some(&serde_json::json!("keep"))
            );
            for bar in ["top", "dock"] {
                assert_eq!(
                    app.get(DocId::Bars, &format!("/{bar}/enabled")),
                    Some(&serde_json::json!(false))
                );
            }
        }
        run(&mut app, Action::FirstRunTaskbar(2));
        run(&mut app, Action::FirstRunPick(2));
        assert_eq!(
            app.get(DocId::Widgets, "/instances"),
            Some(&serde_json::json!([]))
        );
        assert_eq!(contents(2, 2), "One UI");
        run(&mut app, Action::FirstRunNext);
        let docs =
            [DocId::Config, DocId::Bars, DocId::Widgets].map(|doc| app.docs.doc(doc).raw.clone());
        // With no bars or widgets the look step is skipped both ways.
        for step in [TASKBAR, WELCOME] {
            let frame = crate::panel::build(&app, &mut ui.text);
            assert!(
                frame
                    .targets
                    .iter()
                    .any(|target| target.action
                        == Action::Panel(super::super::Command::FirstRunBack))
            );
            back(&mut app);
            assert!(
                matches!(app.overlay, Some(Overlay::FirstRun { step: current, .. }) if current == step)
            );
            assert_eq!(
                docs,
                [DocId::Config, DocId::Bars, DocId::Widgets].map(|doc| app
                    .docs
                    .doc(doc)
                    .raw
                    .clone())
            );
        }
    }
}
