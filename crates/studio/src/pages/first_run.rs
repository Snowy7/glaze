//! First run (D6): "Make this desktop yours" — four starters, each a whole desktop (a style, a
//! wallpaper, a bars preset and a few widgets), applied to the real desktop as you pick; then
//! the taskbar choice; then Start with Windows and the emergency keys. Every step can be
//! skipped, and each pick is one undo step. Also reachable later from the palette.

use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Rect, Rgba, Role};
use serde_json::{Value, json};

use crate::actions::Action;
use crate::app::{App, Overlay};
use crate::docs::{DocId, Op};
use crate::ui::B;

struct Starter {
    name: &'static str,
    line: &'static str,
    style: &'static str,
    wallpaper: &'static str,
    preset: &'static str,
    widgets: &'static [(&'static str, [u32; 2])],
}

const STARTERS: [Starter; 4] = [
    Starter { name: "Calm", line: "Liquid glass, a menu bar and a dock", style: "liquid", wallpaper: "rain-city", preset: "macos", widgets: &[("clock", [3, 2]), ("weather", [3, 2])] },
    Starter { name: "Familiar", line: "Fluent, the Windows 11 taskbar", style: "fluent", wallpaper: "spring-forest", preset: "windows11", widgets: &[("clock", [3, 2])] },
    Starter { name: "Bold", line: "One UI cards on islands", style: "oneui", wallpaper: "star-nest", preset: "islands", widgets: &[("clock", [3, 2]), ("system-monitor", [3, 2])] },
    Starter { name: "Quiet", line: "Mono, just a top bar", style: "mono", wallpaper: "snowfall", preset: "monochrome", widgets: &[] },
];

/// Share starter metadata and the native desktop drawing with the compact panel.
pub(crate) fn panel_starters() -> Vec<(&'static str, &'static str)> { STARTERS.iter().map(|s| (s.name, s.line)).collect() }
pub(crate) fn panel_starter_tile(app: &App, b: &mut B, index: usize, rect: Rect) {
    if let Some(starter) = STARTERS.get(index) { starter_tile(app, b, starter, rect, b.p.panel); }
}

/// Read-only starter data for the panel's software desktop specimens.
pub(crate) fn panel_starter(index: usize) -> Option<(&'static str, &'static str, &'static [(&'static str, [u32; 2])])> {
    STARTERS.get(index).map(|s| (s.wallpaper, s.preset, s.widgets))
}

