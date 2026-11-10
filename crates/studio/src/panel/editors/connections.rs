use serde_json::{Value, json};

use super::*;
use crate::editing::integrations::{self as legacy, Field};

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

/// Set up (running, starting or needing attention), not merely installed: only then are its
/// widgets and bar modules offered. Connections is where an available one gets set up.
pub fn connected(app: &App, id: &str) -> bool {
    plugin_status(app, id) != "Available"
}

pub fn visible_plugin(plugin: &glaze_widgets::manifest::PluginManifest) -> bool {
    !["hello", "hello-ts", "demo", "experimental"].contains(&plugin.id.as_str())
        && !plugin
            .name
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| matches!(word, "demo" | "experimental"))
}

/// The list and Home count share membership, including hidden and replaced sources.
fn members(app: &App, group: &str) -> Vec<usize> {
    let replaces_weather =
        app.plugins.installed.iter().any(|plugin| {
            matches!(plugin.id.as_str(), "weather" | "rss") && visible_plugin(plugin)
        });
    legacy::LIST
        .iter()
        .enumerate()
        .filter(|(_, item)| !(item.name == "Weather and RSS" && replaces_weather))
        .filter(|(_, item)| {
            if legacy::enabled(app, item) {
                group == "Connected"
            } else {
                group == "Built in"
            }
        })
        .map(|(index, _)| index)
        .chain(
            app.plugins
                .installed
                .iter()
                .enumerate()
                .filter(|(_, plugin)| visible_plugin(plugin))
                .filter(|(_, plugin)| {
                    if plugin_status(app, &plugin.id) == "Available" {
                        group == "Available"
                    } else {
                        group == "Connected"
                    }
                })
                .map(|(index, _)| legacy::LIST.len() + index),
        )
        .collect()
}

pub fn summary(app: &App) -> String {
    let connected = members(app, "Connected").len();
    format!("{connected} connected")
}

