//! Settings: a sub-list (General, Performance and power, Desktop, Keyboard, Troubleshoot,
//! About) and grouped rows, at most 720 wide. Performance is the home of the graphics card and
//! the frame rate, with a live line of what mypaper uses. Troubleshoot lists the parts with
//! Restart, the emergency stop as a button, Start over per file (keeping a backup), diagnostics
//! and logs.

use mypaper_config::desktop_menu::{Builtin, Entry, default_items};
use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Point, Rect, Role};
use serde_json::{Value, json};

use crate::actions::{Action, Edit, FieldId, Section};
use crate::app::App;
use crate::docs::{DocId, Op, StartedOver};
use crate::theme::glyph;
use crate::ui::{B, Col, label, setting};

pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let nav = Rect::new(body.left, body.top, b.px(232.0), body.height());
    let mut y = nav.top + b.px(16.0);
    b.group(key("settings-nav"), |b| {
        for section in Section::ALL {
            let r = Rect::new(nav.left + b.px(12.0), y, nav.width() - b.px(24.0), b.px(36.0));
            let k = key(("section", section.title()));
            let on = app.nav.section() == section;
            if on {
                b.fill(r, b.px(8.0), p.selected);
            } else {
                b.wash(k, r, b.px(8.0));
            }
            let ink = if on || b.hovered(k) { p.ink1 } else { p.ink2 };
            b.icon(Rect::new(r.left + b.px(10.0), r.top, b.px(16.0), r.height()), section.glyph(), 14.0, ink);
            let style = b.font(Font::Strong);
            b.text_in(Rect::new(r.left + b.px(38.0), r.top, r.width() - b.px(40.0), r.height()), section.title(), &style, ink, Align::Start);
            b.hit_checked(k, r, Role::Tab, section.title(), Some(on), Action::Section(section), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, b.px(8.0));
            y += b.px(38.0);
        }
    });
    let content = Rect::new(nav.right, body.top, body.right - nav.right, body.height());
    let view = key(("settings", app.nav.section().title()));
    b.scroll(view, content, app.nav.scroll(view), Action::None, |b, top| {
        let w = (content.width() - b.px(64.0)).min(b.px(720.0));
        let mut col = Col { x: content.left + b.px(32.0), y: top + b.px(24.0), w, view };
        match app.nav.section() {
            Section::General => general(app, b, &mut col),
            Section::Performance => performance(app, b, &mut col),
            Section::Desktop => desktop(app, b, &mut col),
            Section::Keyboard => keyboard(b, &mut col),
            Section::Troubleshoot => troubleshoot(app, b, &mut col),
            Section::About => about(app, b, &mut col),
        }
        col.y + b.px(24.0)
    });
}

/// A titled card of rows; `rows` draws them inside a padded column.
fn group(b: &mut B, col: &mut Col, title: &str, rows: impl FnOnce(&mut B, &mut Col)) {
    let p = *b.p;
    if !title.is_empty() {
        b.section_header(Rect::new(col.x + b.px(4.0), col.y, col.w, b.px(18.0)), title);
        col.y += b.px(26.0);
    }
    // The card is drawn under its rows: its ops go first, sized after.
    let at = b.list.ops.len();
    let mut inner = Col { x: col.x + b.px(16.0), y: col.y + b.px(8.0), w: col.w - b.px(32.0), view: col.view };
    rows(b, &mut inner);
    let rect = Rect::new(col.x, col.y, col.w, inner.y - col.y + b.px(4.0));
    let card = vec![mypaper_ui::Op::Fill { rect, radius: b.px(12.0), color: p.panel }];
    b.list.ops.splice(at..at, card);
    col.y = rect.bottom + b.px(28.0);
}

/// A hairline between rows of a card, inset from its left like the rows.
fn divider(b: &mut B, c: &mut Col) {
    b.hline(c.x, c.x + c.w + b.px(16.0), c.y - b.px(2.0), b.p.line);
    c.y += b.px(4.0);
}

fn rows(app: &App, b: &mut B, c: &mut Col, ids: &[&'static str]) {
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            divider(b, c);
        }
        setting(app, b, c, id);
    }
}

