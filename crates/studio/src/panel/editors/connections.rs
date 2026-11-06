use serde_json::{Value, json};

use super::*;
use crate::pages::integrations::{self as legacy, Field};

fn plugin_status<'a>(app: &'a App, id: &str) -> &'a str {
    if app.plugins.preferences.disabled.contains(id) { return "Available" }
    app.plugins.snapshot.plugins.get(id).map_or("Available", |s| s.status.as_str())
}

pub fn list(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Connections", "Your apps and built-in data sources");
    for name in ["Connected", "Built in", "Available"] {
        let plugins: Vec<_> = app.plugins.installed.iter().enumerate().filter(|(_, p)| {
            let state = plugin_status(app, &p.id);
            match name { "Connected" => state != "Available", "Available" => state == "Available", _ => false }
        }).collect();
        if plugins.is_empty() && name != "Built in" { continue }
        group(b, c, name, |b, c| {
            if name == "Built in" {
                for (i, item) in legacy::LIST.iter().enumerate() {
                    let on = legacy::enabled(app, item);
                    row(b, c, Some(item.glyph), item.name, if on { "On" } else { "Off" }, item.adds, None, Action::Panel(Command::Connection(i)));
                }
            }
            for (i, item) in plugins {
                let status = plugin_status(app, &item.id);
                let reconnect = status.starts_with("Stopped") || status.starts_with("Restarting");
                let detail = if reconnect { status.to_owned() } else {
                    let faces = item.faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ");
                    if faces.is_empty() { format!("{} · {}", item.version.as_deref().unwrap_or("Plugin"), item.author.as_deref().unwrap_or("")) } else { faces }
                };
                row(b, c, Some(glyph::INTEGRATIONS), &item.name, if reconnect { "Reconnect" } else { status }, &detail, None, Action::Panel(Command::Connection(legacy::LIST.len() + i)));
            }
        });
    }
    if app.plugins.installed.is_empty() { notice(b, c, "Installed app connections appear here when their plugins are available."); }
    notice(b, c, "Each connection describes what it reads and where it connects. A stopped connection keeps its options.");
}

pub fn details(app: &App, b: &mut B, c: &mut Col) {
    let index = app.nav.integration;
    if let Some(plugin) = index.checked_sub(legacy::LIST.len()).and_then(|i| app.plugins.installed.get(i)) {
        let status = plugin_status(app, &plugin.id);
        title(b, c, &plugin.name, status);
        group(b, c, "Connection", |b, c| {
            let disabled = app.plugins.preferences.disabled.contains(&plugin.id);
            let needs_attention = status.starts_with("Stopped") || status.starts_with("Restarting");
            let label = if needs_attention { "Reconnect" } else if disabled || status == "Available" { "Connect" } else { "Disconnect" };
            let path = if label == "Disconnect" { "/plugins/disable" } else { "/plugins/enable" };
            action_button(b, c, label, Action::Set(Edit::new(DocId::Studio, path, label), json!(plugin.id)));
        });
        group(b, c, "Adds to your desktop", |b, c| {
            for face in &plugin.faces { row(b, c, Some(glyph::WIDGETS), &face.name, "Widget", "", None, open(Page::AddWidget)); }
            for bar in &plugin.bars { row(b, c, Some(glyph::BARS), &bar.name, "Bar module", "", None, open(Page::Bars)); }
            for panel in &plugin.panels { notice(b, c, &format!("Panel · {}", panel.name)); }
        });
        group(b, c, "Options", |b, c| {
            if plugin.settings.is_empty() { notice(b, c, "This connection has no options."); }
            for schema in &plugin.settings {
                c.y = crate::pages::plugins::setting(app, b, plugin, schema, c.x, c.y, c.w);
            }
        });
        group(b, c, "Access and privacy", |b, c| {
            for line in crate::pages::plugins::permissions(plugin) { notice(b, c, &line); }
        });
        return;
    }
    let Some(item) = legacy::LIST.get(index) else { return };
    let on = legacy::enabled(app, item);
    title(b, c, item.name, if on { "Built in · On" } else { "Built in · Off" });
    group(b, c, "Connection", |b, c| {
        notice(b, c, item.adds);
        if item.section.is_some() {
            action_button(b, c, if on { "Turn off" } else { "Turn on" },
                Action::Set(Edit::new(DocId::Studio, format!("/integration_{}/{index}", if on { "off" } else { "on" }), "Connection"), Value::Null));
        }
    });
    if item.module.is_some() {
        group(b, c, "Shows in", |b, c| {
            let module = item.module.unwrap();
            let shown = ["left", "center", "right"].iter().any(|side| app.get(DocId::Bars, &format!("/top/{side}")).and_then(Value::as_array).is_some_and(|list| list.iter().any(|m| m.as_str() == Some(module))));
            toggle(b, c, "Top bar", "Its module, at the end", shown, Action::Set(Edit::new(DocId::Studio, format!("/integration_module/{index}"), "Shows in"), Value::Null));
        });
    }
    group(b, c, "Options", |b, c| {
        notice(b, c, item.setup);
        for option in item.fields {
            let Field::Switch { path, label, hint } = option;
            let on = app.get(DocId::Integrations, path).and_then(Value::as_bool).unwrap_or(false);
            toggle(b, c, label, hint, on, Action::Toggle(Edit::new(DocId::Integrations, *path, *label), on));
        }
        if item.name == "Notifications" {
            action_button(b, c, "Grant access in Windows", Action::Set(Edit::new(DocId::Studio, "/integration_access", "Access"), Value::Null));
        }
        if item.name == "Weather and RSS" { action_button(b, c, "Their widgets", open(Page::Widgets)); }
    });
    group(b, c, "Privacy", |b, c| notice(b, c, item.privacy));
}