pub fn list(app: &App, b: &mut B, c: &mut Col) {
    page_title(app, b, c, "Connections", "Your apps and built-in data sources");
    for name in ["Connected", "Built in", "Available"] {
        let members = members(app, name);
        if members.is_empty() {
            continue;
        }
        group(b, c, name, |b, c| {
            for index in members {
                if let Some(item) = legacy::LIST.get(index) {
                    let on = legacy::enabled(app, item);
                    crate::panel::controls::artwork_row(
                        b,
                        c,
                        item.glyph,
                        None,
                        item.name,
                        if on { "On" } else { "Off" },
                        item.adds,
                        Action::Panel(Command::Connection(index)),
                    );
                    continue;
                }
                let item = &app.plugins.installed[index - legacy::LIST.len()];
                // A status with a reason shows one word; the reason becomes the caption.
                let (status, reason) = match plugin_status(app, &item.id).split_once(": ") {
                    Some((word, reason)) => (word, Some(reason)),
                    None => (plugin_status(app, &item.id), None),
                };
                // One vocabulary with the built-ins: running reads On. Under "Available" the heading
                // already says it, so the row needs no status.
                let status = match status {
                    "Live" => "On",
                    "Available" => "",
                    other => other,
                };
                let detail = if let Some(reason) = reason {
                    reason.to_owned()
                } else {
                    let faces = item
                        .faces
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    if faces.is_empty() {
                        [item.version.as_deref(), item.author.as_deref()]
                            .into_iter()
                            .flatten()
                            .filter(|text| !text.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    } else {
                        faces
                    }
                };
                let image = crate::panel::icons::plugin_image(app, &format!("custom:{}", item.id));
                let icon = match item.id.as_str() {
                    "rss" => crate::editing::widgets::glyph_of("rss"),
                    "weather" => glaze_ui::icons::Icon::CloudSun.into(),
                    _ => glyph::INTEGRATIONS,
                };
                crate::panel::controls::artwork_row(
                    b,
                    c,
                    icon,
                    image.as_ref(),
                    &item.name,
                    status,
                    &detail,
                    Action::Panel(Command::Connection(index)),
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
        page_title(app, b, c, &plugin.name, status);
        crate::panel::controls::displays(app, b, c);
        let disabled = app.plugins.preferences.disabled.contains(&plugin.id);
        let needs_attention = status.starts_with("Stopped") || status.starts_with("Restarting");
        if disabled || needs_attention || status == "Available" {
            group(b, c, "Connection", |b, c| {
                action_button(
                    b,
                    c,
                    if needs_attention {
                        "Reconnect"
                    } else {
                        "Connect"
                    },
                    Action::Panel(Command::Connect(plugin.id.clone())),
                );
            });
        }
        if !plugin.faces.is_empty() || !plugin.bars.is_empty() || !plugin.panels.is_empty() {
            group(b, c, "Shows in", |b, c| {
                for face in &plugin.faces {
                    let id = format!("{}--{}", plugin.id, face.id);
                    let layout = crate::editing::widgets::layout(app);
                    let shown = crate::editing::widgets::here(app, &layout)
                        .iter()
                        .any(|i| i.widget == id);
                    toggle(
                        b,
                        c,
                        &face.name,
                        &format!(
                            "Widget · {}",
                            crate::panel::controls::display_name(app, app.nav.display)
                        ),
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
        if !plugin.settings.is_empty() {
            group(b, c, "Options", |b, c| {
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
                            let value = value.as_f64().unwrap_or(0.0);
                            crate::panel::material::row(
                                b,
                                c,
                                label,
                                value,
                                (
                                    schema["min"].as_f64().unwrap_or(0.0),
                                    schema["max"].as_f64().unwrap_or(100.0),
                                    schema["step"].as_f64().unwrap_or(1.0),
                                ),
                                &crate::panel::controls::unit(
                                    value,
                                    schema["unit"].as_str().unwrap_or(""),
                                ),
                                edit,
                            );
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
        }
        group(b, c, "Access and privacy", |b, c| {
            c.y += b.px(8.0);
            for line in crate::editing::plugins::permissions(plugin) {
                notice(b, c, &line);
            }
            c.y += b.px(8.0);
        });
        if !disabled && !needs_attention && status != "Available" {
            group(b, c, "", |b, c| {
                action_button(
                    b,
                    c,
                    "Disconnect",
                    Action::Panel(Command::Disconnect(plugin.id.clone())),
                );
            });
        }
        return;
    }
    let Some(item) = legacy::LIST.get(index) else {
        return;
    };
    let on = legacy::enabled(app, item);
    page_title(app,
        b,
        c,
        item.name,
        if on {
            "Built in · On"
        } else {
            "Built in · Off"
        },
    );
    crate::panel::controls::displays(app, b, c);
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
    group(b, c, "Privacy", |b, c| {
        c.y += b.px(8.0);
        notice(b, c, item.privacy);
        c.y += b.px(8.0);
    });
}

fn module_shown(app: &App, id: &str, bar: crate::actions::Bar) -> bool {
    ["left", "center", "right"].iter().any(|side| {
        bar_value(app, &format!("/{}/{side}", bar.key()))
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(|m| m.as_str() == Some(id)))
    })
}

pub fn connection_module(app: &mut App, id: &str, bar: crate::actions::Bar, on: bool) {
    let ops = ["left", "center", "right"]
        .into_iter()
        .filter(|side| bar != crate::actions::Bar::Dock || *side != "center")
        .filter_map(|side| {
            let source = format!("/{}/{side}", bar.key());
            let path = bar_path(app, &source);
            let before = bar_value(app, &source)
                .as_ref()
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
    let mut layout = crate::editing::widgets::layout(app);
    let here: Vec<_> = crate::editing::widgets::here(app, &layout)
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
    let raw = &app.docs.doc(DocId::Bars).raw;
    let layers = std::iter::once(String::new()).chain(
        raw.get("overrides")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(display, _)| format!("/overrides/{}", crate::docs::token(display))),
    );
    let module_prefix = format!("custom:{prefix}");
    for layer in layers {
        for bar in ["top", "dock"] {
            for zone in ["left", "center", "right"] {
                let path = format!("{layer}/{bar}/{zone}");
                let modules = app
                    .docs
                    .doc(DocId::Bars)
                    .raw
                    .pointer(&path)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let kept: Vec<_> = modules
                    .iter()
                    .filter(|module| {
                        !module
                            .as_str()
                            .is_some_and(|module| module.starts_with(&module_prefix))
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
    }
    app.apply("Disconnect", ops);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_count_matches_connected_rows_and_sources_are_never_duplicated() {
        let mut ui = glaze_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        app.panel.as_mut().unwrap().page = Page::Connections;
        crate::editing::plugins::fixture(&mut app.plugins);
        let count = members(&app, "Connected").len();
        assert_eq!(summary(&app), format!("{count} connected"));
        let built = crate::panel::build(&app, &mut ui.text);
        assert!(!built.list.hits.iter().any(
            |hit| hit.label.starts_with("Hello ") || hit.label.starts_with("Weather and RSS ")
        ));
        let connected = members(&app, "Connected");
        assert!(connected.contains(&0)); // Windows media is enabled by default.
        assert!(connected.contains(&legacy::LIST.len())); // Live T3 plugin.
        app.plugins.preferences.disabled.insert("t3".into());
        assert_eq!(members(&app, "Connected").len(), count - 1);
        assert!(members(&app, "Available").contains(&legacy::LIST.len()));
    }

    #[test]
    fn disconnect_removes_modules_from_base_and_all_overrides_as_one_undo_step() {
        let ui = glaze_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let bars = json!({"version":1,
            "top":{"left":["start","custom:t3--status","custom:t30--status"],"center":[],"right":["clock"]},
            "dock":{"left":[],"right":["custom:t3--agents","network"]},
            "overrides":{
                "DISPLAY2":{"top":{"left":["start","custom:t30--status"],"right":["custom:t3--status","clock"]}},
                "secondary":{"dock":{"left":["custom:t3--agents"],"right":["network"]}},
                "DISPLAY9":{"top":{"left":["custom:t3--status","volume"]}}
            }
        });
        app.docs = crate::docs::Docs::fixture(["{}", &bars.to_string(), "{}", "{}"]);
        assert!(
            glaze_bars::BarsConfig::parse(&bars.to_string()).is_ok(),
            "{:?}",
            glaze_bars::BarsConfig::parse(&bars.to_string()).err()
        );
        disconnect(&mut app, "t3");
        let after = &app.docs.doc(DocId::Bars).raw;
        for path in [
            "/top/left",
            "/dock/right",
            "/overrides/DISPLAY2/top/right",
            "/overrides/secondary/dock/left",
            "/overrides/DISPLAY9/top/left",
        ] {
            assert!(
                !after
                    .pointer(path)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|module| module
                        .as_str()
                        .is_some_and(|id| id.starts_with("custom:t3--"))),
                "{path}"
            );
        }
        assert_eq!(
            after.pointer("/top/left"),
            Some(&json!(["start", "custom:t30--status"]))
        );
        assert!(app.plugins.preferences.disabled.contains("t3"));
        assert_eq!(app.docs.undo_count(), 1);
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Bars).raw, bars);
        assert!(!app.plugins.preferences.disabled.contains("t3"));
    }

    #[test]
    fn shows_in_and_toggles_follow_the_selected_display() {
        let ui = glaze_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let second = app.monitors[1]
            .device
            .trim_start_matches(r"\\.\")
            .to_owned();
        let bars = json!({"version":1,"top":{"center":[],"right":["clock"]},"overrides":{
            second: {"top":{"right":["custom:t3--status","clock"]}}
        }});
        glaze_bars::BarsConfig::parse(&bars.to_string()).unwrap();
        app.docs = crate::docs::Docs::fixture(["{}", &bars.to_string(), "{}", "{}"]);
        app.nav.display = 0;
        assert!(!module_shown(
            &app,
            "custom:t3--status",
            crate::actions::Bar::Top
        ));
        app.nav.display = 1;
        assert!(module_shown(
            &app,
            "custom:t3--status",
            crate::actions::Bar::Top
        ));
        connection_module(
            &mut app,
            "custom:t3--status",
            crate::actions::Bar::Top,
            false,
        );
        assert!(!module_shown(
            &app,
            "custom:t3--status",
            crate::actions::Bar::Top
        ));
        assert_eq!(
            app.docs.doc(DocId::Bars).raw.pointer("/top/right"),
            bars.pointer("/top/right")
        );
        assert_eq!(app.docs.undo_count(), 1);
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Bars).raw, bars);
    }
}
