use serde_json::{Value, json};

use super::*;
use crate::pages::integrations::{self as legacy, Field};

fn plugin_status<'a>(app: &'a App, id: &str) -> &'a str {
    if app.plugins.preferences.disabled.contains(id) {
        return "Available";
    }
    app.plugins
        .snapshot
        .plugins
        .get(id)
        .map_or("Available", |s| {
            if s.status == "Unloaded" {
                "Available"
            } else {
                s.status.as_str()
            }
        })
}

pub fn list(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Connections", "Your apps and built-in data sources");
    for name in ["Connected", "Built in", "Available"] {
        let plugins: Vec<_> = app
            .plugins
            .installed
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let state = plugin_status(app, &p.id);
                match name {
                    "Connected" => state != "Available",
                    "Available" => state == "Available",
                    _ => false,
                }
            })
            .collect();
        if plugins.is_empty() && name != "Built in" {
            continue;
        }
        group(b, c, name, |b, c| {
            if name == "Built in" {
                for (i, item) in legacy::LIST.iter().enumerate() {
                    let on = legacy::enabled(app, item);
                    row(
                        b,
                        c,
                        Some(item.glyph),
                        item.name,
                        if on { "On" } else { "Off" },
                        item.adds,
                        None,
                        Action::Panel(Command::Connection(i)),
                    );
                }
            }
            for (i, item) in plugins {
                let status = plugin_status(app, &item.id);
                let reconnect = status.starts_with("Stopped") || status.starts_with("Restarting");
                let detail = if reconnect {
                    status.to_owned()
                } else {
                    let faces = item
                        .faces
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    if faces.is_empty() {
                        format!(
                            "{} · {}",
                            item.version.as_deref().unwrap_or("Plugin"),
                            item.author.as_deref().unwrap_or("")
                        )
                    } else {
                        faces
                    }
                };
                crate::panel::icons::row(
                    app,
                    b,
                    c,
                    &format!("custom:{}", item.id),
                    glyph::INTEGRATIONS,
                    &item.name,
                    &format!(
                        "{} \u{b7} {detail}",
                        if reconnect { "Reconnect" } else { status }
                    ),
                    false,
                    Action::Panel(Command::Connection(legacy::LIST.len() + i)),
                );
            }
        });
    }
    if app.plugins.installed.is_empty() {
        notice(
            b,
            c,
            "Installed app connections appear here when their plugins are available.",
        );
    }
    notice(
        b,
        c,
        "Each connection describes what it reads and where it connects. A stopped connection keeps its options.",
    );
}