pub fn view(app: &App, b: &mut B) {
    let p = *b.p;
    let Some(Overlay::FirstRun { step, starter, taskbar }) = app.overlay else { return };
    let (w, h) = (app.size.0 as f32, app.size.1 as f32);
    let content_w = b.px(1080.0).min(w - b.px(64.0));
    let x = (w - content_w) / 2.0;
    let mut y = h / 2.0 - b.px(210.0);
    // Progress: three dashes and "1 of 3".
    let dash = b.px(22.0);
    let caption = b.font(Font::Caption);
    let total = dash * 3.0 + b.px(8.0) * 2.0 + b.px(60.0);
    let mut dx = (w - total) / 2.0;
    for i in 0..3u8 {
        b.fill(Rect::new(dx, y, dash, b.px(3.0)), 2.0, if i <= step { p.ink1 } else { p.selected });
        dx += dash + b.px(8.0);
    }
    b.text_at(dx + b.px(6.0), y - b.px(7.0), &format!("{} of 3", step + 1), &caption, p.ink3);
    y += b.px(28.0);
    let display = b.font(Font::Display);
    let body = b.font(Font::Body);
    let (title, line) = match step {
        0 => ("Make this desktop yours", "Pick a starting point. It shows on your desktop as you choose, and you can change everything later."),
        1 => ("Your taskbar", "mypaper's bars can replace it, sit beside it, or stay out of the way."),
        _ => ("You're set", "mypaper is running. Studio is where everything changes."),
    };
    b.text_in(Rect::new(0.0, y, w, b.px(40.0)), title, &display, p.ink1, Align::Center);
    y += b.px(48.0);
    b.text_in(Rect::new(0.0, y, w, b.px(20.0)), line, &body, p.ink3, Align::Center);
    y += b.px(42.0);
    match step {
        0 => {
            let gap = b.px(14.0);
            let tw = (content_w - gap * 3.0) / 4.0;
            let th = tw * 10.0 / 16.0;
            b.group(key("starters"), |b| {
                for (i, s) in STARTERS.iter().enumerate() {
                    let pic = Rect::new(x + i as f32 * (tw + gap), y, tw, th);
                    let k = key(("starter", i));
                    let on = starter == i;
                    let pad = b.px(6.0);
                    b.plate(k, Rect::new(pic.left - pad, pic.top - pad, tw + pad * 2.0, th + b.px(54.0) + pad * 2.0), b.px(10.0) + pad, on);
                    // What the corners are trimmed to: the plate when there is one.
                    let behind = if on {
                        mypaper_ui::theme::over(p.selected, p.bg)
                    } else if b.hovered(k) {
                        mypaper_ui::theme::over(p.hover, p.bg)
                    } else {
                        p.bg
                    };
                    starter_tile(app, b, s, pic, behind);
                    let strong = b.font(Font::Heading);
                    b.text_at(pic.left, pic.bottom + b.px(14.0), s.name, &strong, p.ink1);
                    b.text_at(pic.left, pic.bottom + b.px(36.0), s.line, &caption, p.ink3);
                    b.hit_checked(k, Rect::new(pic.left, pic.top, tw, th + b.px(56.0)), Role::RadioButton, &format!("{}: {}", s.name, s.line), Some(on), Action::FirstRunPick(i), mypaper_ui::controls::Behavior::Click);
                    b.focus_ring(k, pic, b.px(10.0));
                }
            });
            y += th + b.px(76.0);
        }
        1 => {
            let options = [
                ("Replace it with mypaper's bars", "The Windows taskbar hides; your bars take its place"),
                ("Keep Windows' taskbar", "Add widgets only"),
                ("Hide everything", "Just the wallpaper"),
            ];
            let ow = b.px(520.0).min(content_w);
            let ox = (w - ow) / 2.0;
            b.group(key("taskbar-choice"), |b| {
                for (i, (title, line)) in options.iter().enumerate() {
                    let r = Rect::new(ox, y, ow, b.px(64.0));
                    let k = key(("taskbar-choice", i));
                    let on = taskbar == i;
                    b.fill(r, b.px(12.0), p.panel);
                    b.wash(k, r, b.px(12.0));
                    if on {
                        b.fill(r, b.px(12.0), p.selected);
                        b.check_mark(Rect::new(r.left, r.top + b.px(10.0), r.width(), r.height()));
                    }
                    let strong = b.font(Font::Strong);
                    b.text_at(r.left + b.px(18.0), r.top + b.px(13.0), title, &strong, p.ink1);
                    b.text_at(r.left + b.px(18.0), r.top + b.px(34.0), line, &caption, p.ink3);
                    b.hit_checked(k, r, Role::RadioButton, title, Some(on), Action::FirstRunTaskbar(i), mypaper_ui::controls::Behavior::Click);
                    b.focus_ring(k, r, b.px(12.0));
                    y += b.px(74.0);
                }
            });
            y += b.px(20.0);
        }
        _ => {
            let ow = b.px(520.0).min(content_w);
            let ox = (w - ow) / 2.0;
            let r = Rect::new(ox, y, ow, b.px(64.0));
            b.fill(r, b.px(12.0), p.panel);
            let strong = b.font(Font::Strong);
            b.text_at(r.left + b.px(18.0), r.top + b.px(13.0), "Start with Windows", &strong, p.ink1);
            b.text_at(r.left + b.px(18.0), r.top + b.px(34.0), "mypaper starts when you sign in", &caption, p.ink3);
            let on = app.system.autostart.as_ref().is_some_and(|a| a.enabled);
            b.switch(key("first-autostart"), mypaper_ui::Point::new(r.right - b.px(54.0), r.top + b.px(22.0)), on, "Start with Windows", Action::ToggleAutostart(!on));
            y += b.px(100.0);
        }
    }
    // The footer: Skip, the safety line, Continue.
    let fy = y + b.px(12.0);
    let skip = b.button_width("Skip", false, false);
    b.button(key("first-skip"), Rect::new(x, fy, skip, b.px(32.0)), "Skip", None, ButtonKind::Ghost, true, Action::FirstRunSkip);
    let label = if step == 2 { "Open Studio" } else { "Continue" };
    let cw = b.button_width(label, false, false);
    b.button(key("first-next"), Rect::new(x + content_w - cw, fy, cw, b.px(32.0)), label, None, ButtonKind::Primary, true, Action::FirstRunNext);
    let safety = "If anything goes wrong,";
    let tail = "puts Windows back.";
    let (sw, _) = b.measure(safety, &caption);
    let (tw2, _) = b.measure(tail, &caption);
    let keys_w = b.px(200.0);
    let total = sw + keys_w + tw2 + b.px(16.0);
    let mut kx = (w - total) / 2.0;
    b.text_at(kx, fy + b.px(8.0), safety, &caption, p.ink3);
    kx += sw + b.px(8.0);
    kx += b.keys(kx, fy + b.px(6.0), &["Ctrl", "Alt", "Shift", "Backspace"]) + b.px(8.0);
    b.text_at(kx, fy + b.px(8.0), tail, &caption, p.ink3);
}