fn general(app: &App, b: &mut B, col: &mut Col) {
    group(b, col, "Startup", |b, c| {
        let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
        let auto = app.system.autostart.as_ref();
        let detail = match auto {
            Some(a) if a.enabled => "mypaper starts when you sign in",
            Some(_) => "Off",
            None => "Checking…",
        };
        label(b, row, "Start with Windows", detail, None);
        let on = auto.is_some_and(|a| a.enabled);
        b.switch(key("autostart"), Point::new(c.x + c.w - b.px(36.0), c.y + b.px(10.0)), on, "Start with Windows", Action::ToggleAutostart(!on));
        crate::ui::anchor(app, "autostart", c.view, c.y);
        c.y += b.px(44.0);
        divider(b, c);
        setting(app, b, c, "static_snapshot");
    });
    group(b, col, "Studio", |b, c| setting(app, b, c, "studio_appearance"));
    group(b, col, "Experimental", |b, c| {
        rows(app, b, c, &["experimental_file_index", "experimental_files_default", "experimental_app_styler"]);
    });
}

fn performance(app: &App, b: &mut B, col: &mut Col) {
    let p = *b.p;
    b.section_header(Rect::new(col.x + b.px(4.0), col.y, col.w, b.px(18.0)), "Graphics card");
    col.y += b.px(26.0);
    crate::ui::anchor(app, "gpu", col.view, col.y);
    let gpus = app.system.gpus.clone().unwrap_or_default();
    let integrated = gpus.iter().find(|(_, i)| *i).map(|(n, _)| n.clone());
    let discrete = gpus.iter().find(|(_, i)| !*i).map(|(n, _)| n.clone());
    let current = app.get(DocId::Config, "/engine/gpu").and_then(Value::as_str).unwrap_or("auto").to_owned();
    let gap = b.px(10.0);
    let tw = (col.w - gap * 2.0) / 3.0;
    let th = b.px(96.0);
    let edit = Edit::new(DocId::Config, "/engine/gpu", "Graphics card");
    b.group(key("gpu-tiles"), |b| {
        for (i, (id, title, detail, g)) in [
            ("integrated", "Integrated", format!("{} · uses the least power", integrated.as_deref().unwrap_or("Built in")), glyph::CHIP),
            ("high-performance", "High performance", discrete.clone().unwrap_or_else(|| "None found".into()), glyph::GAUGE),
            ("auto", "Your screens' card", format!("{} · draws where your screens are", app.system.display_gpu.as_deref().unwrap_or("The one they use")), glyph::MONITOR),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(col.x + i as f32 * (tw + gap), col.y, tw, th);
            let k = key(("gpu", id));
            let on = current == id;
            b.fill(r, b.px(12.0), p.panel);
            b.wash(k, r, b.px(12.0));
            b.icon(Rect::new(r.left + b.px(14.0), r.top + b.px(14.0), b.px(20.0), b.px(20.0)), g, 17.0, if on { p.ink1 } else { p.ink2 });
            if on {
                b.fill(r, b.px(12.0), p.selected);
                b.check_mark(r);
            }
            let strong = b.font(Font::Heading);
            let small = b.font(Font::Caption);
            b.text_at(r.left + b.px(14.0), r.top + b.px(46.0), title, &strong, p.ink1);
            b.text_in(Rect::new(r.left + b.px(14.0), r.top + b.px(66.0), r.width() - b.px(28.0), b.px(16.0)), &detail, &small, p.ink3, Align::Start);
            b.hit_checked(k, r, Role::RadioButton, title, Some(on), Action::Set(edit.clone(), json!(id)), mypaper_ui::controls::Behavior::Click);
            b.focus_ring(k, r, b.px(10.0));
        }
    });
    col.y += th + b.px(12.0);
    // The live line: what the engine uses now.
    let (cpu, _) = app.system.total();
    let fps = app.get(DocId::Config, "/engine/fps").and_then(Value::as_u64).map_or("its own frame rate".into(), |f| format!("{f} fps"));
    let gpu_now = match current.as_str() {
        "high-performance" => discrete.clone().or(integrated.clone()),
        "auto" => app.system.display_gpu.clone().or(integrated.clone()),
        _ => integrated.clone().or(discrete.clone()),
    }
    .unwrap_or_else(|| "The graphics card".into());
    let small = b.font(Font::Caption);
    b.dot(Point::new(col.x + b.px(4.0), col.y + b.px(8.0)), app.system.running());
    let line = if app.system.running() { format!("Now: {gpu_now} · {fps} · {cpu:.1}% CPU") } else { "mypaper is off".into() };
    b.text_at(col.x + b.px(16.0), col.y, &line, &small, p.ink3);
    col.y += b.px(36.0);
    if current == "high-performance" && discrete.is_none() && !gpus.is_empty() {
        b.text_at(col.x, col.y - b.px(12.0), &format!("No high-performance graphics card found; using {gpu_now}."), &small, p.ink2);
        col.y += b.px(16.0);
    }
    group(b, col, "Frame rate", |b, c| {
        setting(app, b, c, "fps");
        divider(b, c);
        let own: Vec<String> = app.get(DocId::Config, "/engine/wallpapers").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let names: Vec<String> = own.iter().map(|id| app.library.items.iter().find(|w| &w.id == id).map_or(id.clone(), |w| w.name.clone())).collect();
        let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
        label(b, row, "Per wallpaper", &if names.is_empty() { "None have their own".into() } else { names.join(", ") }, None);
        crate::ui::anchor(app, "per_wallpaper", c.view, c.y);
        let text = format!("{} {} their own", own.len(), if own.len() == 1 { "has" } else { "have" });
        let w = b.button_width(&text, true, true);
        b.button(key("per-wallpaper"), Rect::new(row.right - w, row.top + b.px(6.0), w, b.px(28.0)), &text, Some(glyph::CHEVRON_RIGHT), ButtonKind::Ghost, true, Action::WallpapersOwn);
        c.y += b.px(44.0);
    });
    group(b, col, "When to rest", |b, c| rows(app, b, c, &["pause_on_battery", "pause_on_fullscreen", "save_power"]));
    group(b, col, "Motion", |b, c| setting(app, b, c, "reduce_motion"));
}

