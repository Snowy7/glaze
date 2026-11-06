use mypaper_ui::Rect;
use mypaper_ui::controls::Align;
use serde_json::{Value, json};

use super::*;
use crate::actions::{Bar, Panel, Pins, Slot};
use crate::panel::controls::{more, setting};

const MODULES: [(&str, &str); 15] = [
    ("start", "Start"),
    ("search", "Search"),
    ("workspaces", "Desktops"),
    ("app", "App name"),
    ("clock", "Date and clock"),
    ("media", "Now playing"),
    ("tray", "Tray"),
    ("network", "Network"),
    ("volume", "Volume"),
    ("battery", "Battery"),
    ("quick", "Quick settings"),
    ("power", "Power"),
    ("stats", "CPU and memory"),
    ("language", "Keyboard language"),
    ("notifications", "Notifications"),
];

fn name(id: &str) -> &str {
    MODULES
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(id.strip_prefix("custom:").unwrap_or(id), |(_, name)| *name)
}

pub fn bar(app: &App, b: &mut B, c: &mut Col, page: Page) {
    let bar = if page == Page::Dock {
        Bar::Dock
    } else {
        Bar::Top
    };
    title(
        b,
        c,
        page.title(),
        &format!(
            "{} · Display {}",
            crate::pages::bars::design_name(app, bar),
            app.nav.display + 1
        ),
    );
    group(b, c, "", |b, c| {
        let ids: &[&str] = if bar == Bar::Top {
            &["top_enabled", "top_edge", "top_height", "top_shape"]
        } else {
            &[
                "dock_enabled",
                "dock_style",
                "dock_edge",
                "dock_size",
                "dock_magnify",
                "dock_autohide",
            ]
        };
        for &id in ids {
            let s = crate::settings::Setting::get(id).unwrap();
            setting(
                app,
                b,
                c,
                id,
                if id.ends_with("enabled") {
                    "Show"
                } else {
                    s.label
                },
                "",
            );
        }
    });
    group(b, c, "Design · this bar only", |b, c| {
        let designs: &[(&str, &str)] = if bar == Bar::Top {
            &crate::pages::bars::TOP_DESIGNS
        } else {
            &crate::pages::bars::DOCK_DESIGNS
        };
        let path = format!("/{}/design", bar.key());
        let current = app
            .get(DocId::Bars, &path)
            .and_then(Value::as_str)
            .unwrap_or("monochrome");
        for &(id, label) in designs {
            design_row(
                b,
                c,
                id,
                label,
                current == id,
                Action::Set(
                    Edit::new(DocId::Bars, path.clone(), "Bar design"),
                    json!(id),
                ),
            );
        }
    });
    if bar == Bar::Dock {
        group(b, c, "Pinned apps", |b, c| {
            let own = app.docs.is_set(DocId::Bars, "/dock/pinned");
            row(
                b,
                c,
                None,
                "Windows' pins",
                "",
                "",
                Some(!own),
                Action::Unset(Edit::new(DocId::Bars, "/dock/pinned", "Windows' pins")),
            );
            row(
                b,
                c,
                None,
                "Your own pins",
                "",
                "",
                Some(own),
                Action::PinsOwn(Pins::Dock),
            );
            if own {
                let pins = app
                    .get(DocId::Bars, "/dock/pinned")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for (i, pin) in pins.iter().enumerate() {
                    let title = pin["name"]
                        .as_str()
                        .or(pin["path"].as_str())
                        .unwrap_or("App");
                    let r = Rect::new(c.x, c.y, c.w, b.px(38.0));
                    text(
                        b,
                        Rect::new(r.left, r.top, (r.width() - b.px(90.0)).max(0.0), r.height()),
                        title,
                        13.0,
                        400,
                        b.p.ink1,
                        Align::Start,
                    );
                    for (n, icon, label, enabled, action) in [
                        (
                            0,
                            mypaper_ui::icons::Icon::ChevronUp.into(),
                            format!("Move pin {i} up"),
                            i > 0,
                            Action::PinMove(Pins::Dock, i, -1),
                        ),
                        (
                            1,
                            glyph::CHEVRON_DOWN,
                            format!("Move pin {i} down"),
                            i + 1 < pins.len(),
                            Action::PinMove(Pins::Dock, i, 1),
                        ),
                        (
                            2,
                            glyph::DELETE,
                            format!("Remove pin {i}"),
                            true,
                            Action::PinRemove(Pins::Dock, i),
                        ),
                    ] {
                        crate::panel::controls::icon_button(
                            b,
                            Rect::new(
                                r.right - b.px(84.0) + n as f32 * b.px(28.0),
                                r.top + b.px(5.0),
                                b.px(26.0),
                                b.px(28.0),
                            ),
                            icon,
                            &label,
                            enabled,
                            action,
                        );
                    }
                    c.y = r.bottom;
                }
                action_button(b, c, "Pin an app", Action::PinAdd(Pins::Dock));
            }
        });
    }
    group(b, c, "Modules", |b, c| {
        for (list, label) in [("left", "Start"), ("center", "Middle"), ("right", "End")] {
            if bar == Bar::Dock && list == "center" {
                continue;
            }
            text(
                b,
                Rect::new(c.x, c.y, c.w, b.px(26.0)),
                label,
                12.0,
                600,
                b.p.ink3,
                Align::Start,
            );
            c.y += b.px(28.0);
            let modules = app
                .get(DocId::Bars, &format!("/{}/{list}", bar.key()))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (index, module) in modules.iter().enumerate() {
                let id = module.as_str().unwrap_or("Custom module");
                let slot = Slot { bar, list, index };
                row(
                    b,
                    c,
                    Some(glyph::GRIP),
                    name(id),
                    "",
                    "",
                    None,
                    Action::SelectModule(Some(slot)),
                );
            }
            if modules.is_empty() {
                notice(b, c, "No modules here");
            }
        }
        action_button(b, c, "Add a module", Action::AddModule(bar));
    });
    if more(app, b, c, "Layout, look, motion, displays") {
        let place = crate::settings::Place::Bars(Some(bar));
        let mut groups = Vec::new();
        for setting in crate::settings::SETTINGS
            .iter()
            .filter(|s| s.place == place)
        {
            if let Some(group) = setting.more
                && !groups.contains(&group)
            {
                groups.push(group);
            }
        }
        for heading in groups {
            group(b, c, heading, |b, c| {
                for s in crate::settings::SETTINGS.iter().filter(|s| {
                    s.place == place
                        && s.more == Some(heading)
                        && !matches!(s.kind, crate::settings::Kind::Custom)
                }) {
                    setting(app, b, c, s.id, s.label, "");
                }
            });
        }
        action_button(
            b,
            c,
            "Follow the design's look and motion",
            Action::ResetBar(bar),
        );
    }
}

