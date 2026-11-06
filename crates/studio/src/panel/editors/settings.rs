use mypaper_config::desktop_menu::{Builtin, Entry, default_items};
use mypaper_ui::Rect;
use mypaper_ui::controls::Align;
use serde_json::Value;

use super::*;
use crate::panel::controls::setting;

const RESET_PARTS: &[(&str, DocId, Option<&str>)] = &[
    ("Bars", DocId::Bars, None),
    ("Widgets", DocId::Widgets, None),
    ("Look", DocId::Config, Some("/look")),
    ("Windows", DocId::Config, Some("/look/windows")),
    ("Desktop menu", DocId::Config, Some("/desktop_menu")),
    ("Built-in connections", DocId::Integrations, None),
    ("App connections", DocId::Plugins, None),
    ("Settings", DocId::Config, None),
    ("Studio", DocId::Studio, None),
];

pub fn frame_cap(app: &App, b: &mut B, c: &mut Col, path: &str, inherit: &str) {
    let value = app.get(DocId::Config, path).cloned().unwrap_or(Value::Null);
    let custom_key = key(("panel-custom-fps", path));
    let custom = app.is_open(custom_key);
    let refresh = crate::panel::measure::refresh_rate(app);
    let mut options = vec![(Value::Null, inherit.into())];
    if let Some(hz) = refresh {
        options.push((serde_json::json!(hz), format!("Display refresh · {hz} fps")));
    }
    for fps in [24, 30, 60, 120] {
        if refresh != Some(fps) {
            options.push((serde_json::json!(fps), format!("{fps} fps")));
        }
    }
    let known = options.iter().any(|(v, _)| *v == value);
    options.push((serde_json::json!("custom"), "Custom…".into()));
    segments(
        b,
        c,
        "Cap",
        if custom || !known {
            serde_json::json!("custom")
        } else {
            value.clone()
        },
        Edit::new(
            DocId::Studio,
            format!("/panel/frame-cap{path}"),
            "Frame rate",
        ),
        options,
    );
    if custom || !known {
        let current = value
            .as_f64()
            .unwrap_or_else(|| refresh.unwrap_or(60) as f64);
        crate::panel::material::row(
            b,
            c,
            "Custom cap",
            current,
            (1.0, 360.0, 1.0),
            &crate::panel::controls::unit(current, "fps"),
            Edit::new(DocId::Config, path, "Custom frame rate"),
        );
    }
}

pub fn intercept_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if edit.doc != DocId::Studio {
        return false;
    }
    if edit.path.as_ref() == "/panel/start-over" {
        if let Some((_, doc, path)) = value.as_u64().and_then(|i| RESET_PARTS.get(i as usize)) {
            if app.docs.doc(*doc).issue.is_some() {
                app.run(Action::StartOver(*doc));
            } else {
                app.run(Action::Panel(Command::ResetPart(*doc, *path)));
            }
        }
        return true;
    }
    let Some(path) = edit.path.strip_prefix("/panel/frame-cap") else {
        return false;
    };
    let k = key(("panel-custom-fps", path));
    if value == "custom" {
        app.nav.more.insert(k);
    } else {
        app.nav.more.remove(&k);
        app.set(&Edit::new(DocId::Config, path, "Frame rate"), value.clone());
    }
    true
}

#[cfg(test)]
mod frame_cap_tests {
    use super::*;

    #[test]
    fn custom_frame_rate_is_staged_and_the_slider_spans_the_supported_range() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let before = app.docs.version;
        app.set(
            &Edit::new(DocId::Studio, "/panel/frame-cap/engine/fps", "Cap"),
            serde_json::json!("custom"),
        );
        assert_eq!(app.docs.version, before);
        app.panel.as_mut().unwrap().page = Page::Performance;
        let built = crate::panel::build(&app, &mut ui.text);
        assert!(built.targets.iter().any(|target| matches!(&target.behavior, Behavior::Slider { min: 1.0, max: 360.0, .. }) && matches!(&target.action, Action::Slider(edit) if edit.path.as_ref() == "/engine/fps")));
        app.set(
            &Edit::new(DocId::Config, "/engine/fps", "Cap"),
            serde_json::json!(144),
        );
        assert_eq!(
            app.get(DocId::Config, "/engine/fps"),
            Some(&serde_json::json!(144))
        );
        app.run(Action::Undo);
        assert_eq!(app.docs.undo_count(), 0);
    }
}

