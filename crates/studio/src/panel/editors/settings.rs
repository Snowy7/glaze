use mypaper_config::desktop_menu::{Builtin, Entry, default_items};
use mypaper_ui::Rect;
use mypaper_ui::controls::Align;
use serde_json::Value;

use super::*;
use crate::panel::controls::{icon_button, setting};

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
        for (name, side) in [("Panel on the left", true), ("Panel on the right", false)] {
            row(
                b,
                c,
                None,
                name,
                "",
                "",
                Some(left == side),
                Action::Panel(Command::Side(side)),
            );
        }
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
        action_button(b, c, "Clear history", Action::Panel(Command::ClearHistory));
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
                text(
                    b,
                    Rect::new(c.x, c.y, c.w, height(b, 23.0)),
                    label,
                    13.0,
                    400,
                    b.p.ink1,
                    Align::Start,
                );
                text(
                    b,
                    Rect::new(c.x, c.y + height(b, 23.0), c.w, height(b, 20.0)),
                    keys,
                    12.0,
                    500,
                    b.p.ink3,
                    Align::Start,
                );
                c.y += height(b, 50.0);
            }
        });
    }
    notice(b, c, "Peek (Ctrl D) is not available yet.");
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
            row(
                b,
                c,
                Some(glyph::CHIP),
                label,
                "Restart",
                &detail,
                None,
                Action::Restart(id),
            );
        }
    });
    group(b, c, "Recovery", |b, c| {
        notice(
            b,
            c,
            "Ctrl Alt Shift Backspace restores the Windows desktop from anywhere.",
        );
        action_button(b, c, "Restore Windows desktop", Action::RestoreDesktop);
        for (name, doc) in [
            ("Start over: Bars", DocId::Bars),
            ("Start over: Widgets", DocId::Widgets),
            ("Start over: Look", DocId::Config),
        ] {
            row(
                b,
                c,
                Some(glyph::RESET),
                name,
                "",
                "A backup is kept",
                None,
                Action::StartOver(doc),
            );
        }
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

pub fn desktop_menu(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Desktop menu", "Right-click on bare wallpaper");
    group(b, c, "", |b, c| {
        setting(app, b, c, "desktop_menu", "Use mypaper's menu", "")
    });
    let items: Vec<Entry> = app
        .get(DocId::Config, "/desktop_menu/items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(default_items);
    group(b, c, "Items · top to bottom", |b, c| {
        for (i, item) in items.iter().enumerate() {
            let h = height(b, 40.0);
            let r = Rect::new(c.x, c.y, c.w, h);
            let label = menu_name(item);
            let item_key = mypaper_ui::controls::key(("menu-item", i));
            let select = Rect::new(r.left, r.top, r.width() - b.px(90.0), r.height());
            b.hit(
                item_key,
                select,
                mypaper_ui::Role::ListItem,
                &label,
                Action::Panel(Command::MenuItem(i)),
                mypaper_ui::controls::Behavior::Click,
            );
            b.focus_ring(item_key, select, b.px(6.0));
            b.icon(
                Rect::new(c.x, c.y, b.px(14.0), h),
                glyph::GRIP,
                12.0,
                b.p.ink3,
            );
            text(
                b,
                Rect::new(c.x + b.px(20.0), c.y, c.w - b.px(112.0), h),
                &label,
                13.0,
                400,
                b.p.ink1,
                Align::Start,
            );
            // Keep keyboard and visible controls equally complete, including separators.
            for (n, icon, name, enabled, action) in [
                (
                    0,
                    mypaper_ui::icons::Icon::ChevronUp.into(),
                    format!("Move {label} {i} up"),
                    i > 0,
                    Action::MenuMove(i, -1),
                ),
                (
                    1,
                    glyph::CHEVRON_DOWN,
                    format!("Move {label} {i} down"),
                    i + 1 < items.len(),
                    Action::MenuMove(i, 1),
                ),
                (
                    2,
                    glyph::DELETE,
                    format!("Remove {label} {i}"),
                    true,
                    Action::MenuRemove(i),
                ),
            ] {
                icon_button(
                    b,
                    Rect::new(
                        r.right - b.px(84.0) + n as f32 * b.px(28.0),
                        r.top + (h - b.px(28.0)) / 2.0,
                        b.px(26.0),
                        b.px(28.0),
                    ),
                    icon,
                    &name,
                    enabled,
                    action,
                );
            }
            c.y += h;
            b.hline(c.x, c.right(), c.y - 1.0, b.p.line);
        }
    });
    group(b, c, "Add", |b, c| {
        let mut shown = Vec::new();
        for item in default_items()
            .into_iter()
            .filter(|e| !items.contains(e) || *e == Entry::Builtin(Builtin::Separator))
        {
            if shown.contains(&item) {
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
        action_button(b, c, "Add your item", Action::MenuAdd("custom".into()));
        action_button(b, c, "Reset to the default menu", Action::MenuReset);
    });
    group(b, c, "Explorer", |b, c| {
        setting(app, b, c, "classic_menu", "Full Windows menus", "")
    });
}