pub fn module(app: &App, b: &mut B, c: &mut Col) {
    let Some(slot) = &app.nav.module else { return };
    let list = app
        .get(DocId::Bars, &format!("/{}/{}", slot.bar.key(), slot.list))
        .and_then(Value::as_array);
    let Some(id) = list.and_then(|l| l.get(slot.index)).and_then(Value::as_str) else {
        return;
    };
    title(
        b,
        c,
        name(id),
        &format!(
            "{} · {}",
            if slot.bar == Bar::Top {
                "Top bar"
            } else {
                "Dock"
            },
            match slot.list {
                "left" => "Start",
                "center" => "Middle",
                _ => "End",
            }
        ),
    );
    let base = format!("/modules/{}", crate::docs::token(id));
    if id == "clock" {
        group(b, c, "Shows", |b, c| {
            setting(app, b, c, "top_clock_seconds", "Seconds", "");
            setting(app, b, c, "top_clock_format", "Format", "");
        });
    }
    group(b, c, "Clicks", |b, c| {
        for (trigger, label) in [
            ("left", "Click"),
            ("middle", "Middle click"),
            ("right", "Right click"),
            ("wheel_up", "Scroll up"),
            ("wheel_down", "Scroll down"),
        ] {
            let path = format!("{base}/click/{trigger}");
            select(
                b,
                c,
                label,
                app.get(DocId::Bars, &path).cloned().unwrap_or(Value::Null),
                Edit::new(DocId::Bars, path, label),
                [
                    ("", "Its own"),
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
                ]
                .into_iter()
                .map(|(value, label)| {
                    (
                        if value.is_empty() {
                            Value::Null
                        } else {
                            json!(value)
                        },
                        label.into(),
                    )
                })
                .collect(),
            );
        }
    });
    if id.starts_with("custom:") {
        group(b, c, "Program", |b, c| {
            for (key, label) in [("command", "Program"), ("interval", "Run every (seconds)")] {
                let path = format!("{base}/{key}");
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(DocId::Bars, path.clone().into()),
                    label,
                    "",
                    app.get(DocId::Bars, &path)
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
            }
            field(
                app,
                b,
                c,
                FieldId::Text(
                    DocId::Studio,
                    format!("/module_args/{}", crate::docs::token(id)).into(),
                ),
                "Arguments",
                "",
                "",
            );
        });
    }
    if more(app, b, c, "Icon, colors and room") {
        group(b, c, "Look", |b, c| {
            for (key, label) in [
                ("icon", "Icon"),
                ("style/color", "Text color"),
                ("style/background", "Background"),
                ("style/border/color", "Edge color"),
            ] {
                let path = format!("{base}/{key}");
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(DocId::Bars, path.clone().into()),
                    label,
                    "From the bar",
                    app.get(DocId::Bars, &path)
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
            }
        });
    }
    group(b, c, "Order", |b, c| {
        action_button(b, c, "Move earlier", Action::MoveModule(slot.clone(), -1));
        action_button(b, c, "Move later", Action::MoveModule(slot.clone(), 1));
        action_button(
            b,
            c,
            "Remove from this bar",
            Action::RemoveModule(slot.clone()),
        );
    });
}