pub fn general(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "General", "mypaper and Studio");
    group(b, c, "Startup", |b, c| {
        let on = app.system.autostart.as_ref().is_some_and(|a| a.enabled);
        toggle(
            b,
            c,
            "Start with Windows",
            "When you sign in",
            on,
            Action::ToggleAutostart(!on),
        );
        setting(
            app,
            b,
            c,
            "static_snapshot",
            "Wallpaper at sign-in",
            "A still while Windows starts",
        );
    });
    group(b, c, "Studio", |b, c| {
        setting(app, b, c, "studio_appearance", "Appearance", "");
        let left = app
            .get(DocId::Studio, "/panel_side")
            .and_then(Value::as_str)
            == Some("left");
        text(
            b,
            Rect::new(c.x, c.y, c.w, height(b, 24.0)),
            "Panel side",
            13.0,
            400,
            b.p.ink1,
            Align::Start,
        );
        c.y += height(b, 28.0);
        let rect = Rect::new(c.x, c.y, c.w, height(b, 30.0));
        b.segmented(
            key("panel-side"),
            rect,
            &["Left", "Right"],
            Some(usize::from(!left)),
            "Panel side",
            true,
            |i| Action::Panel(Command::Side(i == 0)),
        );
        b.stroke(rect, b.px(7.0), b.px(1.0), b.p.edge);
        c.y = rect.bottom + b.px(8.0);
    });
    group(b, c, "History", |b, c| {
        select(
            b,
            c,
            "Keep",
            app.get(DocId::Studio, "/panel_history_days")
                .cloned()
                .unwrap_or(serde_json::json!(30)),
            Edit::new(DocId::Studio, "/panel_history_days", "Keep history"),
            [(7, "7 days"), (30, "30 days"), (90, "90 days")]
                .into_iter()
                .map(|(v, l)| (serde_json::json!(v), l.into()))
                .collect(),
        );
        notice(b, c, "Up to 100 desktops, saved between Studio sessions.");
        action_button(b, c, "Open History", open(Page::History));
        let r = Rect::new(c.x, c.y + b.px(4.0), c.w, b.px(30.0));
        b.button(
            key("panel-clear-history"),
            r,
            "Clear history",
            None,
            mypaper_ui::controls::ButtonKind::Ghost,
            app.panel.as_ref().unwrap().history.borrow().error.is_none(),
            Action::Panel(Command::ClearHistory),
        );
        c.y = r.bottom + b.px(6.0);
    });
}