pub fn details(app: &App, b: &mut B, c: &mut Col) {
    let index = app.nav.integration;
    if let Some(plugin) = index
        .checked_sub(legacy::LIST.len())
        .and_then(|i| app.plugins.installed.get(i))
    {
        let status = plugin_status(app, &plugin.id);
        title(b, c, &plugin.name, status);
        group(b, c, "Connection", |b, c| {
            let disabled = app.plugins.preferences.disabled.contains(&plugin.id);
            let needs_attention = status.starts_with("Stopped") || status.starts_with("Restarting");
            let label = if needs_attention {
                "Reconnect"
            } else if disabled || status == "Available" {
                "Connect"
            } else {
                "Disconnect"
            };
            action_button(
                b,
                c,
                label,
                if label == "Disconnect" {
                    Action::Panel(Command::Disconnect(plugin.id.clone()))
                } else {
                    Action::Panel(Command::Connect(plugin.id.clone()))
                },
            );
        });
        if !plugin.faces.is_empty() || !plugin.bars.is_empty() || !plugin.panels.is_empty() {
            group(b, c, "Shows in", |b, c| {
                for face in &plugin.faces {
                    let id = format!("{}--{}", plugin.id, face.id);
                    let layout = crate::pages::widgets::layout(app);
                    let shown = crate::pages::widgets::here(app, &layout)
                        .iter()
                        .any(|i| i.widget == id);
                    toggle(
                        b,
                        c,
                        &face.name,
                        &format!("Widget · Display {}", app.nav.display + 1),
                        shown,
                        if shown {
                            Action::Panel(Command::HideConnectionWidget(id))
                        } else {
                            Action::Panel(Command::PickWidget(id))
                        },
                    );
                }
                for bar in &plugin.bars {
                    let id = format!("custom:{}--{}", plugin.id, bar.id);
                    for (which, label) in [
                        (crate::actions::Bar::Top, "Top bar"),
                        (crate::actions::Bar::Dock, "Dock"),
                    ] {
                        let shown = module_shown(app, &id, which);
                        toggle(
                            b,
                            c,
                            &format!("{label} · {}", bar.name),
                            "",
                            shown,
                            Action::Panel(Command::ConnectionModule(id.clone(), which, !shown)),
                        );
                    }
                }
                for panel in &plugin.panels {
                    notice(b, c, &format!("Panel · {}", panel.name));
                }
            });
        }
        group(b, c, "Options", |b, c| {
            if plugin.settings.is_empty() {
                notice(b, c, "This connection has no options.");
            }
            for schema in &plugin.settings {
                let Some(name) = schema["key"].as_str() else {
                    continue;
                };
                let label = schema["label"].as_str().unwrap_or(name);
                let path = format!(
                    "/plugins/setting/{}/{}",
                    plugin.id,
                    crate::docs::token(name)
                );
                let value = app
                    .plugins
                    .preferences
                    .settings
                    .get(&plugin.id)
                    .and_then(|v| v.get(name))
                    .cloned()
                    .unwrap_or_else(|| schema["default"].clone());
                let edit = Edit::new(DocId::Studio, path.clone(), label);
                match schema["type"].as_str() {
                    Some("boolean") => {
                        let on = value.as_bool().unwrap_or(false);
                        toggle(b, c, label, "", on, Action::Toggle(edit, on));
                    }
                    Some("number" | "slider") => {
                        crate::panel::controls::text(
                            b,
                            mypaper_ui::Rect::new(c.x, c.y, c.w, height(b, 24.0)),
                            label,
                            13.0,
                            400,
                            b.p.ink1,
                            mypaper_ui::controls::Align::Start,
                        );
                        c.y += height(b, 24.0);
                        let value = value.as_f64().unwrap_or(0.0);
                        b.slider(
                            mypaper_ui::controls::key(path.as_str()),
                            mypaper_ui::Rect::new(c.x, c.y, c.w, height(b, 36.0)),
                            value,
                            (
                                schema["min"].as_f64().unwrap_or(0.0),
                                schema["max"].as_f64().unwrap_or(100.0),
                                schema["step"].as_f64().unwrap_or(1.0),
                            ),
                            &format!("{value:.0}"),
                            b.px(48.0),
                            label,
                            Action::Slider(edit),
                        );
                        c.y += height(b, 40.0);
                    }
                    Some("select") => {
                        let options = schema["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|o| {
                                let value = o.get("value").unwrap_or(o).clone();
                                let label = o
                                    .get("label")
                                    .and_then(Value::as_str)
                                    .or_else(|| value.as_str())
                                    .unwrap_or("Option")
                                    .to_owned();
                                (value, label)
                            })
                            .collect();
                        select(b, c, label, value, edit, options);
                    }
                    _ => {
                        let text = value.as_str().map(str::to_owned).unwrap_or_else(|| {
                            if value.is_null() {
                                String::new()
                            } else {
                                value.to_string()
                            }
                        });
                        field(
                            app,
                            b,
                            c,
                            FieldId::Text(DocId::Studio, path.into()),
                            label,
                            "",
                            &text,
                        );
                    }
                }
            }
        });
        group(b, c, "Access and privacy", |b, c| {
            for line in crate::pages::plugins::permissions(plugin) {
                notice(b, c, &line);
            }
        });
        return;
    }
    let Some(item) = legacy::LIST.get(index) else {
        return;
    };
    let on = legacy::enabled(app, item);
    title(
        b,
        c,
        item.name,
        if on {
            "Built in · On"
        } else {
            "Built in · Off"
        },
    );
    group(b, c, "Connection", |b, c| {
        notice(b, c, item.adds);
        if item.section.is_some() {
            action_button(
                b,
                c,
                if on { "Turn off" } else { "Turn on" },
                Action::Set(
                    Edit::new(
                        DocId::Studio,
                        format!("/integration_{}/{index}", if on { "off" } else { "on" }),
                        "Connection",
                    ),
                    Value::Null,
                ),
            );
        }
    });
    if item.module.is_some() {
        group(b, c, "Shows in", |b, c| {
            let module = item.module.unwrap();
            for (bar, label) in [
                (crate::actions::Bar::Top, "Top bar"),
                (crate::actions::Bar::Dock, "Dock"),
            ] {
                let shown = module_shown(app, module, bar);
                toggle(
                    b,
                    c,
                    label,
                    "Its module, at the end",
                    shown,
                    Action::Panel(Command::ConnectionModule(module.into(), bar, !shown)),
                );
            }
        });
    }
    group(b, c, "Options", |b, c| {
        notice(b, c, item.setup);
        for option in item.fields {
            let Field::Switch { path, label, hint } = option;
            let on = app
                .get(DocId::Integrations, path)
                .and_then(Value::as_bool)
                .unwrap_or(false);
            toggle(
                b,
                c,
                label,
                hint,
                on,
                Action::Toggle(Edit::new(DocId::Integrations, *path, *label), on),
            );
        }
        if item.name == "Notifications" {
            action_button(
                b,
                c,
                "Grant access in Windows",
                Action::Set(
                    Edit::new(DocId::Studio, "/integration_access", "Access"),
                    Value::Null,
                ),
            );
        }
        if item.name == "Weather and RSS" {
            action_button(b, c, "Their widgets", open(Page::Widgets));
        }
    });
    group(b, c, "Privacy", |b, c| notice(b, c, item.privacy));
}