pub fn add(app: &App, b: &mut B, c: &mut Col) {
    let bar = app.nav.bar.unwrap_or(Bar::Top);
    title(
        b,
        c,
        "Add a module",
        if bar == Bar::Top { "Top bar" } else { "Dock" },
    );
    group(b, c, "Goes in", |b, c| {
        let current = app.text(&FieldId::Text(DocId::Studio, "/add_module_list".into()));
        select(
            b,
            c,
            "Place",
            json!(if current.is_empty() { "right" } else { current }),
            Edit::new(DocId::Studio, "/add_module_list", "Place"),
            [("left", "Start"), ("center", "Middle"), ("right", "End")]
                .into_iter()
                .filter(|(id, _)| bar == Bar::Top || *id != "center")
                .map(|(v, l)| (json!(v), l.into()))
                .collect(),
        );
    });
    let selected = app.panel.as_ref().unwrap().add_module.as_deref();
    group(b, c, "Built in", |b, c| {
        for (id, name) in MODULES {
            row(
                b,
                c,
                Some(glyph::BARS),
                name,
                "",
                "",
                Some(selected == Some(id)),
                Action::Panel(Command::PickModule(id.into())),
            );
        }
    });
    group(b, c, "Your apps", |b, c| {
        for plugin in &app.plugins.installed {
            for module in &plugin.bars {
                let id = format!("custom:{}--{}", plugin.id, module.id);
                row(
                    b,
                    c,
                    Some(glyph::INTEGRATIONS),
                    &module.name,
                    "",
                    &plugin.name,
                    Some(selected == Some(id.as_str())),
                    Action::Panel(Command::PickModule(id)),
                );
            }
        }
    });
    group(b, c, "", |b, c| {
        row(
            b,
            c,
            Some(glyph::GRIP),
            "A gap",
            "",
            "",
            Some(selected == Some("|")),
            Action::Panel(Command::PickModule("|".into())),
        );
        field(
            app,
            b,
            c,
            FieldId::Text(DocId::Studio, "/new_custom".into()),
            "Your own module's name",
            "weather-line",
            "",
        );
        let custom = app
            .text(&FieldId::Text(DocId::Studio, "/new_custom".into()))
            .trim()
            .to_lowercase()
            .replace(' ', "-");
        if !custom.is_empty() {
            row(
                b,
                c,
                Some(glyph::APP),
                "Your own program",
                "",
                &custom,
                Some(selected == Some(format!("custom:{custom}").as_str())),
                Action::Panel(Command::PickModule(format!("custom:{custom}"))),
            );
        }
    });
}