pub fn keyboard(b: &mut B, c: &mut Col) {
    title(
        b,
        c,
        "Keyboard",
        "Reference · shortcuts cannot be changed yet",
    );
    for (heading, shortcuts) in [
        (
            "Anywhere",
            &[
                ("Restore Windows desktop", "Ctrl Alt Shift Backspace"),
                ("Arrange widgets", "Ctrl Alt Shift W"),
                ("Widgets above windows", "Ctrl Alt Shift O"),
            ][..],
        ),
        (
            "In Studio",
            &[
                ("Home", "Ctrl 0 / Ctrl Home"),
                ("Peek (hold)", "Ctrl D"),
                ("Search everything", "Ctrl K"),
                ("Search this page", "Ctrl F"),
                ("Wallpapers", "Ctrl 1"),
                ("Look", "Ctrl 2"),
                ("Bars", "Ctrl 3"),
                ("Widgets", "Ctrl 4"),
                ("Desktop menu", "Ctrl 5"),
                ("Windows", "Ctrl 6"),
                ("Connections", "Ctrl 7"),
                ("Settings", "Ctrl ,"),
                ("History", "Ctrl H"),
                ("Undo", "Ctrl Z"),
                ("Redo", "Ctrl Y / Ctrl Shift Z"),
                ("Duplicate widget", "Ctrl Shift D"),
                ("Done", "Ctrl W"),
                ("Next / previous control", "Tab / Shift Tab"),
                ("Next / previous region", "F6 / Shift F6"),
                ("Open or apply", "Enter / Space"),
                ("Back / leave typing", "Esc"),
                ("Change slider", "← / →"),
                ("Slider minimum / maximum", "Home / End"),
            ][..],
        ),
        (
            "Selected widget",
            &[
                ("Move one cell", "Arrow keys"),
                ("Resize one cell", "Shift + arrows"),
                ("Remove widget", "Delete"),
            ][..],
        ),
        ("Lists", &[("Move selected row", "Alt ↑ / Alt ↓")][..]),
    ] {
        group(b, c, heading, |b, c| {
            for (label, keys) in shortcuts {
                let h = height(b, 36.0);
                let style = crate::panel::controls::font(b, 10.5, 500);
                let key_w = b.measure(keys, &style).0.min(c.w * 0.58);
                text(
                    b,
                    Rect::new(c.x, c.y, (c.w - key_w - b.px(12.0)).max(1.0), h),
                    label,
                    12.0,
                    400,
                    b.p.ink1,
                    Align::Start,
                );
                text(
                    b,
                    Rect::new(c.right() - key_w, c.y, key_w, h),
                    keys,
                    10.5,
                    500,
                    b.p.ink3,
                    Align::End,
                );
                c.y += h;
            }
        });
    }
}

