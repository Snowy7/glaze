use mypaper_ui::controls::{Align, Behavior, key};
use mypaper_ui::{Rect, Role};
use serde_json::{Value, json};

use super::controls::{
    self as ui, button, group, height, icon_button, more, notice, row, setting, text, title,
};
use super::{Command, Page};
use crate::actions::{Action, Bar, Edit};
use crate::app::App;
use crate::docs::DocId;
use crate::theme::glyph;
use crate::ui::{B, Col};

fn open(page: Page) -> Action {
    Action::Panel(Command::Open(page))
}

pub fn header(app: &App, b: &mut B, r: Rect, page: Page) {
    let done_style = ui::font(b, 13.0, 500);
    let done_width = (b.measure("Done", &done_style).0 + b.px(20.0)).max(b.px(52.0));
    let done = Rect::new(
        r.right - done_width - b.px(8.0),
        b.px(10.0),
        done_width,
        b.px(28.0),
    );
    button(
        b,
        done,
        "Done",
        None,
        true,
        true,
        Action::Panel(Command::Done),
    );
    let mut x = done.left - b.px(6.0);
    let editing = app.system.running();
    let compact = r.width() <= b.px(360.0) || b.t.text_scale > 1.0;
    for (icon, label, enabled, action) in [
        (
            glyph::REDO,
            "Redo (Ctrl Y)",
            app.docs.can_redo(),
            Action::Redo,
        ),
        (
            glyph::UNDO,
            "Undo (Ctrl Z)",
            app.docs.can_undo(),
            Action::Undo,
        ),
    ] {
        if !editing || compact && icon == glyph::REDO {
            continue;
        }
        x -= b.px(26.0);
        icon_button(
            b,
            Rect::new(x, b.px(10.0), b.px(24.0), b.px(28.0)),
            icon,
            label,
            enabled,
            action,
        );
    }
    if editing {
        let status = if !app.panel.as_ref().unwrap().save_errors.borrow().is_empty()
            || !app.panel.as_ref().unwrap().theme_errors.borrow().is_empty()
            || crate::docs::DocId::ALL
                .into_iter()
                .any(|id| app.docs.error(id).is_some())
        {
            "Couldn't save"
        } else if app.drag.is_some() {
            "Saving…"
        } else {
            match &app.docs.saved {
                crate::docs::Saved::Failed(_) => "Couldn't save",
                _ => "Saved",
            }
        };
        let style = ui::font(b, 10.0, 400);
        let width = b.measure(status, &style).0 + b.px(8.0);
        x -= width;
        text(
            b,
            Rect::new(x, b.px(10.0), width, b.px(28.0)),
            status,
            10.0,
            400,
            b.p.ink3,
            Align::Center,
        );
    }
    x -= b.px(8.0);
    if editing {
        b.vline(x + b.px(2.0), b.px(15.0), b.px(33.0), b.p.line);
    }
    x -= b.px(4.0);
    if compact && editing {
        x -= b.px(26.0);
        let mut options = vec![(json!("history"), "History (Ctrl H)".into())];
        if app.docs.can_undo() {
            options.push((json!("undo"), "Undo (Ctrl Z)".into()));
        }
        if app.docs.can_redo() {
            options.push((json!("redo"), "Redo (Ctrl Y)".into()));
        }
        icon_button(
            b,
            Rect::new(x, b.px(10.0), b.px(24.0), b.px(28.0)),
            glyph::CHEVRON_DOWN,
            "More actions",
            true,
            Action::Choose(crate::actions::Choices {
                edit: Edit::new(DocId::Studio, "/panel/header", "Header action"),
                options,
                current: None,
            }),
        );
    }
    for (icon, label) in [
        (glyph::EYE, "Hold to peek (Ctrl D)"),
        (glyph::CLOCK, "History (Ctrl H)"),
    ] {
        if !editing || compact {
            continue;
        }
        x -= b.px(26.0);
        icon_button(
            b,
            Rect::new(x, b.px(10.0), b.px(24.0), b.px(28.0)),
            icon,
            label,
            true,
            if icon == glyph::CLOCK {
                open(Page::History)
            } else {
                Action::Panel(Command::Peek)
            },
        );
        if icon == glyph::EYE
            && let Some(target) = b.targets.last_mut()
        {
            target.behavior = Behavior::Drag;
        }
    }
    let left = b.px(12.0);
    let parent = if matches!(page, Page::Module | Page::AddModule) && app.nav.bar == Some(Bar::Dock)
    {
        Some(Page::Dock)
    } else {
        page.parent()
    };
    let end = if let Some(parent) = parent {
        let style = ui::font(b, 13.0, 500);
        let width = (b.measure(parent.title(), &style).0 + b.px(24.0))
            .min((x - left - b.px(8.0)).max(b.px(40.0)));
        let back = Rect::new(left - b.px(4.0), b.px(10.0), width, b.px(28.0));
        b.icon(
            Rect::new(back.left, back.top, b.px(16.0), back.height()),
            glyph::CHEVRON_LEFT,
            16.0,
            b.p.ink2,
        );
        text(
            b,
            Rect::new(
                back.left + b.px(18.0),
                back.top,
                back.width() - b.px(18.0),
                back.height(),
            ),
            parent.title(),
            13.0,
            500,
            b.p.ink2,
            Align::Start,
        );
        let k = key("panel-back");
        b.hit(
            k,
            back,
            Role::Button,
            &format!("Back to {}", parent.title()),
            Action::Panel(Command::Back),
            Behavior::Click,
        );
        b.focus_ring(k, back, b.px(6.0));
        back.right
    } else {
        let mark = Rect::new(left, b.px(16.0), b.px(16.0), b.px(16.0));
        b.fill(mark, b.px(4.0), b.p.ink1);
        b.icon(mark, glyph::WALLPAPERS, 12.0, b.p.chrome);
        let style = ui::font(b, 13.0, 600);
        let name_width = b.measure("Studio", &style).0 + b.px(4.0);
        text(
            b,
            Rect::new(mark.right + b.px(8.0), 0.0, name_width, r.height()),
            "Studio",
            13.0,
            600,
            b.p.ink1,
            Align::Start,
        );
        mark.right + b.px(8.0) + name_width
    };
    let caption_x = end + b.px(8.0);
    // The caption yields to the fixed icon actions on a 360-DIP panel.
    let style = ui::font(b, 12.0, 400);
    let caption = if editing { "" } else { "Off" };
    if x - caption_x >= b.measure(caption, &style).0 {
        text(
            b,
            Rect::new(caption_x, 0.0, x - caption_x, r.height()),
            caption,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
}

pub fn view(app: &App, b: &mut B, c: &mut Col, page: Page) {
    match page {
        Page::Root => root(app, b, c),
        Page::Widgets => widgets(app, b, c),
        Page::Bars => bars(app, b, c),
        Page::Settings => settings(app, b, c),
        Page::Performance => performance(app, b, c),
        Page::Top | Page::Dock => super::editors::view(app, b, c, page),
        _ => super::editors::view(app, b, c, page),
    }
}

fn wallpaper_name(app: &App) -> &str {
    app.library
        .active()
        .map_or("No wallpaper", |i| app.library.items[i].name.as_str())
}

fn image(app: &App, b: &mut B, r: Rect, index: usize) {
    let wallpaper = &app.library.items[index];
    b.fill(r, b.px(6.0), b.p.fill);
    let still = wallpaper.preview.as_ref().or(wallpaper.video.as_ref());
    if let Some(still) = still
        && let Some(image) = app
            .images
            .borrow_mut()
            .get(still, (r.width() as u32, r.height() as u32))
    {
        // The renderer clips cover images to the rounded thumbnail's own geometry.
        crate::stage::image_round(b, r, &image, 1.0, b.px(6.0));
    } else {
        b.icon(r, glyph::WALLPAPERS, 16.0, b.p.ink3);
    }
}

fn metrics(app: &App) -> String {
    let (cpu, memory) = app.system.total();
    let gpu = app
        .get(DocId::Config, "/engine/gpu")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let gpu = match gpu {
        "integrated" => "Integrated",
        "high-performance" => "High performance",
        _ => "Windows' choice",
    };
    let fps = app
        .get(DocId::Config, "/engine/fps")
        .and_then(Value::as_u64)
        .map_or("Wallpaper's fps".into(), |n| format!("{n} fps cap"));
    format!("{gpu} · {fps} · {cpu:.1}% CPU · {memory:.0} MB")
}

fn root(app: &App, b: &mut B, c: &mut Col) {
    let h = height(b, 56.0);
    let status = Rect::new(c.x, c.y, c.w, h);
    b.fill(status, b.px(10.0), b.p.panel);
    let thumb = Rect::new(
        c.x + b.px(8.0),
        c.y + (h - b.px(32.0)) / 2.0,
        b.px(32.0),
        b.px(32.0),
    );
    if let Some(i) = app.library.active() {
        image(app, b, thumb, i);
    } else {
        b.icon(thumb, glyph::WALLPAPERS, 20.0, b.p.ink3);
    }
    let paused = app
        .get(DocId::Config, "/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let playing = if !app.system.running() {
        "Off"
    } else if paused {
        "Paused"
    } else {
        "Playing"
    };
    let x = thumb.right + b.px(10.0);
    let label = if paused { "Resume" } else { "Pause" };
    let control_w = b.px(if paused { 90.0 } else { 80.0 });
    let available = c.right() - x - control_w - b.px(16.0);
    text(
        b,
        Rect::new(x, c.y + b.px(8.0), available, height(b, 21.0)),
        &format!("{playing} · {}", wallpaper_name(app)),
        13.0,
        600,
        b.p.ink1,
        Align::Start,
    );
    text(
        b,
        Rect::new(x, c.y + b.px(29.0), available, height(b, 18.0)),
        &metrics(app),
        12.0,
        400,
        b.p.ink3,
        Align::Start,
    );
    button(
        b,
        Rect::new(
            c.right() - control_w - b.px(8.0),
            c.y + (h - b.px(28.0)) / 2.0,
            control_w,
            b.px(28.0),
        ),
        label,
        Some(if paused { glyph::PLAY } else { glyph::PAUSE }),
        false,
        app.system.running(),
        Action::Pause,
    );
    c.y += h + b.px(12.0);
    ui::search(app, b, c);
    group(b, c, "Your desktop", |b, c| {
        row(
            b,
            c,
            Some(glyph::WALLPAPERS),
            "Wallpaper",
            wallpaper_name(app),
            "",
            None,
            open(Page::Wallpapers),
        );
        let recent = crate::pages::home::recent(app);
        if !recent.is_empty() {
            // The thumbnails belong to the Wallpaper row, above its separator.
            if matches!(b.list.ops.last(), Some(mypaper_ui::Op::Line { .. })) {
                b.list.ops.pop();
            }
            let gap = b.px(8.0);
            let start = c.x + b.px(28.0);
            let width = ((c.right() - start - gap * 4.0) / 5.0).min(b.px(72.0));
            for (n, &i) in recent.iter().take(5).enumerate() {
                let r = Rect::new(
                    start + n as f32 * (width + gap),
                    c.y + b.px(4.0),
                    width,
                    b.px(42.0),
                );
                image(app, b, r, i);
                if app.library.items[i].active {
                    let check = Rect::new(
                        r.right - b.px(18.0),
                        r.top + b.px(3.0),
                        b.px(14.0),
                        b.px(14.0),
                    );
                    b.fill(check, b.px(7.0), b.p.solid);
                    b.icon(check, glyph::CHECK, 10.0, b.p.on_solid);
                }
                // Wallpaper trying is deferred: the strip opens its panel editor.
                let k = key(("panel-recent", i));
                b.hit(
                    k,
                    r,
                    Role::Button,
                    &app.library.items[i].name,
                    Action::UseWallpaper(i),
                    Behavior::Click,
                );
                b.focus_ring(k, r, b.px(6.0));
            }
            c.y += b.px(58.0);
            b.hline(c.x, c.right(), c.y - 1.0, b.p.line);
        }
        let look = crate::pages::look::summary(app)
            .split(" · ")
            .take(2)
            .collect::<Vec<_>>()
            .join(" · ");
        row(
            b,
            c,
            Some(glyph::LOOK),
            "Look",
            &look,
            "",
            None,
            open(Page::Look),
        );
        let preset = app
            .get(DocId::Bars, "/preset")
            .and_then(Value::as_str)
            .unwrap_or("monochrome");
        let preset = crate::pages::bars::PRESETS
            .iter()
            .find(|(id, _)| *id == preset)
            .map_or(preset, |(_, n)| *n);
        row(
            b,
            c,
            Some(glyph::BARS),
            "Bars",
            preset,
            "",
            None,
            open(Page::Bars),
        );
        row(
            b,
            c,
            Some(glyph::WIDGETS),
            "Widgets",
            &format!("{} on this display", crate::pages::widgets::count_here(app)),
            "",
            None,
            open(Page::Widgets),
        );
        let own_menu = app
            .get(DocId::Config, "/desktop_menu/enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        row(
            b,
            c,
            Some(glyph::NOTES),
            "Desktop menu",
            if own_menu { "mypaper's" } else { "Windows'" },
            "",
            None,
            open(Page::DesktopMenu),
        );
    });
    group(b, c, "Around it", |b, c| {
        let windows = app
            .get(DocId::Config, "/look/windows/apps")
            .and_then(Value::as_str)
            .unwrap_or("as-is");
        let windows = match windows {
            "match" => "Match mypaper",
            "light" => "Light",
            "dark" => "Dark",
            _ => "As is",
        };
        row(
            b,
            c,
            Some(glyph::APP),
            "Windows",
            windows,
            "",
            None,
            open(Page::Windows),
        );
        let connections = crate::pages::integrations::LIST
            .iter()
            .filter(|i| crate::pages::integrations::enabled(app, i))
            .map(|i| i.name)
            .collect::<Vec<_>>()
            .join(", ");
        row(
            b,
            c,
            Some(glyph::INTEGRATIONS),
            "Connections",
            &connections,
            "",
            None,
            open(Page::Connections),
        );
        row(
            b,
            c,
            Some(glyph::SETTINGS),
            "Settings",
            "",
            "",
            None,
            open(Page::Settings),
        );
    });
}

fn widgets(app: &App, b: &mut B, c: &mut Col) {
    let layout = crate::pages::widgets::layout(app);
    let items = crate::pages::widgets::here(app, &layout);
    let add = Rect::new(
        c.right() - b.px(60.0),
        c.y + b.px(2.0),
        b.px(60.0),
        b.px(28.0),
    );
    button(
        b,
        add,
        "Add",
        Some(glyph::ADD),
        false,
        true,
        open(Page::AddWidget),
    );
    title(
        b,
        c,
        "Widgets",
        &format!("Display {} · {} widgets", app.nav.display + 1, items.len()),
    );
    super::controls::displays(app, b, c);
    group(b, c, "On this display", |b, c| {
        if items.is_empty() {
            notice(b, c, "No widgets on this display yet.");
        }
        for instance in &items {
            let name = app
                .registry
                .get(&instance.widget)
                .map_or(instance.widget.as_str(), |w| w.manifest.name.as_str());
            row(
                b,
                c,
                Some(crate::pages::widgets::glyph_of(&instance.widget)),
                name,
                "",
                &format!(
                    "{} × {} · {}",
                    instance.size[0],
                    instance.size[1],
                    super::editors::position(app, &layout, instance)
                ),
                None,
                Action::SelectWidget(Some(instance.instance.clone())),
            );
        }
    });
    group(b, c, "", |b, c| {
        setting(
            app,
            b,
            c,
            "always_on_top",
            "Always on top",
            "Keeps widgets above your windows",
        )
    });
    if more(app, b, c, "Look") {
        group(b, c, "Look", |b, c| {
            row(
                b,
                c,
                Some(glyph::LOOK),
                "How widgets look",
                "From Look",
                "",
                None,
                open(Page::Look),
            )
        });
    }
}

fn bars(app: &App, b: &mut B, c: &mut Col) {
    title(
        b,
        c,
        "Bars",
        &format!("Top bar and dock · Display {}", app.nav.display + 1),
    );
    super::controls::displays(app, b, c);
    let current = app
        .get(DocId::Bars, "/preset")
        .and_then(Value::as_str)
        .unwrap_or("monochrome");
    group(b, c, "Design", |b, c| {
        for &(id, name) in &crate::pages::bars::PRESETS {
            super::editors::design_row(b, c, id, name, current == id, Action::Preset(id));
        }
    });
    group(b, c, "", |b, c| {
        for (bar, page, id, label) in [
            (Bar::Top, Page::Top, "top_enabled", "Top bar"),
            (Bar::Dock, Page::Dock, "dock_enabled", "Dock"),
        ] {
            let s = crate::settings::Setting::get(id).expect("bar switch");
            let on = app.value(s).as_bool().unwrap_or(true);
            row(
                b,
                c,
                None,
                label,
                &format!(
                    "{} · {}",
                    if on { "On" } else { "Off" },
                    crate::pages::bars::design_name(app, bar)
                ),
                "",
                None,
                open(page),
            );
        }
        setting(app, b, c, "taskbar_mode", "Windows' taskbar", "");
    });
    if more(app, b, c, "Panels, per display") {
        group(b, c, "", |b, c| {
            row(
                b,
                c,
                Some(glyph::TOGGLES),
                "Panels",
                "",
                "Start, sound, calendar, tray and notifications",
                None,
                open(Page::Panels),
            )
        });
        group(b, c, "Windows' taskbar", |b, c| {
            setting(app, b, c, "taskbar_alignment", "Icons", "")
        });
    }
}

fn settings(_app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Settings", "mypaper and Studio");
    group(b, c, "", |b, c| {
        row(
            b,
            c,
            Some(glyph::SLIDERS),
            "General",
            "",
            "Startup, Studio, history",
            None,
            open(Page::General),
        );
        row(
            b,
            c,
            Some(glyph::GAUGE),
            "Performance and power",
            "",
            "Graphics card, frame rate, when to rest",
            None,
            open(Page::Performance),
        );
        for (icon, page, caption) in [
            (glyph::KEYBOARD, Page::Keyboard, "All shortcuts"),
            (
                glyph::WRENCH,
                Page::Troubleshoot,
                "Parts, logs and recovery",
            ),
            (glyph::INFO, Page::About, "Version, licenses and credits"),
        ] {
            row(
                b,
                c,
                Some(icon),
                page.title(),
                "",
                caption,
                None,
                open(page),
            );
        }
    });
}

fn performance(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Performance and power", "");
    // This is the existing Studio process sampler and configured frame cap, not measured GPU
    // utilization or measured fps (neither is exposed by its current model).
    notice(
        b,
        c,
        &if app.system.running() {
            format!("Now: {}", metrics(app))
        } else {
            "Now: mypaper is off".into()
        },
    );
    let current = app
        .get(DocId::Config, "/engine/gpu")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let gpus = app.system.gpus.as_deref().unwrap_or_default();
    let integrated = gpus
        .iter()
        .find(|(_, i)| *i)
        .map_or("None found", |(n, _)| n.as_str());
    let discrete = gpus
        .iter()
        .find(|(_, i)| !*i)
        .map_or("None found", |(n, _)| n.as_str());
    let edit = Edit::new(DocId::Config, "/engine/gpu", "Graphics card");
    group(b, c, "Graphics card", |b, c| {
        for (id, label, caption, icon) in [
            ("integrated", "Integrated", integrated, glyph::CHIP),
            (
                "high-performance",
                "High performance",
                discrete,
                glyph::GAUGE,
            ),
            (
                "auto",
                "Let Windows choose",
                app.system
                    .display_gpu
                    .as_deref()
                    .unwrap_or("The card your screens use"),
                glyph::MONITOR,
            ),
        ] {
            row(
                b,
                c,
                Some(icon),
                label,
                "",
                caption,
                Some(current == id),
                Action::Set(edit.clone(), json!(id)),
            );
        }
    });
    group(b, c, "Frame rate", |b, c| {
        setting(app, b, c, "fps", "Cap", "")
    });
    group(b, c, "When to rest", |b, c| {
        setting(
            app,
            b,
            c,
            "pause_on_battery",
            "On battery",
            "Pause the wallpaper",
        );
        setting(
            app,
            b,
            c,
            "pause_on_fullscreen",
            "Behind fullscreen",
            "Games and videos get the GPU",
        );
        setting(app, b, c, "save_power", "Save power", "");
        setting(app, b, c, "reduce_motion", "Reduce motion", "");
    });
}

pub fn toast(app: &App, b: &mut B, window: Rect) {
    let Some(toast) = app.toast.as_ref() else {
        return;
    };
    let r = Rect::new(
        b.px(16.0),
        window.bottom - b.px(60.0 + super::editors::footer_height(app)),
        window.width() - b.px(32.0),
        b.px(44.0),
    );
    b.fill(r, b.px(10.0), b.p.fill);
    let action_width = if toast.action.is_some() {
        b.px(68.0)
    } else {
        0.0
    };
    text(
        b,
        Rect::new(
            r.left + b.px(12.0),
            r.top,
            r.width() - b.px(24.0) - action_width,
            r.height(),
        ),
        &toast.text,
        12.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    if let Some((label, action)) = &toast.action {
        button(
            b,
            Rect::new(
                r.right - action_width,
                r.top + b.px(8.0),
                action_width - b.px(8.0),
                b.px(28.0),
            ),
            label,
            None,
            false,
            true,
            action.clone(),
        );
    }
}