fn module_shown(app: &App, id: &str, bar: crate::actions::Bar) -> bool {
    ["left", "center", "right"].iter().any(|side| {
        app.get(DocId::Bars, &format!("/{}/{side}", bar.key()))
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(|m| m.as_str() == Some(id)))
    })
}

pub fn connection_module(app: &mut App, id: &str, bar: crate::actions::Bar, on: bool) {
    let ops = ["left", "center", "right"]
        .into_iter()
        .filter(|side| bar != crate::actions::Bar::Dock || *side != "center")
        .filter_map(|side| {
            let path = format!("/{}/{side}", bar.key());
            let before = app
                .get(DocId::Bars, &path)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut after = before.clone();
            after.retain(|m| m.as_str() != Some(id));
            if on && side == "right" {
                after.push(json!(id));
            }
            (after != before).then_some(crate::docs::Op {
                doc: DocId::Bars,
                path,
                value: Some(json!(after)),
            })
        })
        .collect();
    app.apply("Connection module", ops);
}

pub fn hide_connection_widget(app: &mut App, widget: &str) {
    let mut layout = crate::pages::widgets::layout(app);
    let here: Vec<_> = crate::pages::widgets::here(app, &layout)
        .iter()
        .filter(|i| i.widget == widget)
        .map(|i| i.instance.clone())
        .collect();
    layout.instances.retain(|i| !here.contains(&i.instance));
    app.set(
        &Edit::new(DocId::Widgets, "/instances", "Remove connection widget"),
        json!(layout.instances),
    );
}

pub fn connect(app: &mut App, id: &str) {
    let mut disabled = app.plugins.preferences.disabled.clone();
    disabled.remove(id);
    app.apply(
        "Connect",
        vec![crate::docs::Op {
            doc: DocId::Plugins,
            path: "/disabled".into(),
            value: Some(json!(disabled)),
        }],
    );
}

/// Disable a connection and remove its widget faces and bar modules in one reversible step.
pub fn disconnect(app: &mut App, id: &str) {
    use crate::docs::Op;
    let mut disabled = app.plugins.preferences.disabled.clone();
    disabled.insert(id.into());
    let mut ops = vec![Op {
        doc: DocId::Plugins,
        path: "/disabled".into(),
        value: Some(json!(disabled)),
    }];
    let prefix = format!("{id}--");
    let widgets = app
        .get(DocId::Widgets, "/instances")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let kept: Vec<_> = widgets
        .iter()
        .filter(|instance| {
            !instance["widget"]
                .as_str()
                .is_some_and(|widget| widget.starts_with(&prefix))
        })
        .cloned()
        .collect();
    if kept != widgets {
        ops.push(Op {
            doc: DocId::Widgets,
            path: "/instances".into(),
            value: Some(json!(kept)),
        });
    }
    for bar in ["top", "dock"] {
        for zone in ["left", "center", "right"] {
            let path = format!("/{bar}/{zone}");
            let modules = app
                .get(DocId::Bars, &path)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let kept: Vec<_> = modules
                .iter()
                .filter(|module| {
                    !module
                        .as_str()
                        .is_some_and(|module| module.starts_with(&format!("custom:{prefix}")))
                })
                .cloned()
                .collect();
            if kept != modules {
                ops.push(Op {
                    doc: DocId::Bars,
                    path,
                    value: Some(json!(kept)),
                });
            }
        }
    }
    app.apply("Disconnect", ops);
}