pub fn troubleshoot(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Troubleshoot", "Status and recovery");
    group(b, c, "mypaper", |b, c| {
        notice(
            b,
            c,
            if app.system.running() {
                "Running · changes apply live"
            } else {
                "Off · changes apply when mypaper starts"
            },
        );
        action_button(
            b,
            c,
            if app.system.running() {
                "Stop mypaper"
            } else {
                "Start mypaper"
            },
            if app.system.running() {
                Action::StopMypaper
            } else {
                Action::StartMypaper
            },
        );
    });
    group(b, c, "Parts", |b, c| {
        if let Some(memory) = crate::panel::measure::memory(app.fixture) {
            row(
                b,
                c,
                Some(glyph::APP),
                "Studio",
                "",
                &format!(
                    "{:.0} MB working set \u{b7} {:.0} MB private",
                    memory.working_mb, memory.private_mb
                ),
                None,
                Action::None,
            );
            notice(b, c, "Software rendering");
        }
        for ((id, _), part) in crate::system::PARTS.iter().zip(app.system.parts) {
            let label = match *id {
                "core" => "Core",
                "engine" => "Engine",
                _ => "Shell",
            };
            let detail = if part.running {
                format!("Running · {:.0} MB · {:.1}% CPU", part.memory, part.cpu)
            } else {
                "Stopped".into()
            };
            let r = Rect::new(c.x, c.y, c.w, height(b, 58.0));
            text(
                b,
                Rect::new(r.left, r.top, r.width() - b.px(104.0), height(b, 28.0)),
                label,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            text(
                b,
                Rect::new(
                    r.left,
                    r.top + height(b, 28.0),
                    r.width() - b.px(104.0),
                    height(b, 22.0),
                ),
                &detail,
                11.0,
                400,
                b.p.ink3,
                Align::Start,
            );
            button(
                b,
                Rect::new(
                    r.right - b.px(98.0),
                    r.top + b.px(12.0),
                    b.px(98.0),
                    b.px(30.0),
                ),
                &format!("Restart {label}"),
                None,
                false,
                true,
                Action::Restart(id),
            );
            c.y = r.bottom;
        }
    });
    group(b, c, "Recovery", |b, c| {
        notice(
            b,
            c,
            "Ctrl Alt Shift Backspace restores the Windows desktop from anywhere.",
        );
        action_button(b, c, "Restore Windows desktop", Action::RestoreDesktop);
        let options = RESET_PARTS
            .iter()
            .enumerate()
            .filter(|(_, (_, doc, _))| {
                !matches!(
                    app.docs.doc(*doc).issue,
                    Some(crate::docs::ReadIssue::Newer | crate::docs::ReadIssue::Unavailable)
                )
            })
            .map(|(index, (name, _, _))| (serde_json::json!(index), (*name).to_owned()))
            .collect();
        row(
            b,
            c,
            Some(glyph::RESET),
            "Start over…",
            "",
            "Choose a part · A backup is kept",
            None,
            Action::Choose(Choices {
                edit: Edit::new(DocId::Studio, "/panel/start-over", "Start over"),
                options,
                current: None,
            }),
        );
    });
    group(b, c, "Help", |b, c| {
        action_button(b, c, "Copy diagnostics", Action::CopyDiagnostics);
        action_button(b, c, "Open logs", Action::OpenLogs);
        let how = app
            .system
            .autostart
            .as_ref()
            .and_then(|a| a.method.as_deref())
            .unwrap_or("Not configured");
        notice(b, c, &format!("How it starts: {how}"));
        if let Some(path) = app.system.core_path.as_ref().and_then(|p| p.parent()) {
            notice(b, c, &format!("Installed at: {}", path.display()));
        }
    });
}

pub fn about(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "About", "mypaper Studio");
    group(b, c, "mypaper", |b, c| {
        notice(b, c, &format!("Version {}", env!("CARGO_PKG_VERSION")));
        notice(
            b,
            c,
            "MIT licensed. A native desktop, built with Rust and Direct2D.",
        );
        notice(
            b,
            c,
            "Studio keeps your desktop settings on this PC. Connections describe their own data use.",
        );
    });
    group(b, c, "Wallpaper credits", |b, c| {
        for (i, wallpaper) in app
            .library
            .items
            .iter()
            .enumerate()
            .filter(|(_, w)| w.credit.is_some())
        {
            let credit = wallpaper.credit.as_ref().unwrap();
            let detail = [credit.author.as_deref(), credit.license.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            row(
                b,
                c,
                Some(glyph::WALLPAPERS),
                &wallpaper.name,
                "",
                &detail,
                None,
                Action::Panel(Command::Wallpaper(i)),
            );
        }
    });
}

fn menu_name(entry: &Entry) -> String {
    match entry {
        Entry::Custom(custom) => custom.label.clone(),
        Entry::Builtin(builtin) => match builtin {
            Builtin::Separator => "Separator",
            Builtin::View => "View",
            Builtin::Sort => "Sort by",
            Builtin::Refresh => "Refresh",
            Builtin::New => "New",
            Builtin::Paste => "Paste",
            Builtin::Wallpaper => "Change wallpaper",
            Builtin::Widgets => "Edit widgets",
            Builtin::Display => "Display settings",
            Builtin::Personalize => "Personalize",
            Builtin::Terminal => "Open terminal here",
            Builtin::More => "Show more options",
        }
        .into(),
    }
}

pub fn add_menu_item(app: &mut App) {
    let label = app
        .text(&FieldId::Text(DocId::Studio, "/menu_label".into()))
        .trim()
        .to_owned();
    let command = app
        .text(&FieldId::Text(DocId::Studio, "/menu_command".into()))
        .trim()
        .to_owned();
    if label.is_empty() || command.is_empty() {
        app.alert("Give your item a name and a program");
        return;
    }
    let args = match crate::library::split_args(app.text(&FieldId::Text(
        DocId::Studio,
        "/panel/draft/menu_args".into(),
    ))) {
        Ok(args) => args,
        Err(error) => {
            app.alert(error.to_string());
            return;
        }
    };
    let mut items: Vec<Entry> = app
        .get(DocId::Config, "/desktop_menu/items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(default_items);
    items.push(Entry::Custom(mypaper_config::desktop_menu::Custom {
        label,
        command,
        args,
    }));
    app.set(
        &Edit::new(DocId::Config, "/desktop_menu/items", "Add menu item"),
        serde_json::to_value(items).unwrap(),
    );
    if !matches!(app.docs.saved, crate::docs::Saved::Failed(_)) {
        for path in ["/menu_label", "/menu_command", "/panel/draft/menu_args"] {
            app.texts.remove(&FieldId::Text(DocId::Studio, path.into()));
        }
    }
}

pub fn desktop_menu(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Desktop menu", "Right-click on bare wallpaper");
    group(b, c, "", |b, c| {
        setting(app, b, c, "desktop_menu", "Use mypaper's menu", "")
    });
    let items: Vec<Entry> = app
        .get(DocId::Config, "/desktop_menu/items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(default_items);
    menu_preview(app, b, c, &items);
    group(b, c, "Items · top to bottom", |b, c| {
        for (i, item) in items.iter().enumerate() {
            crate::panel::lists::row(
                app,
                b,
                c,
                &crate::panel::SortList::Menu,
                i,
                &menu_name(item),
                *item == Entry::Builtin(Builtin::Separator),
                None,
                None,
            );
            if let Entry::Custom(custom) = item
                && app.is_open(key(("panel-menu-item", i)))
            {
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(
                        DocId::Config,
                        format!("/desktop_menu/items/{i}/label").into(),
                    ),
                    "Name",
                    "Menu item",
                    &custom.label,
                );
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(
                        DocId::Config,
                        format!("/desktop_menu/items/{i}/command").into(),
                    ),
                    "Program",
                    "notepad.exe",
                    &custom.command,
                );
                let args = custom
                    .args
                    .iter()
                    .map(|arg| {
                        if arg.contains(' ') {
                            format!("\"{arg}\"")
                        } else {
                            arg.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(DocId::Studio, format!("/panel/menu_args/{i}").into()),
                    "Arguments",
                    "\"D:\\My notes.txt\"",
                    &args,
                );
            }
        }
    });
    if crate::panel::controls::more(app, b, c, "Add item") {
        group(b, c, "", |b, c| {
            let mut shown = Vec::new();
            for item in default_items().into_iter() {
                if shown.contains(&item)
                    || items.contains(&item) && item != Entry::Builtin(Builtin::Separator)
                {
                    continue;
                }
                shown.push(item.clone());
                if let Entry::Builtin(builtin) = &item {
                    let id = serde_json::to_value(builtin)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap();
                    row(
                        b,
                        c,
                        Some(glyph::ADD),
                        &menu_name(&item),
                        "",
                        "",
                        None,
                        Action::MenuAdd(id),
                    );
                }
            }
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Studio, "/menu_label".into()),
                "Your own item's name",
                "Notes",
                "",
            );
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Studio, "/menu_command".into()),
                "Program",
                "notepad.exe",
                "",
            );
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Studio, "/panel/draft/menu_args".into()),
                "Arguments",
                "\"D:\\My notes.txt\"",
                "",
            );
            action_button(b, c, "Add your item", Action::Panel(Command::AddMenuItem));
            action_button(b, c, "Reset to the default menu", Action::MenuReset);
        });
    }
    group(b, c, "Explorer", |b, c| {
        setting(app, b, c, "classic_menu", "Full Windows menus", "")
    });
}