/// A starter's mini desktop: its wallpaper, its bars' silhouette and a clock card. `behind` is
/// the colour around the tile, which trims the bars' square ends at the rounded corners.
fn starter_tile(app: &App, b: &mut B, s: &Starter, pic: Rect, behind: Rgba) {
    let still = app.library.items.iter().find(|w| w.id == s.wallpaper).and_then(|w| w.preview.clone().or_else(|| w.video.clone()));
    let radius = b.px(10.0);
    match still.and_then(|path| app.images.borrow_mut().get(&path, (pic.width() as u32 * 2, pic.height() as u32 * 2))) {
        Some(image) => crate::stage::image_round(b, pic, &image, 1.0, radius),
        None => b.fill(pic, radius, Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0)),
    }
    let (fill, rim, weight) = match s.style {
        "liquid" => (Rgba::new(1.0, 1.0, 1.0, 0.1), Rgba::new(1.0, 1.0, 1.0, 0.45), 300),
        "oneui" => (Rgba::rgb8([40, 44, 56], 0.62), Rgba::TRANSPARENT, 700),
        "fluent" => (Rgba::rgb8([36, 36, 40], 0.78), Rgba::new(1.0, 1.0, 1.0, 0.1), 400),
        _ => (Rgba::new(0.0, 0.0, 0.0, 0.62), Rgba::new(1.0, 1.0, 1.0, 0.16), 400),
    };
    let (w, h) = (pic.width(), pic.height());
    let mut glass = |r: Rect, radius: f32| {
        b.fill(r, radius, fill);
        if rim.a > 0.0 {
            b.stroke(r, radius, 1.0, rim);
        }
    };
    match s.preset {
        "windows11" => glass(Rect::new(pic.left, pic.bottom - h * 0.11, w, h * 0.11), 0.0),
        _ => {
            glass(Rect::new(pic.left + w * 0.04, pic.top + h * 0.05, w * 0.92, h * 0.08), 4.0);
            if s.preset != "monochrome" {
                glass(Rect::new(pic.left + w * 0.28, pic.bottom - h * 0.14, w * 0.44, h * 0.1), 5.0);
            }
        }
    }
    let card = Rect::new(pic.left + w * 0.06, pic.top + h * 0.2, w * 0.28, h * 0.24);
    glass(card, 6.0);
    let mut font = b.font(Font::Body).scaled(15.0 / 14.0);
    font.weight = weight;
    b.text_in(Rect::new(card.left + b.px(8.0), card.top, card.width(), card.height() * 0.75), "10:42", &font, Rgba::new(1.0, 1.0, 1.0, 1.0), Align::Start);
    // A full-width bar reaches the corners: trim it, from a pixel inside the edge (no seam).
    let t = b.px(3.0);
    b.stroke(pic.inflate(t - 0.5), radius + t - 0.5, t * 2.0 + 1.0, behind);
}