fn desktop(app: &App, b: &mut B, col: &mut Col) {
    group(b, col, "Right-click menu", |b, c| {
        setting(app, b, c, "desktop_menu");
        let on = app.get(DocId::Config, "/desktop_menu/enabled").and_then(Value::as_bool).unwrap_or(true);
        if !on {
            return;
        }
        divider(b, c);
        crate::ui::anchor(app, "desktop_menu_items", c.view, c.y);
        menu_items(app, b, c);
    });
    group(b, col, "Explorer", |b, c| setting(app, b, c, "classic_menu"));
}

/// The menu's items as a sortable list beside a live preview of the real menu.
fn menu_items(app: &App, b: &mut B, c: &mut Col) {
    let p = *b.p;
    let items = entries(app);
    let list_w = c.w * 0.55;
    let top = c.y + b.px(4.0);
    let rect = Rect::new(c.x, top, list_w, items.len().max(1) as f32 * b.px(34.0));
    b.fill(rect, b.px(10.0), p.well);
    b.group(key("menu-items"), |b| {
        for (i, e) in items.iter().enumerate() {
            let label = match e {
                Entry::Builtin(Builtin::Separator) => "──────".to_owned(),
                Entry::Builtin(builtin) => builtin_label(*builtin).to_owned(),
                Entry::Custom(custom) => format!("{} (yours)", custom.label),
            };
            let r = Rect::new(c.x, top + i as f32 * b.px(34.0), list_w, b.px(34.0));
            crate::ui::list_row(b, r, key(("menu-item", i)), None, &label, Action::MenuRemove(i), Some(Action::MenuRemove(i)), i + 1 == items.len());
        }
    });
    // The live preview: the shell's own menu view with these items.
    let config = mypaper_config::DesktopMenu { enabled: true, items: items.clone() };
    let tokens = mypaper_shell::preview::menu::tokens(&crate::app::theme_name(&app.docs).unwrap_or_default(), b.text);
    let menu = mypaper_shell::preview::menu::build(&config, mypaper_shell::preview::menu::ViewState { icons_visible: true, ..Default::default() });
    let layout = mypaper_ui::menu::layout(&menu, &tokens, b.s, b.text);
    let list = mypaper_ui::menu::view(&menu, &layout, None, &tokens);
    let px = rect.right + b.px(24.0);
    let preview = Rect::new(px, top, c.x + c.w - px, layout.height + b.px(40.0));
    if let Some(still) = app.library.active().and_then(|i| app.library.items[i].preview.clone())
        && let Some(image) = app.images.borrow_mut().get(&still, (preview.width() as u32, preview.height() as u32))
    {
        crate::stage::image_round(b, preview, &image, 1.0, b.px(8.0));
    } else {
        b.fill(preview, b.px(8.0), mypaper_ui::Rgba::rgb8([0x2a, 0x30, 0x4a], 1.0));
    }
    b.list.ops.push(mypaper_ui::Op::Clip { rect: preview });
    b.list.ops.extend(crate::stage::offset(&list.ops, preview.left + b.px(4.0), preview.top + b.px(4.0)));
    b.list.ops.push(mypaper_ui::Op::Unclip);
    c.y = rect.bottom.max(preview.bottom) + b.px(10.0);
    // Add a built-in, a separator, your own; reset.
    let hidden: Vec<Builtin> = ALL_BUILTINS.iter().copied().filter(|b| !items.contains(&Entry::Builtin(*b))).collect();
    let mut x = c.x;
    for builtin in hidden {
        let label = format!("+ {}", builtin_label(builtin));
        let w = b.button_width(&label, false, true);
        if x + w > c.x + c.w {
            x = c.x;
            c.y += b.px(34.0);
        }
        b.button(key(("menu-add", builtin_label(builtin))), Rect::new(x, c.y, w, b.px(28.0)), &label, None, ButtonKind::Secondary, true, Action::MenuAdd(builtin_id(builtin)));
        x += w + b.px(6.0);
    }
    for (label, action) in [("+ Separator", Action::MenuAdd("separator".into())), ("Reset", Action::MenuReset)] {
        let w = b.button_width(label, false, true);
        if x + w > c.x + c.w {
            x = c.x;
            c.y += b.px(34.0);
        }
        b.button(key(("menu-btn", label)), Rect::new(x, c.y, w, b.px(28.0)), label, None, ButtonKind::Ghost, true, action);
        x += w + b.px(6.0);
    }
    c.y += b.px(38.0);
    // Your own item: a name and a program.
    let half = (c.w - b.px(90.0)) / 2.0;
    crate::ui::text_field(app, b, Rect::new(c.x, c.y, half, b.px(32.0)), FieldId::Text(DocId::Studio, "/menu_label".into()), None, "Your own item's name", "", "Name");
    crate::ui::text_field(app, b, Rect::new(c.x + half + b.px(8.0), c.y, half, b.px(32.0)), FieldId::Text(DocId::Studio, "/menu_command".into()), None, "Program", "", "Program");
    b.button(key("menu-custom"), Rect::new(c.x + c.w - b.px(74.0), c.y, b.px(74.0), b.px(32.0)), "Add", None, ButtonKind::Secondary, true, Action::MenuAdd("custom".into()));
    c.y += b.px(42.0);
}

