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
    // Creation has one exit in its footer. First run has no header chrome.
    if page == Page::FirstRun {
        return;
    }
    let committing = matches!(
        page,
        Page::AddWidget | Page::AddModule | Page::AddWallpaper
    );
    let done_label = "Done";
    let done_style = ui::font(b, 13.0, 500);
    let done_width = (b.measure(done_label, &done_style).0 + b.px(20.0)).max(b.px(52.0));
    let done = Rect::new(
        r.right - done_width - b.px(8.0),
        b.px(10.0),
        done_width,
        b.px(28.0),
    );
    if !committing {
        button(
            b,
            done,
            "Done",
            None,
            true,
            true,
            Action::Panel(Command::Done),
        );
    }
    let mut x = if committing { r.right - b.px(8.0) } else { done.left - b.px(6.0) };
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
        let issue = super::controls::relevant_docs(page)
            .iter()
            .find_map(|id| app.docs.doc(*id).issue);
        let status = if let Some(issue) = issue {
            match issue {
                crate::docs::ReadIssue::Newer => "Newer",
                crate::docs::ReadIssue::Unavailable => "Can't read",
                crate::docs::ReadIssue::Malformed { .. } => "Read only",
            }
        } else if !app.panel.as_ref().unwrap().save_errors.borrow().is_empty()
            || !app.panel.as_ref().unwrap().theme_errors.borrow().is_empty()
        {
            ""
        } else if app.drag.is_some() {
            "Saving…"
        } else {
            match &app.docs.saved {
                crate::docs::Saved::Failed(_) => "",
                _ => "Saved",
            }
        };
        let announcement = super::controls::relevant_docs(page)
            .iter()
            .find_map(|id| app.docs.error(*id))
            .unwrap_or(status);
        b.announce(
            announcement,
            if app.drag.is_some() {
                0
            } else {
                app.docs.version
            },
        );
        let alert = matches!(status, "Read only" | "Can't read" | "Couldn't save");
        let size = if alert { 12.0 } else { 10.0 };
        let style = ui::font(b, size, if alert { 600 } else { 400 });
        let glyph_width = if alert { b.px(18.0) } else { 0.0 };
        let width = b.measure(status, &style).0 + b.px(8.0) + glyph_width;
        x -= width;
        if alert {
            b.icon(
                Rect::new(x, b.px(10.0), b.px(16.0), b.px(28.0)),
                mypaper_ui::icons::Icon::Warning,
                14.0,
                b.p.ink1,
            );
        }
        text(
            b,
            Rect::new(x + glyph_width, b.px(10.0), width - glyph_width, b.px(28.0)),
            status,
            size,
            if alert { 600 } else { 400 },
            if alert { b.p.ink1 } else { b.p.ink3 },
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
    let parent = if page == Page::FirstRun {
        None
    } else if page == Page::BarDesign {
        Some(match app.nav.bar {
            Some(Bar::Top) => Page::Top,
            Some(Bar::Dock) => Page::Dock,
            None => Page::Bars,
        })
    } else if matches!(page, Page::Module | Page::AddModule) && app.nav.bar == Some(Bar::Dock) {
        Some(Page::Dock)
    } else {
        page.parent()
    };
    let end = if let Some(parent) = parent {
        let parent_title = if parent == Page::Wallpapers {
            "Wallpapers"
        } else {
            parent.title()
        };
        let style = ui::font(b, 13.0, 500);
        let width = (b.measure(parent_title, &style).0 + b.px(24.0))
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
            parent_title,
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
            &format!("Back to {parent_title}"),
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
    if !editing {
        b.announce(
            "mypaper is off. Changes apply when it starts.",
            app.docs.version,
        );
    }
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
    let first_op = b.list.ops.len();
    let first_target = b.targets.len();
    match page {
        Page::Root => root(app, b, c),
        Page::Widgets => widgets(app, b, c),
        Page::Bars => bars(app, b, c),
        Page::Settings => settings(app, b, c),
        Page::Performance => performance(app, b, c),
        Page::Top | Page::Dock => super::editors::view(app, b, c, page),
        _ => super::editors::view(app, b, c, page),
    }
    super::saving::finish_page(app, b, page, first_op, first_target);
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

fn playback_status(app: &App) -> String {
    let stopped: Vec<_> = crate::system::PARTS
        .iter()
        .zip(app.system.parts)
        .filter(|(_, part)| !part.running)
        .map(|((id, _), _)| *id)
        .collect();
    if !stopped.is_empty() {
        return format!("{} stopped · {}", stopped.join(", "), metrics(app));
    }
    let Some(wallpaper) = app.library.active().map(|i| &app.library.items[i]) else {
        return "No wallpaper selected".into();
    };
    let loaded: Option<mypaper_theme::Theme> = serde_json::from_value(
        super::saving::theme_value(app, wallpaper).as_ref().clone(),
    ).ok();
    let engine: mypaper_config::EngineConfig = app
        .get(DocId::Config, "/engine")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let cap = engine
        .resolve(
            &wallpaper.id,
            loaded.as_ref().map_or_default(|theme| theme.engine),
        )
        .fps;
    let source = loaded
        .as_ref()
        .and_then(|theme| match &theme.wallpaper {
            mypaper_theme::Wallpaper::Shader(source) => Some(source.fps),
            mypaper_theme::Wallpaper::Video(source) => Some(source.fps),
            mypaper_theme::Wallpaper::Scene(source) => Some(source.fps),
            _ => None,
        });
    match (source, cap, wallpaper.kind) {
        (_, _, Some(crate::library::Kind::Image)) => "Still picture".into(),
        (Some(source), cap, _) => format!("{} fps", cap.map_or(source, |cap| cap.min(source))),
        (_, Some(cap), _) => format!("Up to {cap} fps"),
        (_, _, Some(crate::library::Kind::Web)) => "Frame rate set by the page".into(),
        (_, _, Some(crate::library::Kind::App)) => "Frame rate set by the app".into(),
        _ => "At the wallpaper's frame rate".into(),
    }
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
        &playback_status(app),
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
        let preset = super::editors::bar_design_name(app, None);
        row(
            b,
            c,
            Some(glyph::BARS),
            "Bars",
            &preset,
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
        let connected = crate::pages::integrations::LIST
            .iter()
            .filter(|item| crate::pages::integrations::enabled(app, item))
            .count()
            + app
                .plugins
                .installed
                .iter()
                .filter(|plugin| {
                    !app.plugins.preferences.disabled.contains(&plugin.id)
                        && app
                            .plugins
                            .snapshot
                            .plugins
                            .get(&plugin.id)
                            .is_some_and(|state| state.status != "Unloaded")
                })
                .count();
        let connections = format!("{connected} connected");
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
                super::editors::position(app, &layout, instance)
                    .split(", column")
                    .next()
                    .unwrap_or("On this display"),
                None,
                Action::SelectWidget(Some(instance.instance.clone())),
            );
            if let Some(hit) = b.list.hits.last_mut() {
                hit.label = format!(
                    "{name}, {}",
                    super::editors::position(app, &layout, instance)
                )
                .into();
            }
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
    super::editors::bars_overview(app, b, c);
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
        super::editors::frame_cap(app, b, c, "/engine/fps", "Wallpaper's frame rate")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(ui: &mypaper_ui::Ui) -> App {
        let mut app = crate::goldens::app(true, ui);
        app.panel = Some(super::super::State::default());
        app.size = (480, 760);
        app
    }

    #[test]
    fn creation_exits_live_in_the_footer_and_history_keeps_done() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        for page in [
            Page::AddWidget,
            Page::AddModule,
            Page::AddWallpaper,
            Page::FirstRun,
        ] {
            app.panel.as_mut().unwrap().page = page;
            let built = super::super::build(&app, &mut ui.text);
            assert!(
                !built
                    .list
                    .hits
                    .iter()
                    .any(|hit| hit.label.as_ref() == "Done"),
                "{page:?}"
            );
            assert!(
                built
                    .list
                    .hits
                    .iter()
                    .any(|hit| hit.label.as_ref() == "Cancel" && hit.rect.top >= app.size.1 as f32 - 64.0),
                "{page:?}"
            );
            if page == Page::FirstRun {
                assert!(
                    !built
                        .list
                        .hits
                        .iter()
                        .any(|hit| hit.label.starts_with("Back to "))
                );
            }
        }
        app.panel.as_mut().unwrap().page = Page::Root;
        super::super::history::observe(&app);
        app.run(Action::Preset("islands"));
        let last = app.panel.as_ref().unwrap().history.borrow().entries.len() - 1;
        app.panel.as_mut().unwrap().page = Page::History;
        app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(last);
        assert_eq!(super::super::editors::footer_height(&app), 0.0);
        app.panel.as_ref().unwrap().history.borrow_mut().selected = Some(0);
        assert_eq!(super::super::editors::footer_height(&app), 64.0);
        let built = super::super::build(&app, &mut ui.text);
        let cancel = built
            .targets
            .iter()
            .find(|target| target.action == Action::Panel(Command::Done))
            .unwrap()
            .action
            .clone();
        app.run(cancel);
        assert!(app.quit);
    }

    #[test]
    fn unreadable_files_have_readable_status_and_inert_editors_while_newer_stays_newer() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        for (source, label) in [
            ("{broken", "Read only"),
            ("{\"future_setting\":true}", "Newer"),
        ] {
            let mut app = fixture(&ui);
            app.docs = crate::docs::Docs::fixture([source, "{}", "{}", "{}"]);
            app.panel.as_mut().unwrap().page = Page::Look;
            let built = super::super::build(&app, &mut ui.text);
            assert!(built.list.ops.iter().any(|op| matches!(op, mypaper_ui::Op::Text { text, style, color, .. } if text.as_ref() == label && (label == "Newer" || style.size >= 12.0 && *color == super::super::palette_for(&app).ink1))));
            assert!(
                !built
                    .targets
                    .iter()
                    .any(|target| matches!(target.action, Action::SetStyle(_) | Action::Slider(_)))
            );
            assert!(built.list.hits.iter().any(|hit| !hit.enabled));
        }
    }

    #[test]
    fn widget_switcher_reports_only_the_selected_display_and_positions_keep_grid_details_accessible()
     {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        app.panel.as_mut().unwrap().page = Page::Widgets;
        let first = super::super::build(&app, &mut ui.text);
        let rows: Vec<_> = first
            .list
            .hits
            .iter()
            .filter(|hit| hit.label.contains("column "))
            .collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|hit| hit.label.contains("row ")));
        assert!(
            !first.list.ops.iter().any(
                |op| matches!(op, mypaper_ui::Op::Text { text, .. } if text.contains("column "))
            )
        );
        let version = app.docs.version;
        super::super::run(&mut app, Command::Display(1));
        let second = super::super::build(&app, &mut ui.text);
        assert!(
            !second
                .list
                .hits
                .iter()
                .any(|hit| hit.label.contains("column "))
        );
        assert_eq!(app.docs.version, version);
    }

    #[test]
    fn home_uses_the_wallpapers_frame_rate_and_honors_a_lower_user_cap() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        for wallpaper in &mut app.library.items {
            wallpaper.active = wallpaper.id == "rain-city";
        }
        assert_eq!(playback_status(&app), "30 fps");
        app.set(&Edit::new(DocId::Config, "/engine/fps", "Cap"), json!(12));
        assert_eq!(playback_status(&app), "12 fps");
    }
}