pub fn run(app: &mut App, action: Action) {
    if app.panel.is_some() {
        crate::panel::run_first_run(app, action);
        return;
    }
    let Some(Overlay::FirstRun { step, starter, taskbar }) = app.overlay.clone() else {
        if let Action::FirstRunPick(_) = action {
            app.overlay = Some(Overlay::FirstRun { step: 0, starter: 0, taskbar: 0 });
        }
        return;
    };
    match action {
        Action::FirstRunPick(i) if i < STARTERS.len() => {
            app.overlay = Some(Overlay::FirstRun { step, starter: i, taskbar });
            apply(app, &STARTERS[i]);
        }
        Action::FirstRunTaskbar(i) => {
            app.overlay = Some(Overlay::FirstRun { step, starter, taskbar: i });
            let ops = match i {
                0 => vec![Op { doc: DocId::Bars, path: "/taskbar/mode".into(), value: Some(json!("hide")) }],
                1 => vec![
                    Op { doc: DocId::Bars, path: "/taskbar/mode".into(), value: Some(json!("keep")) },
                    Op { doc: DocId::Bars, path: "/top/enabled".into(), value: Some(json!(false)) },
                    Op { doc: DocId::Bars, path: "/dock/enabled".into(), value: Some(json!(false)) },
                ],
                _ => vec![
                    Op { doc: DocId::Bars, path: "/taskbar/mode".into(), value: Some(json!("hide")) },
                    Op { doc: DocId::Bars, path: "/top/enabled".into(), value: Some(json!(false)) },
                    Op { doc: DocId::Bars, path: "/dock/enabled".into(), value: Some(json!(false)) },
                    Op { doc: DocId::Widgets, path: "/instances".into(), value: Some(json!([])) },
                ],
            };
            app.apply("Your taskbar", ops);
        }
        Action::FirstRunNext => {
            if step == 0 && !app.docs.can_undo() {
                apply(app, &STARTERS[starter]);
            }
            if step >= 2 {
                finish(app);
            } else {
                app.overlay = Some(Overlay::FirstRun { step: step + 1, starter, taskbar });
            }
        }
        Action::FirstRunSkip => finish(app),
        _ => {}
    }
}

fn finish(app: &mut App) {
    app.overlay = None;
    app.apply("First run", vec![Op { doc: DocId::Studio, path: "/first_run_done".into(), value: Some(Value::Bool(true)) }]);
}

/// Applies a starter to the real desktop: one undoable step.
fn apply(app: &mut App, s: &Starter) {
    let monitor = app.display().map_or_else(String::new, |m| m.device.clone());
    let widgets: Vec<Value> = {
        let mut row = 0u32;
        s.widgets
            .iter()
            .enumerate()
            .map(|(i, (id, size))| {
                let v = json!({ "instance": format!("{id}-{}", i + 1), "widget": id, "monitor": monitor, "cell": [20, row], "size": size });
                row += size[1];
                v
            })
            .collect()
    };
    let theme = app.library.items.iter().find(|w| w.id == s.wallpaper).map(|w| w.apply_as.clone());
    let mut ops = vec![
        Op { doc: DocId::Config, path: "/look/style".into(), value: Some(json!(s.style)) },
        Op { doc: DocId::Bars, path: "/preset".into(), value: Some(json!(s.preset)) },
        Op { doc: DocId::Widgets, path: "/instances".into(), value: Some(Value::Array(widgets)) },
    ];
    for bar in ["top", "dock"] {
        for field in ["design", "enabled"] {
            ops.push(Op { doc: DocId::Bars, path: format!("/{bar}/{field}"), value: None });
        }
    }
    if s.preset == "macos" || s.preset == "islands" {
        ops.push(Op { doc: DocId::Bars, path: "/dock/enabled".into(), value: Some(json!(true)) });
    }
    if let Some(theme) = theme {
        ops.push(Op { doc: DocId::Config, path: "/theme".into(), value: Some(json!(theme)) });
    }
    app.apply(&format!("{} starter", s.name), ops);
}