const ALL_BUILTINS: [Builtin; 13] = [
    Builtin::View,
    Builtin::Sort,
    Builtin::Refresh,
    Builtin::New,
    Builtin::Paste,
    Builtin::Wallpaper,
    Builtin::Widgets,
    Builtin::AddWidget,
    Builtin::Studio,
    Builtin::Display,
    Builtin::Personalize,
    Builtin::Terminal,
    Builtin::More,
];

fn builtin_label(b: Builtin) -> &'static str {
    match b {
        Builtin::Separator => "Separator",
        Builtin::View => "View",
        Builtin::Sort => "Sort by",
        Builtin::Refresh => "Refresh",
        Builtin::New => "New",
        Builtin::Paste => "Paste",
        Builtin::Wallpaper => "Change wallpaper",
        Builtin::Widgets => "Edit widgets",
        Builtin::AddWidget => "Add widget",
        Builtin::Studio => "mypaper Studio",
        Builtin::Display => "Display settings",
        Builtin::Personalize => "Personalize",
        Builtin::Terminal => "Open terminal here",
        Builtin::More => "Show more options",
    }
}

fn builtin_id(b: Builtin) -> String {
    serde_json::to_value(b).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn entries(app: &App) -> Vec<Entry> {
    app.get(DocId::Config, "/desktop_menu/items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(default_items)
}

fn keyboard(b: &mut B, col: &mut Col) {
    group(b, col, "Shortcuts", |b, c| {
        let list: [(&str, &[&str]); 9] = [
            ("Restore Windows (the emergency stop)", &["Ctrl", "Alt", "Shift", "Backspace"]),
            ("Arrange widgets on the desktop", &["Ctrl", "Alt", "Shift", "W"]),
            ("Always-on-top widgets", &["Ctrl", "Alt", "Shift", "O"]),
            ("Search everything in Studio", &["Ctrl", "K"]),
            ("Peek at your desktop", &["Ctrl", "D"]),
            ("Undo, redo", &["Ctrl", "Z"]),
            ("Go to a place", &["Ctrl", "1–6"]),
            ("Next region", &["F6"]),
            ("Move a list row", &["Alt", "↑"]),
        ];
        for (i, (name, keys)) in list.iter().enumerate() {
            if i > 0 {
                divider(b, c);
            }
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            label(b, row, name, "", None);
            let style = b.font(Font::Micro);
            let w: f32 = keys.iter().map(|k| (b.measure(k, &style).0 + b.px(10.0)).max(b.px(20.0)) + b.px(4.0)).sum();
            b.keys(row.right - w, row.top + b.px(8.0), keys);
            c.y += b.px(40.0);
        }
        let small = b.font(Font::Caption);
        c.y += b.paragraph(c.x, c.y, c.w, "Shortcuts can't be changed yet.", &small, b.px(16.0), b.p.ink3);
    });
}

fn troubleshoot(app: &App, b: &mut B, col: &mut Col) {
    let running = app.system.running();
    group(b, col, "mypaper", |b, c| {
        let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
        label(b, row, if running { "Running" } else { "Stopped" }, if running { "Core, the engine and the shell" } else { "Windows shows its own desktop" }, None);
        let (text, action) = if running { ("Stop", Action::StopMypaper) } else { ("Start mypaper", Action::StartMypaper) };
        let w = b.button_width(text, false, true);
        b.button(key("mypaper-run"), Rect::new(row.right - w, row.top + b.px(6.0), w, b.px(28.0)), text, None, if running { ButtonKind::Secondary } else { ButtonKind::Primary }, true, action);
        c.y += b.px(44.0);
    });
    group(b, col, "Parts", |b, c| {
        for (i, ((id, _), part)) in crate::system::PARTS.iter().zip(app.system.parts).enumerate() {
            if i > 0 {
                divider(b, c);
            }
            let name = match *id {
                "core" => "Core",
                "engine" => "Engine (the wallpaper and glass)",
                _ => "Shell (bars, widgets, menus)",
            };
            let detail = if part.running { format!("Running · {:.0} MB · {:.1}% CPU", part.memory, part.cpu) } else { "Stopped".into() };
            let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
            label(b, row, name, &detail, None);
            let w = b.button_width("Restart", true, true);
            b.button(key(("restart", *id)), Rect::new(row.right - w, row.top + b.px(6.0), w, b.px(28.0)), "Restart", Some(glyph::RESET), ButtonKind::Secondary, running, Action::Restart(id));
            c.y += b.px(44.0);
        }
    });
    group(b, col, "", |b, c| {
        let row = Rect::new(c.x, c.y, c.w, b.px(44.0));
        label(b, row, "Restore the Windows desktop", "The emergency stop: Ctrl Alt Shift Backspace", None);
        let w = b.button_width("Restore", false, true);
        b.button(key("restore"), Rect::new(row.right - w, row.top + b.px(8.0), w, b.px(28.0)), "Restore", None, ButtonKind::Secondary, true, Action::RestoreDesktop);
        c.y += b.px(48.0);
    });
    group(b, col, "Start over", |b, c| {
        for (i, (name, detail, doc)) in [
            ("Bars", "Back to the default bars", DocId::Bars),
            ("Widgets", "No widgets, the default grid", DocId::Widgets),
            ("Look", "Back to the wallpaper's look", DocId::Config),
        ]
        .iter()
        .enumerate()
        {
            if i > 0 {
                divider(b, c);
            }
            let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
            label(b, row, name, &format!("{detail}; a backup is kept"), None);
            let w = b.button_width("Start over", false, true);
            // A file from a newer mypaper is never started over (restart Studio instead).
            b.button(key(("start-over", *name)), Rect::new(row.right - w, row.top + b.px(6.0), w, b.px(28.0)), "Start over", None, ButtonKind::Secondary, !app.docs.newer(*doc), Action::StartOver(*doc));
            c.y += b.px(44.0);
        }
    });
    group(b, col, "Help", |b, c| {
        let w1 = b.button_width("Copy diagnostics", true, true);
        b.button(key("diag"), Rect::new(c.x, c.y, w1, b.px(28.0)), "Copy diagnostics", Some(glyph::COPY), ButtonKind::Secondary, true, Action::CopyDiagnostics);
        let w2 = b.button_width("Open logs", true, true);
        b.button(key("logs"), Rect::new(c.x + w1 + b.px(8.0), c.y, w2, b.px(28.0)), "Open logs", Some(glyph::FILE), ButtonKind::Secondary, true, Action::OpenLogs);
        c.y += b.px(36.0);
        divider(b, c);
        let how = app.system.autostart.as_ref().and_then(|a| a.method.as_deref()).map_or("Not set to start with Windows", |m| if m == "task" { "A sign-in task" } else { "A startup entry" });
        let at = app.system.core_path.as_ref().and_then(|p| p.parent()).map_or("Not found".into(), |p| p.display().to_string());
        for (k, v) in [("How it starts", how.to_owned()), ("Installed at", at)] {
            let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
            label(b, row, k, "", None);
            let small = b.font(Font::Mono);
            b.text_in(Rect::new(row.left + b.px(150.0), row.top, row.width() - b.px(150.0), row.height()), &v, &small, b.p.ink2, Align::End);
            c.y += b.px(38.0);
        }
    });
}

fn about(app: &App, b: &mut B, col: &mut Col) {
    group(b, col, "mypaper", |b, c| {
        let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
        label(b, row, "Version", "", None);
        let small = b.font(Font::Caption);
        b.text_in(row, env!("CARGO_PKG_VERSION"), &small, b.p.ink2, Align::End);
        c.y += b.px(40.0);
        divider(b, c);
        let row = Rect::new(c.x, c.y, c.w, b.px(36.0));
        label(b, row, "Licenses", "mypaper is MIT licensed", None);
        c.y += b.px(40.0);
    });
    group(b, col, "Wallpaper credits", |b, c| {
        let credited: Vec<&crate::library::Wallpaper> = app.library.items.iter().filter(|w| w.credit.is_some()).collect();
        for (i, w) in credited.iter().enumerate() {
            if i > 0 {
                divider(b, c);
            }
            let credit = w.credit.as_ref().expect("filtered");
            let row = Rect::new(c.x, c.y, c.w, b.px(40.0));
            let detail = format!("{} · {}", credit.author.as_deref().unwrap_or(""), credit.license.as_deref().unwrap_or(""));
            label(b, row, &w.name, &detail, None);
            if let Some(source) = &credit.source {
                let bw = b.button_width("Source", false, true);
                b.button(key(("credit", i)), Rect::new(row.right - bw, row.top + b.px(6.0), bw, b.px(28.0)), "Source", None, ButtonKind::Ghost, true, Action::Open(source.clone()));
            }
            c.y += b.px(44.0);
        }
    });
}

pub fn run(app: &mut App, action: Action) {
    match action {
        Action::ToggleAutostart(on) => match crate::system::autostart(if on { "on" } else { "off" }) {
            Ok(state) => app.system.autostart = Some(state),
            Err(e) => app.alert(format!("{e:#}")),
        },
        Action::MenuReset => app.apply("Reset the menu", vec![Op { doc: DocId::Config, path: "/desktop_menu/items".into(), value: None }]),
        Action::MenuAdd(what) => {
            let mut items = entries(app);
            match what.as_str() {
                "custom" => {
                    let label = app.text(&FieldId::Text(DocId::Studio, "/menu_label".into())).trim().to_owned();
                    let command = app.text(&FieldId::Text(DocId::Studio, "/menu_command".into())).trim().to_owned();
                    if label.is_empty() || command.is_empty() {
                        app.alert("Give your item a name and a program");
                        return;
                    }
                    items.push(Entry::Custom(mypaper_config::desktop_menu::Custom { label, command, args: vec![] }));
                    app.texts.remove(&FieldId::Text(DocId::Studio, "/menu_label".into()));
                    app.texts.remove(&FieldId::Text(DocId::Studio, "/menu_command".into()));
                }
                id => {
                    let Ok(builtin) = serde_json::from_value::<Builtin>(json!(id)) else { return };
                    items.push(Entry::Builtin(builtin));
                }
            }
            set_items(app, items, "Add a menu item");
        }
        Action::MenuRemove(i) => {
            let mut items = entries(app);
            if i < items.len() {
                items.remove(i);
                set_items(app, items, "Remove a menu item");
            }
        }
        Action::MenuMove(i, delta) => {
            let mut items = entries(app);
            let to = i as i32 + delta;
            if to >= 0 && (to as usize) < items.len() {
                items.swap(i, to as usize);
                set_items(app, items, "Move a menu item");
                app.input.focus = Some(key(("menu-item", to as usize)));
            }
        }
        Action::RestartStudio => match crate::system::restart_studio() {
            Ok(()) => app.quit = true,
            Err(e) => app.alert(format!("{e:#}")),
        },
        Action::StartOver(doc) => {
            if doc == DocId::Config && app.docs.error(DocId::Config).is_none() {
                // Look's start over: only the look goes (a backup of the file is kept).
                if let Some(path) = app.docs.doc(DocId::Config).path.clone() {
                    let _ = std::fs::copy(&path, path.with_extension("json.bak"));
                }
                app.apply("Start over: look", vec![Op { doc: DocId::Config, path: "/look".into(), value: None }]);
                app.toast("Your look is back to the wallpaper's (config.json.bak keeps the old one)", None);
                return;
            }
            let defaults = match doc {
                DocId::Bars => "{ \"version\": 1 }\n".to_owned(),
                // The current layout version: an older one would be upgraded (and refused) again.
                DocId::Widgets => format!("{{ \"version\": {}, \"instances\": [] }}\n", mypaper_widgets::layout::VERSION),
                _ => "{}\n".to_owned(),
            };
            match app.docs.start_over(doc, &defaults, |backup| confirm_start_over(doc, backup)) {
                Ok(StartedOver::Declined) => {}
                Ok(StartedOver::Done(backup)) => app.toast(format!("Started over{}", backup.map_or(String::new(), |b| format!("; the old file is {}", b.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))), None),
                Err(e) => app.alert(format!("{e:#}")),
            }
        }
        Action::ShowFile(doc) => {
            if let Some(path) = app.docs.doc(doc).path.clone() {
                crate::system::reveal(&path);
            }
        }
        _ => {}
    }
}

fn set_items(app: &mut App, items: Vec<Entry>, label: &str) {
    let value = serde_json::to_value(items).unwrap_or(Value::Null);
    app.apply(label, vec![Op { doc: DocId::Config, path: "/desktop_menu/items".into(), value: Some(value) }]);
}

/// Asks before Start over, in a modal dialog (Cancel is the default): what goes, and where the
/// current file is kept first.
fn confirm_start_over(doc: DocId, backup: Option<&std::path::Path>) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MB_TASKMODAL, MessageBoxW,
    };
    let what = match doc {
        DocId::Widgets => "All your widgets and where they sit are removed.",
        DocId::Bars => "Your bars go back to the defaults.",
        _ => "These settings go back to the defaults.",
    };
    let kept = backup
        .and_then(|b| b.file_name())
        .map(|n| format!("\n\nThe current file is kept first as {}.", n.to_string_lossy()))
        .unwrap_or_default();
    let text = windows::core::HSTRING::from(format!("Start over?\n\n{what}{kept}"));
    // SAFETY: a modal dialog on this (the UI) thread with owned strings.
    unsafe {
        MessageBoxW(
            None,
            &text,
            windows::core::w!("Start over"),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TASKMODAL,
        ) == IDOK
    }
}