pub fn insert_module(app: &mut App) {
    let bar = app.nav.bar.unwrap_or(Bar::Top);
    let list = match app.text(&FieldId::Text(DocId::Studio, "/add_module_list".into())) {
        "left" => "left",
        "center" if bar == Bar::Top => "center",
        _ => "right",
    };
    if let Some(id) = app.panel.as_ref().unwrap().add_module.clone() {
        app.run(Action::InsertModule(bar, list, id));
        app.panel.as_mut().unwrap().page = if bar == Bar::Top {
            Page::Top
        } else {
            Page::Dock
        };
    }
}

pub fn panels(b: &mut B, c: &mut Col) {
    title(b, c, "Panels", "The things your bars open");
    group(b, c, "", |b, c| {
        for (panel, icon, name, caption) in [
            (
                Panel::Start,
                glyph::START,
                "Start menu",
                "Layout, search and the Windows key",
            ),
            (Panel::Sound, glyph::SOUND, "Sound", "Output switching"),
            (
                Panel::Calendar,
                glyph::CALENDAR,
                "Calendar",
                "Time zones and week start",
            ),
            (
                Panel::Tray,
                glyph::CHEVRON_DOWN,
                "Tray",
                "Collect tray icons",
            ),
            (
                Panel::Notifications,
                glyph::BELL,
                "Notifications",
                "Windows' notifications",
            ),
        ] {
            row(
                b,
                c,
                Some(icon),
                name,
                "",
                caption,
                None,
                Action::SelectPanel(Some(panel)),
            );
        }
    });
}

pub fn panel(app: &App, b: &mut B, c: &mut Col) {
    let panel = app.nav.panel.unwrap_or(Panel::Start);
    let (name, ids): (&str, &[&str]) = match panel {
        Panel::Start => (
            "Start menu",
            &["start_layout", "start_files", "start_win_key"],
        ),
        Panel::Sound => ("Sound", &["sound_switch"]),
        Panel::Calendar => ("Calendar", &["calendar_first_day"]),
        Panel::Tray => ("Tray", &["tray_host"]),
        Panel::Notifications => ("Notifications", &[]),
    };
    title(b, c, name, "A panel the bars open");
    group(b, c, "", |b, c| {
        for &id in ids {
            let s = crate::settings::Setting::get(id).unwrap();
            setting(app, b, c, id, s.label, "");
        }
        if panel == Panel::Calendar {
            let zones = app
                .get(DocId::Bars, "/calendar/zones")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (i, zone) in zones.iter().enumerate() {
                let mut remaining = zones.clone();
                remaining.remove(i);
                row(
                    b,
                    c,
                    Some(glyph::CLOCK),
                    zone["zone"].as_str().unwrap_or("Time zone"),
                    "Remove",
                    "",
                    None,
                    Action::Set(
                        Edit::new(DocId::Bars, "/calendar/zones", "Time zones"),
                        Value::Array(remaining),
                    ),
                );
            }
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Studio, "/new_zone".into()),
                "Add a time zone",
                "Europe/London",
                "",
            );
        }
        if panel == Panel::Notifications {
            notice(
                b,
                c,
                "The bell opens Windows' notification center. Turn on access and choose what it shows under Connections.",
            );
            action_button(b, c, "Connections", open(Page::Connections));
        }
    });
}