/// The shell's actual menu drawing, reduced over the active wallpaper.
fn menu_preview(app: &App, b: &mut B, c: &mut Col, items: &[Entry]) {
    let r = Rect::new(c.x, c.y + b.px(12.0), c.w, b.px(160.0));
    crate::panel::specimens::backdrop(app, b, r);
    let config = mypaper_config::DesktopMenu {
        enabled: true,
        items: items.to_vec(),
    };
    let items = mypaper_shell::preview::menu::build(
        &config,
        mypaper_shell::preview::menu::ViewState {
            icons_visible: true,
            ..Default::default()
        },
    );
    let mut tokens = mypaper_shell::preview::menu::tokens(
        &crate::app::theme_name(&app.docs).unwrap_or_default(),
        b.text,
    );
    tokens.shadow = false;
    let layout = mypaper_ui::menu::layout(&items, &tokens, b.s, b.text);
    let list = mypaper_ui::menu::view(&items, &layout, None, &tokens);
    b.list.ops.push(mypaper_ui::Op::Clip { rect: r });
    b.list.ops.extend(crate::stage::offset(
        &list.ops,
        (r.left + (r.width() - layout.width) / 2.0).round(),
        (r.top + b.px(8.0)).round(),
    ));
    b.list.ops.push(mypaper_ui::Op::Unclip);
    c.y = r.bottom + b.px(4.0);
}
