use crate::panel::tokens::radius;
use mypaper_bars::config::DockHide;
use mypaper_ui::Rect;
use mypaper_ui::controls::Align;
use serde_json::{Value, json};

use super::*;
use crate::actions::{Bar, Panel, Pins, Slot};
use crate::panel::controls::more;

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
    if id == "|" {
        return "Gap";
    }
    MODULES
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(id.strip_prefix("custom:").unwrap_or(id), |(_, name)| *name)
}

fn display_key(app: &App) -> String {
    app.display().map_or_else(
        || format!("DISPLAY{}", app.nav.display + 1),
        |m| m.device.trim_start_matches(r"\\.\").to_owned(),
    )
}

fn shared(path: &str) -> bool {
    path.starts_with("/taskbar")
        || path.starts_with("/icons")
        || path == "/dock/pinned"
        || path.ends_with("/clock/format")
        || path.starts_with("/performance")
}

/// Each display edits its own layer; Windows taskbar and pinned apps remain shared.
pub fn bar_path(app: &App, path: &str) -> String {
    if app.monitors.len() > 1 && !shared(path) && !path.starts_with("/overrides/") {
        format!("/overrides/{}{path}", crate::docs::token(&display_key(app)))
    } else {
        path.into()
    }
}

fn resolved(app: &App) -> Value {
    resolved_for(app, app.nav.display)
}

fn resolved_for(app: &App, index: usize) -> Value {
    let raw = &app.docs.doc(DocId::Bars).raw;
    mypaper_bars::BarsConfig::parse(&raw.to_string())
        .ok()
        .and_then(|mut config| {
            config.resolve(Vec::new);
            let config = if let Some(monitor) = app.monitors.get(index) {
                config.for_monitor(&monitor.device, monitor.primary).ok()?
            } else {
                config
            };
            serde_json::to_value(config).ok()
        })
        .unwrap_or_else(|| raw.clone())
}

pub fn bar_value(app: &App, path: &str) -> Option<Value> {
    let unscoped = path
        .strip_prefix("/overrides/")
        .and_then(|p| p.split_once('/'))
        .map(|(_, p)| format!("/{p}"));
    resolved(app)
        .pointer(unscoped.as_deref().unwrap_or(path))
        .cloned()
}

pub fn field_initial(app: &App, id: &FieldId) -> Option<String> {
    let FieldId::Text(doc, path) = id else {
        return None;
    };
    if *doc == DocId::Bars {
        return bar_value(app, path)
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            });
    }
    if *doc != DocId::Studio {
        return None;
    }
    if let Some(path) = path.strip_prefix("/panel/gesture-target/") {
        let (kind, path) = path.split_once('/')?;
        return bar_value(app, &format!("/{path}"))?
            .get(kind)?
            .as_str()
            .map(str::to_owned);
    }
    let module = path.strip_prefix("/panel/module-args/")?;
    let args = bar_value(app, &format!("/modules/{module}/args"))?;
    Some(
        args.as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(|arg| {
                if arg.contains(' ') {
                    format!("\"{arg}\"")
                } else {
                    arg.into()
                }
            })
            .collect::<Vec<String>>()
            .join(" "),
    )
}

pub fn design_name(app: &App, bar: Option<Bar>) -> String {
    if bar.is_none() && !same_pair(app) {
        return format!(
            "{} + {}",
            design_name(app, Some(Bar::Top)),
            design_name(app, Some(Bar::Dock))
        );
    }
    let path = bar.map_or_else(|| "/preset".into(), |bar| format!("/{}/design", bar.key()));
    let value = bar_value(app, &path)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "monochrome".into());
    let designs: &[(&str, &str)] = match bar {
        Some(Bar::Top) => &crate::editing::bars::TOP_DESIGNS,
        Some(Bar::Dock) => &crate::editing::bars::DOCK_DESIGNS,
        None => &crate::editing::bars::PRESETS,
    };
    designs
        .iter()
        .find(|(id, _)| *id == value)
        .map_or(value.clone(), |(_, label)| (*label).into())
}

fn same_pair(app: &App) -> bool {
    let current = resolved(app);
    let preset = current
        .get("preset")
        .and_then(Value::as_str)
        .unwrap_or("monochrome");
    mypaper_bars::BarsConfig::parse(&json!({"preset":preset}).to_string())
        .ok()
        .and_then(|v| serde_json::to_value(v).ok())
        .is_none_or(|preset| {
            ["/top/design", "/dock/design"]
                .into_iter()
                .all(|path| current.pointer(path) == preset.pointer(path))
        })
}

fn setting(app: &App, b: &mut B, c: &mut Col, id: &'static str, label: &str, caption: &str) {
    use crate::settings::Kind;
    let s = crate::settings::Setting::get(id).expect("bar setting");
    if id == "dock_hide" {
        crate::ui::anchor(app, id, c.view, c.y);
        let dock = resolved(app);
        let mode = DockHide::from_flags(
            dock.pointer("/dock/autohide")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            dock.pointer("/dock/hide_when_maximized")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        );
        select(
            b, c, label, json!(mode),
            Edit::new(DocId::Studio, "/panel/dock-hide", "Dock visibility"),
            [
                (DockHide::Never, "Always shown"),
                (DockHide::WhenFilled, "Hide when a window fills the screen"),
                (DockHide::Always, "Always hide (like Mac)"),
            ]
            .into_iter()
            .map(|(mode, label)| (json!(mode), label.into()))
            .collect(),
        );
        return;
    }
    if s.doc != DocId::Bars
        || shared(s.path)
        || matches!(s.place, crate::settings::Place::BarsPanel(_))
    {
        crate::panel::controls::setting(
            app,
            b,
            c,
            id,
            label,
            if s.doc == DocId::Bars {
                "All displays"
            } else {
                caption
            },
        );
        return;
    }
    crate::ui::anchor(app, id, c.view, c.y);
    let value = bar_value(app, s.path).unwrap_or_else(|| app.value(s));
    let edit = Edit::new(DocId::Bars, bar_path(app, s.path), s.label);
    match s.kind {
        Kind::Switch { default } => toggle(
            b,
            c,
            label,
            caption,
            value.as_bool().unwrap_or(default),
            Action::Toggle(edit, value.as_bool().unwrap_or(default)),
        ),
        Kind::Choice { options, .. } => segments(
            b,
            c,
            label,
            value,
            edit,
            options
                .iter()
                .map(|(value, label)| {
                    (
                        serde_json::from_str(value).expect("setting option"),
                        (*label).into(),
                    )
                })
                .collect(),
        ),
        Kind::Select { options, .. } => select(
            b,
            c,
            label,
            value,
            edit,
            options
                .iter()
                .map(|(v, l)| {
                    (
                        serde_json::from_str(v).expect("setting option"),
                        (*l).into(),
                    )
                })
                .collect(),
        ),
        Kind::Slider {
            min,
            max,
            step,
            factor,
            unit,
            default,
        } => {
            let stacked = app.size.0 as f32 <= b.px(360.0) || b.t.text_scale > 1.0;
            let label_width = if stacked {
                c.w
            } else {
                crate::panel::controls::label_width(b, c)
            };
            text(
                b,
                Rect::new(
                    c.x,
                    c.y,
                    label_width,
                    height(b, if stacked { 25.0 } else { 40.0 }),
                ),
                label,
                13.0,
                400,
                b.p.ink1,
                Align::Start,
            );
            if stacked {
                c.y += height(b, 25.0);
            }
            let value = value.as_f64().unwrap_or(default);
            let left = if stacked {
                c.x
            } else {
                c.x + label_width + b.px(8.0)
            };
            let r = Rect::new(left, c.y, c.right() - left, height(b, 36.0));
            b.slider(
                key(("panel-bar-setting", id)),
                r,
                value,
                (min, max, step),
                &crate::panel::controls::unit(value * factor, unit),
                b.px(60.0),
                label,
                Action::Slider(edit),
            );
            c.y = r.bottom + b.px(4.0);
        }
        Kind::Text { placeholder } => field(
            app,
            b,
            c,
            FieldId::Text(DocId::Bars, edit.path),
            label,
            placeholder,
            value.as_str().unwrap_or(""),
        ),
        Kind::Custom => {}
    }
}

pub fn overview(app: &App, b: &mut B, c: &mut Col) {
    page_title(app,
        b,
        c,
        "Bars",
        &format!(
            "Top bar and dock · {}",
            crate::panel::controls::display_name(app, app.nav.display)
        ),
    );
    crate::panel::controls::displays(app, b, c);
    crate::panel::controls::icon_group(b, c, "Desktop bars", glyph::BARS, |b, c| {
        for (bar, page, label) in [
            (Bar::Top, Page::Top, "Top bar"),
            (Bar::Dock, Page::Dock, "Dock"),
        ] {
            let on = bar_value(app, &format!("/{}/enabled", bar.key()))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            row(
                b,
                c,
                Some(if bar == Bar::Top { glyph::BARS } else { glyph::APP }),
                label,
                &format!(
                    "{} · {}",
                    if on { "On" } else { "Off" },
                    design_name(app, Some(bar))
                ),
                "",
                None,
                open(page),
            );
        }
        let mut taskbar = Col { x: c.x + b.px(28.0), w: c.w - b.px(28.0), ..*c };
        b.icon(Rect::new(c.x, c.y, b.px(18.0), height(b, 30.0)), glyph::MONITOR, 16.0, b.p.ink3);
        crate::panel::controls::setting(
            app,
            b,
            &mut taskbar,
            "taskbar_mode",
            "Windows' taskbar",
            "All displays",
        );
        c.y = taskbar.y;
    });
    crate::panel::controls::icon_group(b, c, "Design", glyph::LOOK, |b, c| {
        row(
            b,
            c,
            Some(glyph::LOOK),
            "Preset",
            &design_name(app, None),
            "",
            None,
            Action::Panel(Command::Design(None)),
        );
    });
    display_differences(app, b, c);
    if more(app, b, c, "Panels and Windows' taskbar") {
        group(b, c, "", |b, c| {
            row(
                b,
                c,
                Some(glyph::TOGGLES),
                "Panels",
                "",
                "Start, sound, calendar, tray and notifications · All displays",
                None,
                open(Page::Panels),
            )
        });
        group(b, c, "Windows' taskbar", |b, c| {
            crate::panel::controls::setting(app, b, c, "taskbar_alignment", "Icons", "All displays")
        });
    }
}

fn display_differences(app: &App, b: &mut B, c: &mut Col) {
    if app.monitors.len() < 2 {
        return;
    }
    if app.nav.display == 0 {
        return;
    }
    let first = resolved_for(app, 0);
    let current = resolved(app);
    let difference = differences(&first, &current, "");
    let mut changed = Vec::new();
    collect_differences(&difference, "", &mut changed);
    changed.sort();
    changed.dedup();
    if changed.is_empty() {
        notice(
            b,
            c,
            &format!("Same as {}", crate::panel::controls::display_name(app, 0)),
        );
    } else {
        notice(b, c, &format!("Different here: {}", changed.join(", ")));
        action_button(
            b,
            c,
            &format!(
                "Use the same as {}",
                crate::panel::controls::display_name(app, 0)
            ),
            Action::Panel(Command::SameBars),
        );
    }
}

fn collect_differences(value: &Value, parent: &str, out: &mut Vec<String>) {
    if let Some(map) = value.as_object() {
        for (key, value) in map {
            let path = format!("{parent}/{key}");
            if let Some(setting) = crate::settings::SETTINGS
                .iter()
                .find(|s| s.doc == DocId::Bars && s.path == path)
            {
                out.push(setting.label.into());
            } else if matches!(
                key.as_str(),
                "design" | "preset" | "left" | "center" | "right" | "look" | "animation"
            ) {
                out.push(
                    match key.as_str() {
                        "design" | "preset" => "Design",
                        "look" => "Look",
                        "animation" => "Motion",
                        _ => "Modules",
                    }
                    .into(),
                );
            } else {
                collect_differences(value, &path, out);
            }
        }
    }
}

// Partial objects keep only fields whose resolved values differ from the reference display.
fn differences(base: &Value, current: &Value, path: &str) -> Value {
    if let (Some(base), Some(current)) = (base.as_object(), current.as_object()) {
        let mut out = serde_json::Map::new();
        for (key, value) in current {
            let at = format!("{path}/{key}");
            if shared(&at)
                || path.is_empty()
                    && matches!(key.as_str(), "version" | "overrides" | "vars" | "icons")
            {
                continue;
            }
            let reference = base.get(key).unwrap_or(&Value::Null);
            if value == reference {
                continue;
            }
            let difference = differences(reference, value, &at);
            if difference.as_object().is_none_or(|v| !v.is_empty()) {
                out.insert(key.clone(), difference);
            }
        }
        Value::Object(out)
    } else {
        current.clone()
    }
}

fn merge(target: &mut Value, layer: &Value) {
    if let (Some(target), Some(layer)) = (target.as_object_mut(), layer.as_object()) {
        for (key, value) in layer {
            merge(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    } else {
        *target = layer.clone();
    }
}

fn same_device(a: &str, b: &str) -> bool {
    a.trim_start_matches(r"\\.\")
        .eq_ignore_ascii_case(b.trim_start_matches(r"\\.\"))
}

/// Make Display 1 the inherited baseline, remove this display's differences, and preserve
/// every other connected display, including configs that used primary/secondary aliases.
pub fn same_bars(app: &mut App) {
    if app.nav.display == 0 || app.monitors.len() < 2 {
        return;
    }
    let raw = &app.docs.doc(DocId::Bars).raw;
    let original = raw
        .get("overrides")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let first = &app.monitors[0];
    let role = if first.primary {
        "primary"
    } else {
        "secondary"
    };
    let mut baseline = raw.clone();
    baseline.as_object_mut().unwrap().remove("overrides");
    // Role first, then the device, matching BarsConfig::for_monitor's precedence.
    for (_, layer) in original
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(role))
    {
        merge(&mut baseline, layer);
    }
    for (_, layer) in original
        .iter()
        .filter(|(key, _)| same_device(key, &first.device))
    {
        merge(&mut baseline, layer);
    }
    for path in ["/top/clock/format", "/dock/clock/format"] {
        crate::docs::apply_op(
            &mut baseline,
            &crate::docs::Op {
                doc: DocId::Bars,
                path: path.into(),
                value: raw.pointer(path).cloned(),
            },
        );
    }
    let Some(mut base_config) = mypaper_bars::BarsConfig::parse(&baseline.to_string()).ok() else {
        return;
    };
    base_config.resolve(Vec::new);
    let reference = serde_json::to_value(base_config).unwrap();
    let mut overrides: serde_json::Map<_, _> = original
        .into_iter()
        .filter(|(key, _)| {
            !key.eq_ignore_ascii_case("primary")
                && !key.eq_ignore_ascii_case("secondary")
                && !app.monitors.iter().any(|m| same_device(key, &m.device))
        })
        .collect();
    for (index, monitor) in app.monitors.iter().enumerate() {
        if index == 0 || index == app.nav.display {
            continue;
        }
        let difference = differences(&reference, &resolved_for(app, index), "");
        if difference.as_object().is_some_and(|v| !v.is_empty()) {
            overrides.insert(
                monitor.device.trim_start_matches(r"\\.\").into(),
                difference,
            );
        }
    }
    if !overrides.is_empty() {
        baseline["overrides"] = Value::Object(overrides);
    }
    app.apply(
        &format!(
            "Use the same as {}",
            crate::panel::controls::display_name(app, 0)
        ),
        vec![crate::docs::Op {
            doc: DocId::Bars,
            path: String::new(),
            value: Some(baseline),
        }],
    );
}

pub fn intercept_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if edit.doc == DocId::Bars
        && edit.path.ends_with("/interval")
        && let Some(text) = value.as_str()
    {
        match text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
        {
            Some(number) => app.apply(
                &edit.label,
                vec![crate::docs::Op {
                    doc: edit.doc,
                    path: edit.path.to_string(),
                    value: Some(Value::Number(number)),
                }],
            ),
            None => app.alert("Enter the interval as a number of seconds"),
        }
        return true;
    }
    if edit.doc != DocId::Studio {
        return false;
    }
    if edit.path.as_ref() == "/panel/dock-hide" {
        if let Ok(mode) = serde_json::from_value::<DockHide>(value.clone()) {
            let (autohide, hide_when_maximized) = mode.flags();
            // One undoable edit of the existing fields, scoped to the selected display.
            let ops = [
                ("/dock/autohide", autohide),
                ("/dock/hide_when_maximized", hide_when_maximized),
            ]
            .into_iter()
            .map(|(path, value)| crate::docs::Op {
                doc: DocId::Bars,
                path: bar_path(app, path),
                value: Some(json!(value)),
            })
            .collect();
            app.apply(&edit.label, ops);
        }
        return true;
    }
    if let Some(which) = edit.path.strip_prefix("/panel/pins/") {
        let which = if which == "dock" {
            Pins::Dock
        } else {
            Pins::Start
        };
        if value.as_bool() == Some(true) {
            app.run(Action::PinsOwn(which));
        } else {
            app.run(Action::Unset(Edit::new(
                DocId::Bars,
                which.path(),
                "Follow pins",
            )));
        }
        return true;
    }
    if let Some(module) = edit.path.strip_prefix("/panel/module-args/") {
        match crate::library::split_args(value.as_str().unwrap_or("")) {
            Ok(args) => app.set(
                &Edit::new(
                    DocId::Bars,
                    bar_path(app, &format!("/modules/{module}/args")),
                    "Module arguments",
                ),
                json!(args),
            ),
            Err(error) => app.alert(error.to_string()),
        }
        return true;
    }
    if let Some(path) = edit.path.strip_prefix("/panel/gesture") {
        if let Some(path) = path.strip_prefix("-target/") {
            let Some((kind, tail)) = path.split_once('/') else {
                return false;
            };
            if !matches!(kind, "exec" | "open") {
                return false;
            }
            let target = value.as_str().unwrap_or("").trim();
            if !target.is_empty() {
                let path = format!("/{tail}");
                let args = bar_value(app, &path)
                    .and_then(|v| v.get("args").cloned())
                    .unwrap_or_else(|| json!([]));
                let value = if kind == "exec" {
                    json!({"exec":target,"args":args})
                } else {
                    json!({"open":target})
                };
                app.set(
                    &Edit::new(DocId::Bars, bar_path(app, &path), edit.label.clone()),
                    value,
                );
                app.texts.remove(&FieldId::Text(
                    DocId::Studio,
                    format!("/panel/draft/gesture{path}").into(),
                ));
            }
        } else {
            let mode = value.as_str().unwrap_or("default");
            let draft = FieldId::Text(DocId::Studio, format!("/panel/draft/gesture{path}").into());
            if matches!(mode, "program" | "open") {
                app.texts.insert(draft, mode.into());
            } else {
                app.texts.remove(&draft);
                app.set(
                    &Edit::new(DocId::Bars, bar_path(app, path), edit.label.clone()),
                    json!(mode),
                );
            }
        }
        return true;
    }
    if edit.path.as_ref() == "/panel/bar-preset" {
        let mut ops = vec![crate::docs::Op {
            doc: DocId::Bars,
            path: bar_path(app, "/preset"),
            value: Some(value.clone()),
        }];
        let id = value.as_str().unwrap_or("monochrome");
        let base = mypaper_bars::BarsConfig::parse(&json!({"preset":id}).to_string())
            .ok()
            .and_then(|v| serde_json::to_value(v).ok());
        for bar in ["top", "dock"] {
            for field in ["design", "enabled"] {
                let path = format!("/{bar}/{field}");
                ops.push(crate::docs::Op {
                    doc: DocId::Bars,
                    path: bar_path(app, &path),
                    value: base.as_ref().and_then(|v| v.pointer(&path)).cloned(),
                });
            }
        }
        app.apply(&edit.label, ops);
        return true;
    }
    let Some(bar) = edit
        .path
        .strip_prefix("/panel/bar-reset/")
        .filter(|bar| matches!(*bar, "top" | "dock"))
    else {
        return false;
    };
    app.apply(
        &edit.label,
        ["look", "animation"]
            .into_iter()
            .map(|part| crate::docs::Op {
                doc: DocId::Bars,
                path: bar_path(app, &format!("/{bar}/{part}")),
                value: None,
            })
            .collect(),
    );
    true
}

pub fn designs(app: &App, b: &mut B, c: &mut Col) {
    let bar = app.nav.bar;
    page_title(app,
        b,
        c,
        if bar.is_some() { "Design" } else { "Preset" },
        &format!(
            "{} · {}",
            match bar {
                Some(Bar::Top) => "Top bar",
                Some(Bar::Dock) => "Dock",
                None => "Top bar and dock",
            },
            crate::panel::controls::display_name(app, app.nav.display)
        ),
    );
    let designs: &[(&str, &str)] = match bar {
        Some(Bar::Top) => &crate::editing::bars::TOP_DESIGNS,
        Some(Bar::Dock) => &crate::editing::bars::DOCK_DESIGNS,
        None => &crate::editing::bars::PRESETS,
    };
    let path = bar.map_or_else(|| "/preset".into(), |bar| format!("/{}/design", bar.key()));
    let current = bar_value(app, &path).unwrap_or(json!("monochrome"));
    let pair = bar.is_some() || same_pair(app);
    let choices: Vec<_> = designs
        .iter()
        .map(|(id, label)| {
            (
                *label,
                pair && current == *id,
                if bar.is_none() {
                    if app.monitors.len() < 2 {
                        Action::Preset(id)
                    } else {
                        Action::Set(
                            Edit::new(DocId::Studio, "/panel/bar-preset", "Bar design"),
                            json!(id),
                        )
                    }
                } else {
                    Action::Set(
                        Edit::new(DocId::Bars, bar_path(app, &path), "Bar design"),
                        json!(id),
                    )
                },
            )
        })
        .collect();
    crate::panel::choosers::draw(
        app,
        b,
        c,
        key(("panel-bar-designs", bar)),
        b.px(if bar == Some(Bar::Top) { 72.0 } else { 196.0 }),
        &choices,
        |b, index, pic| {
            let mut raw = match (index, bar) {
                (Some(index), Some(bar)) => {
                    json!({bar.key(): {"design":designs[index].0,"enabled":true}})
                }
                (Some(index), None) => json!({"preset":designs[index].0}),
                (None, _) => resolved(app),
            };
            raw["dock"]["pinned"] = bar_value(app, "/dock/pinned").unwrap_or(json!([]));
            crate::panel::specimens::backdrop(app, b, pic);
            crate::panel::specimens::bars(app, b, pic, &raw, bar);
        },
    );
}

pub fn bar(app: &App, b: &mut B, c: &mut Col, page: Page) {
    let bar = if page == Page::Dock {
        Bar::Dock
    } else {
        Bar::Top
    };
    page_title(app,
        b,
        c,
        page.title(),
        &format!(
            "{} · {}",
            design_name(app, Some(bar)),
            crate::panel::controls::display_name(app, app.nav.display)
        ),
    );
    crate::panel::controls::displays(app, b, c);
    group(b, c, "", |b, c| {
        let ids: &[&str] = if bar == Bar::Top {
            &["top_enabled", "top_edge", "top_height"]
        } else {
            &[
                "dock_enabled",
                "dock_edge",
                "dock_size",
                "dock_magnify",
                "dock_hide",
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
        let path = format!("/{}/look/shadow/rest", bar.key());
        let shadow = bar_value(app, &path).unwrap_or(Value::Null);
        let on = shadow.get("color").and_then(Value::as_str) != Some("transparent");
        toggle(
            b,
            c,
            "Shadow",
            "",
            on,
            Action::Set(
                Edit::new(DocId::Bars, bar_path(app, &path), "Bar shadow"),
                if on {
                    json!({"color":"transparent"})
                } else {
                    json!({"color":"#00000059","y":4,"blur":16})
                },
            ),
        );
    });
    group(b, c, "", |b, c| {
        row(
            b,
            c,
            None,
            "Design",
            &design_name(app, Some(bar)),
            "",
            None,
            Action::Panel(Command::Design(Some(bar))),
        )
    });
    display_differences(app, b, c);
    if bar == Bar::Dock {
        pinned_apps(app, b, c, Pins::Dock);
    }
    super::group_action(
        b,
        c,
        "Modules",
        "Add a module",
        Action::AddModule(bar),
        |b, c| {
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
                let zone_top = c.y;
                c.y += b.px(28.0);
                let modules = bar_value(app, &format!("/{}/{list}", bar.key()))
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                crate::panel::lists::zone_target(
                    app,
                    b,
                    Rect::new(c.x, zone_top, c.w, b.px(28.0)),
                    bar,
                    list,
                    modules.len(),
                );
                module_rows(app, b, c, bar, list, &modules);
                if modules.is_empty() {
                    let top = c.y;
                    notice(b, c, "No modules here");
                    crate::panel::lists::zone_target(
                        app,
                        b,
                        Rect::new(c.x, top, c.w, c.y - top),
                        bar,
                        list,
                        0,
                    );
                }
            }
        },
    );
    if more(app, b, c, "Layout, look, motion, displays") {
        if bar == Bar::Top {
            number(
                app,
                b,
                c,
                DocId::Bars,
                &bar_path(app, "/top/look/radius"),
                "Corners",
                (0.0, 64.0, 1.0),
                8.0,
            );
        }
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
                        && !matches!(
                            s.id,
                            "top_shadow"
                                | "dock_shadow"
                                | "dock_style_own"
                                | "top_displays"
                                | "dock_displays"
                        )
                }) {
                    setting(app, b, c, s.id, s.label, "");
                }
            });
        }
        action_button(
            b,
            c,
            "Follow the design's look and motion",
            Action::Set(
                Edit::new(
                    DocId::Studio,
                    format!("/panel/bar-reset/{}", bar.key()),
                    "Follow bar design",
                ),
                Value::Null,
            ),
        );
    }
}

fn module_rows(app: &App, b: &mut B, c: &mut Col, bar: Bar, list: &'static str, modules: &[Value]) {
    for (index, module) in modules.iter().enumerate() {
        let id = module.as_str().unwrap_or("Custom module");
        let action = click_label(app, id, bar);
        let pic = crate::panel::lists::row_detail(
            app,
            b,
            c,
            &crate::panel::SortList::Modules(bar, list),
            index,
            name(id),
            if id == "|" || action == default_click(app, id, bar) {
                ""
            } else {
                &action
            },
            id == "|",
            Some(crate::panel::icons::module(id)),
            Some(Action::SelectModule(Some(Slot { bar, list, index }))),
        );
        if let Some(image) = crate::panel::icons::plugin_image(app, id) {
            b.fill(pic, b.px(radius::XS), b.p.panel);
            crate::stage::image_round(b, pic, &image, 1.0, b.px(4.0));
        }
    }
}

pub fn selected_module_name(app: &App) -> Option<String> {
    let slot = app.nav.module.as_ref()?;
    let list = bar_value(app, &format!("/{}/{}", slot.bar.key(), slot.list))?;
    let id = list.as_array()?.get(slot.index)?.as_str()?;
    Some(name(id).to_owned())
}

pub fn module(app: &App, b: &mut B, c: &mut Col) {
    let Some(slot) = &app.nav.module else { return };
    let list = bar_value(app, &format!("/{}/{}", slot.bar.key(), slot.list));
    let Some(id) = list
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|l| l.get(slot.index))
        .and_then(Value::as_str)
    else {
        return;
    };
    page_title(app,
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
    group(b, c, "Place", |b, c| {
        for (bar, zone, label) in [
            (Bar::Top, "left", "Top bar \u{b7} Start"),
            (Bar::Top, "center", "Top bar \u{b7} Middle"),
            (Bar::Top, "right", "Top bar \u{b7} End"),
            (Bar::Dock, "left", "Dock \u{b7} Start"),
            (Bar::Dock, "right", "Dock \u{b7} End"),
        ] {
            row(
                b,
                c,
                None,
                label,
                "",
                "",
                Some(slot.bar == bar && slot.list == zone),
                Action::Panel(Command::MoveModuleTo(bar, zone)),
            );
        }
    });
    let base = format!("/modules/{}", crate::docs::token(id));
    let document = resolved(app);
    if id == "clock" {
        group(b, c, "Shows", |b, c| {
            setting(
                app,
                b,
                c,
                if slot.bar == Bar::Top {
                    "top_clock_seconds"
                } else {
                    "dock_clock_seconds"
                },
                "Seconds",
                "",
            );
            let path = format!("/{}/clock/format", slot.bar.key());
            notice(b, c, "Clock format · All displays");
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Bars, path.clone().into()),
                "Format",
                "HH:mm",
                document
                    .pointer(&path)
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        });
    }
    group(b, c, "Clicks", |b, c| {
        for (trigger, label) in [
            ("left", "Click"),
            ("middle", "Middle click"),
            ("right", "Right click"),
            ("wheelUp", "Scroll up"),
            ("wheelDown", "Scroll down"),
        ] {
            let path = format!("{base}/click/{trigger}");
            let current = bar_value(app, &path).unwrap_or(Value::Null);
            let path = bar_path(app, &path);
            let mode_key = format!("/panel/draft/gesture{path}");
            let mode = app
                .texts
                .get(&FieldId::Text(DocId::Studio, mode_key.into()))
                .map(String::as_str)
                .unwrap_or_else(|| {
                    if current.get("exec").is_some() {
                        "program"
                    } else if current.get("open").is_some() {
                        "open"
                    } else {
                        current.as_str().unwrap_or("default")
                    }
                });
            let mut options: Vec<_> = [
                ("default", "Its own action"),
                ("open", "Open…"),
                ("program", "A program…"),
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
            .map(|(value, label)| (json!(value), label.into()))
            .collect();
            if !options.iter().any(|(value, _)| value == mode) {
                options.push((json!(mode), click_label(app, id, slot.bar)));
            }
            select(
                b,
                c,
                label,
                json!(mode),
                Edit::new(DocId::Studio, format!("/panel/gesture{path}"), label),
                options,
            );
            if matches!(mode, "program" | "open") {
                let key = if mode == "program" { "exec" } else { "open" };
                let value = current.get(key).and_then(Value::as_str).unwrap_or("");
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(
                        DocId::Studio,
                        format!("/panel/gesture-target/{key}{path}").into(),
                    ),
                    if mode == "program" {
                        "Program"
                    } else {
                        "File, folder or link"
                    },
                    if mode == "program" {
                        "notepad.exe"
                    } else {
                        "https:// or a file path"
                    },
                    value,
                );
            }
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
                    FieldId::Text(DocId::Bars, bar_path(app, &path).into()),
                    label,
                    "",
                    document
                        .pointer(&path)
                        .map(|value| {
                            value
                                .as_str()
                                .map_or_else(|| value.to_string(), str::to_owned)
                        })
                        .as_deref()
                        .unwrap_or(""),
                );
            }
            let args = document
                .pointer(&format!("{base}/args"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|arg| {
                    if arg.contains(' ') {
                        format!("\"{arg}\"")
                    } else {
                        arg.into()
                    }
                })
                .collect::<Vec<String>>()
                .join(" ");
            field(
                app,
                b,
                c,
                FieldId::Text(
                    DocId::Studio,
                    format!("/panel/module-args/{}", crate::docs::token(id)).into(),
                ),
                "Arguments",
                "No arguments",
                &args,
            );
        });
    }
    if more(app, b, c, "Icon, colors and room") {
        group(b, c, "Look", |b, c| {
            let path = format!("{base}/icon");
            field(
                app,
                b,
                c,
                FieldId::Text(DocId::Bars, bar_path(app, &path).into()),
                "Icon",
                "From the bar",
                document
                    .pointer(&path)
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
            for (field, label) in [
                ("color", "Text color"),
                ("background", "Background"),
                ("border/color", "Edge color"),
            ] {
                let path = format!("{base}/style/{field}");
                text(
                    b,
                    Rect::new(c.x, c.y, c.w, b.px(26.0)),
                    label,
                    13.0,
                    400,
                    b.p.ink1,
                    Align::Start,
                );
                c.y += b.px(28.0);
                crate::panel::colors::swatches(
                    app,
                    b,
                    c,
                    &Edit::new(DocId::Bars, bar_path(app, &path), label),
                    document.pointer(&path),
                    false,
                    false,
                    &[
                        [139, 158, 183],
                        [127, 174, 142],
                        [200, 141, 60],
                        [230, 230, 230],
                    ],
                );
            }
            for (field, label, range, default) in [
                ("padding", "Padding", (0.0, 32.0, 1.0), 8.0),
                ("radius", "Corners", (0.0, 32.0, 1.0), 6.0),
                ("size", "Text size", (8.0, 32.0, 1.0), 13.0),
                ("weight", "Text weight", (100.0, 900.0, 100.0), 400.0),
            ] {
                number(
                    app,
                    b,
                    c,
                    DocId::Bars,
                    &bar_path(app, &format!("{base}/style/{field}")),
                    label,
                    range,
                    default,
                );
            }
        });
    }
}

pub fn add(app: &App, b: &mut B, c: &mut Col) {
    let bar = app.nav.bar.unwrap_or(Bar::Top);
    let document = resolved(app);
    page_title(app,
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
    let present: Vec<_> = ["left", "center", "right"]
        .into_iter()
        .flat_map(|side| {
            document
                .pointer(&format!("/{}/{side}", bar.key()))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
        })
        .collect();
    let builtins: Vec<_> = MODULES
        .into_iter()
        .filter(|(id, _)| !present.contains(id))
        .collect();
    if builtins.is_empty() {
        notice(b, c, "Every built-in module is already on this bar");
    } else {
        group(b, c, "Built in", |b, c| {
            for (id, name) in builtins {
                row(
                    b,
                    c,
                    Some(crate::panel::icons::module(id)),
                    name,
                    "",
                    "",
                    Some(selected == Some(id)),
                    Action::Panel(Command::PickModule(id.into())),
                );
            }
        });
    }
    let from_apps: Vec<_> = app
        .plugins
        .installed
        .iter()
        .filter(|plugin| super::connections::visible_plugin(plugin))
        .flat_map(|plugin| plugin.bars.iter().map(move |module| (plugin, module)))
        .filter(|(plugin, module)| {
            !present.contains(&format!("custom:{}--{}", plugin.id, module.id).as_str())
        })
        .collect();
    if !from_apps.is_empty() {
        group(b, c, "Your apps", |b, c| {
            for (plugin, module) in from_apps {
                let id = format!("custom:{}--{}", plugin.id, module.id);
                crate::panel::icons::row(
                    app,
                    b,
                    c,
                    &id,
                    glyph::APP,
                    &module.name,
                    &plugin.name,
                    selected == Some(id.as_str()),
                    Action::Panel(Command::PickModule(id.clone())),
                );
            }
        });
    }
    group(b, c, "Spacing", |b, c| {
        let top = c.y;
        row(
            b,
            c,
            Some(mypaper_ui::controls::Glyph::Font(' ')),
            "Gap",
            "",
            "",
            Some(selected == Some("|")),
            Action::Panel(Command::PickModule("|".into())),
        );
        crate::panel::controls::spacing_glyph(
            b,
            Rect::new(
                c.x,
                top + (height(b, 40.0) - b.px(18.0)) / 2.0,
                b.px(18.0),
                b.px(18.0),
            ),
        );
    });
    group(b, c, "Your own", |b, c| {
        row(
            b,
            c,
            Some(mypaper_ui::icons::Icon::Terminal.into()),
            "Your own program",
            "",
            "Shows the program's output",
            Some(selected == Some("custom:")),
            Action::Panel(Command::PickModule("custom:".into())),
        );
        if selected == Some("custom:") {
            for (id, label, placeholder) in [
                ("name", "Name", "Weather line"),
                ("program", "Program", "C:\\tools\\weather.exe"),
                ("args", "Arguments (optional)", "--compact"),
            ] {
                field(
                    app,
                    b,
                    c,
                    FieldId::Text(
                        DocId::Studio,
                        format!("/panel/draft/add-module/{id}").into(),
                    ),
                    label,
                    placeholder,
                    "",
                );
            }
        }
    });
}

/// Draft fields become existing bars.json module options in the same edit as insertion.
fn custom_draft(app: &App) -> Option<(String, Value)> {
    let draft = |name| {
        app.text(&FieldId::Text(
            DocId::Studio,
            format!("/panel/draft/add-module/{name}").into(),
        ))
    };
    let id = module_id(draft("name"));
    let program = draft("program").trim();
    if id.is_empty() || program.is_empty() {
        return None;
    }
    let args = crate::library::split_args(draft("args")).ok()?;
    let id = format!("custom:{id}");
    if resolved(app)
        .get("modules")
        .is_some_and(|modules| modules.get(&id).is_some())
    {
        return None;
    }
    Some((id, json!({"command":program,"args":args})))
}

pub fn insert_module(app: &mut App) {
    let bar = app.nav.bar.unwrap_or(Bar::Top);
    let list = match app.text(&FieldId::Text(DocId::Studio, "/add_module_list".into())) {
        "left" => "left",
        "center" if bar == Bar::Top => "center",
        _ => "right",
    };
    if let Some(id) = app.panel.as_ref().unwrap().add_module.clone()
        && can_insert_module(app, &id)
    {
        let custom = (id == "custom:").then(|| custom_draft(app)).flatten();
        let id = custom.as_ref().map_or(id, |(id, _)| id.clone());
        let path = format!("/{}/{list}", bar.key());
        let mut items = bar_value(app, &path)
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let index = items.len();
        items.push(json!(id));
        let before = app.docs.version;
        let mut ops = vec![crate::docs::Op {
            doc: DocId::Bars,
            path: bar_path(app, &path),
            value: Some(json!(items)),
        }];
        if let Some((id, options)) = &custom {
            ops.push(crate::docs::Op {
                doc: DocId::Bars,
                path: bar_path(app, &format!("/modules/{}", crate::docs::token(id))),
                value: Some(options.clone()),
            });
        }
        app.apply("Add module", ops);
        if app.docs.version == before {
            return;
        }
        app.overlay = None;
        app.toast_undo("Added a module");
        app.panel.as_mut().unwrap().page = if custom.is_some() {
            app.nav.module = Some(Slot { bar, list, index });
            for name in ["name", "program", "args"] {
                app.texts.remove(&FieldId::Text(
                    DocId::Studio,
                    format!("/panel/draft/add-module/{name}").into(),
                ));
            }
            Page::Module
        } else if bar == Bar::Top {
            Page::Top
        } else {
            Page::Dock
        };
    }
}

pub fn can_insert_module(app: &App, id: &str) -> bool {
    let custom = if id == "custom:" {
        custom_draft(app)
    } else {
        None
    };
    if id == "custom:" && custom.is_none() {
        return false;
    }
    let id = custom.as_ref().map_or(id, |(id, _)| id.as_str());
    let bar = app.nav.bar.unwrap_or(Bar::Top);
    let document = resolved(app);
    !id.is_empty()
        && (id == "|"
            || ["left", "center", "right"].into_iter().all(|side| {
                !document
                    .pointer(&format!("/{}/{side}", bar.key()))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|v| v.as_str() == Some(id))
            }))
}

pub fn panels(app: &App, b: &mut B, c: &mut Col) {
    page_title(app, b, c, "Panels", "The things your bars open");
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
    page_title(app, b, c, name, "A panel the bars open · All displays");
    if panel == Panel::Start {
        pinned_apps(app, b, c, Pins::Start);
    }
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
                    &zone["label"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            crate::panel::pickers::city_name(zone["zone"].as_str().unwrap_or(""))
                        }),
                    "Remove",
                    "",
                    None,
                    Action::Set(
                        Edit::new(DocId::Bars, "/calendar/zones", "Time zones"),
                        Value::Array(remaining),
                    ),
                );
            }
            if zones.len() < 4 {
                crate::panel::pickers::zones(
                    app,
                    b,
                    c,
                    crate::panel::pickers::ZoneTarget::Calendar,
                    "Add a city",
                );
            }
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

fn pinned_apps(app: &App, b: &mut B, c: &mut Col, which: Pins) {
    group(b, c, "Pinned apps · All displays", |b, c| {
        let own = app
            .get(DocId::Bars, which.path())
            .is_some_and(Value::is_array);
        let follows = if which == Pins::Dock {
            "Windows' pins"
        } else {
            "Dock's pins"
        };
        segments(
            b,
            c,
            "Use",
            json!(own),
            Edit::new(
                DocId::Studio,
                format!(
                    "/panel/pins/{}",
                    if which == Pins::Dock { "dock" } else { "start" }
                ),
                "Pinned apps",
            ),
            vec![
                (json!(true), "Your pins".into()),
                (json!(false), follows.into()),
            ],
        );
        let pins = app
            .get(
                DocId::Bars,
                if !own && which == Pins::Start {
                    "/dock/pinned"
                } else {
                    which.path()
                },
            )
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (i, pin) in pins.iter().enumerate() {
            let title = pin["name"].as_str().map(str::to_owned).unwrap_or_else(|| {
                mypaper_bars::apps::file_stem(pin["path"].as_str().unwrap_or("App"))
            });
            let pic = if own {
                crate::panel::lists::row(
                    app,
                    b,
                    c,
                    &crate::panel::SortList::Pins(which),
                    i,
                    &title,
                    false,
                    Some(glyph::APP),
                    None,
                )
            } else {
                let r = Rect::new(c.x, c.y, c.w, height(b, 36.0));
                let pic = Rect::new(r.left, r.top + b.px(7.0), b.px(22.0), b.px(22.0));
                b.icon(pic, glyph::APP, 16.0, b.p.ink2);
                text(
                    b,
                    Rect::new(
                        pic.right + b.px(8.0),
                        r.top,
                        r.width() - b.px(30.0),
                        r.height(),
                    ),
                    &title,
                    13.0,
                    400,
                    b.p.ink2,
                    Align::Start,
                );
                c.y = r.bottom;
                pic
            };
            if let Some(path) = pin["path"].as_str()
                && let Some(image) = crate::panel::icons::image(app, path)
            {
                b.fill(pic, b.px(radius::XS), b.p.panel);
                crate::stage::image_round(b, pic, &image, 1.0, b.px(4.0));
            }
        }
        if own {
            action_button(b, c, "Pin an app", Action::PinAdd(which));
        }
    });
}

/// Free names become schema-valid identifiers; punctuation and repeated separators collapse.
fn module_id(name: &str) -> String {
    if name.trim().is_empty() {
        return String::new();
    }
    let mut id = name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    id.truncate(64);
    let id = id.trim_matches('-');
    if id.is_empty() {
        "module".into()
    } else {
        id.into()
    }
}

pub fn move_module(app: &mut App, bar: Bar, zone: &'static str) {
    let Some(slot) = app.nav.module.clone() else {
        return;
    };
    if slot.bar == bar && slot.list == zone {
        return;
    }
    let index = bar_value(app, &format!("/{}/{zone}", bar.key()))
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(0);
    crate::panel::lists::move_module(app, slot, bar, zone, index);
}

fn click_label(app: &App, id: &str, bar: Bar) -> String {
    let document = resolved(app);
    let own = document.pointer(&format!("/modules/{}/click/left", crate::docs::token(id)));
    if let Some(program) = own
        .and_then(|value| value.get("exec"))
        .and_then(Value::as_str)
    {
        return format!("Run {}", mypaper_bars::apps::file_stem(program));
    }
    if let Some(target) = own
        .and_then(|value| value.get("open"))
        .and_then(Value::as_str)
    {
        return format!("Open {target}");
    }
    let default = default_click(app, id, bar);
    let name = match own.and_then(Value::as_str).unwrap_or("default") {
        "default" => default,
        "calendar" => "Calendar",
        "mixer" => "Volume mixer",
        "quick-settings" => "Quick settings",
        "start" => "Start menu",
        "search" => "Search",
        "media-toggle" => "Play / pause",
        "media-next" => "Next track",
        "media-previous" => "Previous track",
        "network" => "Wi-Fi",
        "notifications" => "Notifications",
        "native-start" => "Windows Start",
        "native-notifications" => "Windows notifications",
        "native-quick-settings" => "Windows quick settings",
        "task-manager" => "Task Manager",
        "settings" => "Windows Settings",
        "show-desktop" => "Show desktop",
        "volume-up" => "Volume up",
        "volume-down" => "Volume down",
        "volume-mute" => "Mute",
        "workspace-next" => "Next desktop",
        "workspace-previous" => "Previous desktop",
        "lock" => "Lock",
        "explorer" => "File Explorer",
        "none" => "No action",
        other => other,
    };
    name.into()
}

fn default_click(app: &App, id: &str, bar: Bar) -> &'static str {
    let document = resolved(app);
    let grouped = document
        .pointer(&format!("/{}/look/family", bar.key()))
        .and_then(Value::as_str)
        == Some("fluent");
    match id {
        "start" => "Start menu",
        "search" => "Search",
        "clock" | "notifications" => "Notifications",
        "quick" => "Quick settings",
        "network" | "volume" | "battery" if grouped => "Quick settings",
        "network" => "Wi-Fi",
        "volume" => "Volume mixer",
        "battery" => "Battery settings",
        "power" => "Power menu",
        "language" => "Keyboard language",
        "stats" => "Task Manager",
        "media" => "Play / pause",
        "workspaces" => "Switch desktop",
        "tray" => "Tray icon's action",
        "app" => "No action",
        _ if id.starts_with("custom:") => "Connection action",
        _ => "No action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_visibility_updates_only_legacy_flags_on_selected_display_and_undoes_together() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui, json!({"dock":{"autohide":true,"hide_when_maximized":true},
            "top":{"height":40}}));
        app.nav.display = 1;
        let before = app.docs.doc(DocId::Bars).raw.clone();
        app.set(&Edit::new(DocId::Studio, "/panel/dock-hide", "Dock visibility"), json!(DockHide::Never));
        let after = &app.docs.doc(DocId::Bars).raw;
        assert_eq!(after["dock"], before["dock"]);
        assert_eq!(after["top"], before["top"]);
        assert_eq!(after["overrides"]["DISPLAY2"]["dock"], json!({"autohide":false,"hide_when_maximized":false}));
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
    }

    #[test]
    fn render_dock_visibility_window_goldens() {
        let baseline = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
        let mut ui = mypaper_ui::Ui::new().unwrap();
        for dark in [true, false] {
            let mut app = crate::goldens::window_app(dark, &ui, (1280, 800));
            app.run(Action::Panel(Command::Open(Page::Dock)));
            let scheme = if dark { "dark" } else { "light" };
            let image = crate::goldens::shot(&mut app, &mut ui);
            mypaper_ui::golden::check(&image, &baseline.join(format!("dock-visibility-1280x800-{scheme}.png"))).unwrap();
            let choices = app.frame.targets.iter().find_map(|target| match &target.action {
                Action::Choose(choices) if choices.edit.path.as_ref() == "/panel/dock-hide" => Some(choices.clone()),
                _ => None,
            }).expect("dock visibility selector");
            assert_eq!(choices.options.len(), 3);
            let items: Vec<_> = choices.options.iter().enumerate().map(|(i, (value, label))|
                mypaper_ui::menu::Entry::check(label, i, choices.current.as_ref() == Some(value))).collect();
            let p = crate::panel::palette_for(&app);
            let mut tokens = p.theme().tokens();
            tokens.material.tint = p.panel;
            tokens.colors.hover = p.selected;
            let layout = mypaper_ui::menu::layout(&items, &tokens, 1.0, &mut ui.text);
            let list = mypaper_ui::menu::view(&items, &layout, None, &tokens);
            let size = layout.size();
            let image = mypaper_ui::render(&mut ui, &list, size.0 as u32 + 32, size.1 as u32 + 32).unwrap();
            mypaper_ui::golden::check(&image, &baseline.join(format!("dock-visibility-choices-{scheme}.png"))).unwrap();
            for (i, mode) in [DockHide::Never, DockHide::WhenFilled, DockHide::Always].into_iter().enumerate() {
                app.chose(&choices, i);
                let flags = mode.flags();
                assert_eq!(bar_value(&app, "/dock/autohide"), Some(json!(flags.0)));
                assert_eq!(bar_value(&app, "/dock/hide_when_maximized"), Some(json!(flags.1)));
            }
        }
    }

    fn fixture(ui: &mypaper_ui::Ui, bars: Value) -> App {
        let mut app = crate::goldens::app(true, ui);
        app.docs = crate::docs::Docs::fixture(["{}", &bars.to_string(), "{}", "{}"]);
        app.panel = Some(crate::panel::State::default());
        app
    }

    #[test]
    fn display_edits_are_isolated_and_reset_removes_layers_without_changing_other_displays() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(
            &ui,
            json!({"top":{"height":32},"overrides":{"primary":{"top":{"height":40}},"secondary":{"top":{"height":60}},"DISPLAY2":{"top":{"height":72}}}}),
        );
        let mut third = app.monitors[1].clone();
        third.device = r"\\.\DISPLAY3".into();
        app.monitors.push(third);
        app.nav.display = 0;
        assert_eq!(
            bar_path(&app, "/top/height"),
            "/overrides/DISPLAY1/top/height"
        );
        app.nav.display = 1;
        let before_first = resolved_for(&app, 0);
        let before_third = resolved_for(&app, 2);
        let edit = Edit::new(DocId::Bars, bar_path(&app, "/top/height"), "Height");
        app.set(&edit, json!(80));
        assert_eq!(bar_value(&app, "/top/height"), Some(json!(80.0)));
        assert_eq!(resolved_for(&app, 0), before_first);
        assert_eq!(resolved_for(&app, 2), before_third);
        same_bars(&mut app);
        assert!(app.get(DocId::Bars, "/overrides/DISPLAY2").is_none());
        assert_eq!(resolved_for(&app, 1), before_first);
        assert_eq!(resolved_for(&app, 2), before_third);
        assert_eq!(bar_path(&app, "/dock/pinned"), "/dock/pinned");
        assert_eq!(bar_path(&app, "/taskbar/mode"), "/taskbar/mode");
        app.run(Action::Undo);
        assert_eq!(bar_value(&app, "/top/height"), Some(json!(80.0)));
    }

    #[test]
    fn program_gestures_stay_drafts_until_valid_and_save_on_the_selected_display() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui, json!({}));
        app.nav.display = 1;
        let version = app.docs.version;
        app.set(
            &Edit::new(
                DocId::Studio,
                "/panel/gesture/modules/start/click/left",
                "Click",
            ),
            json!("program"),
        );
        assert_eq!(app.docs.version, version);
        app.set(
            &Edit::new(
                DocId::Studio,
                "/panel/gesture-target/exec/modules/start/click/left",
                "Program",
            ),
            json!("notepad.exe"),
        );
        assert_eq!(
            app.get(DocId::Bars, "/overrides/DISPLAY2/modules/start/click/left"),
            Some(&json!({"exec":"notepad.exe","args":[]}))
        );
        assert!(app.get(DocId::Bars, "/modules/start/click/left").is_none());
        app.run(Action::Undo);
        assert!(
            app.get(DocId::Bars, "/overrides/DISPLAY2/modules/start/click/left")
                .is_none()
        );
    }

    #[test]
    fn module_creation_uses_the_display_list_and_leaves_the_other_display_intact() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(
            &ui,
            json!({"top":{"left":["start"],"center":[],"right":[]}}),
        );
        let first = resolved_for(&app, 0);
        app.nav.display = 1;
        app.nav.bar = Some(Bar::Top);
        app.panel.as_mut().unwrap().add_module = Some("clock".into());
        insert_module(&mut app);
        assert_eq!(
            app.get(DocId::Bars, "/overrides/DISPLAY2/top/right"),
            Some(&json!(["clock"]))
        );
        assert_eq!(resolved_for(&app, 0), first);
        app.panel.as_mut().unwrap().page = Page::Module;
        app.nav.module = Some(Slot {
            bar: Bar::Top,
            list: "right",
            index: 0,
        });
        app.key_with(0x2E, (false, false, false));
        assert_eq!(bar_value(&app, "/top/right"), Some(json!([])));
        assert_eq!(resolved_for(&app, 0), first);
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Top);
    }

    #[test]
    fn free_module_names_produce_valid_bounded_ids() {
        assert_eq!(super::module_id(" Weather & CPU! (2) "), "weather-cpu-2");
        for name in ["!!!".to_owned(), "A".repeat(100)] {
            assert!(mypaper_widgets::manifest::valid_id(&super::module_id(
                &name
            )));
        }
        assert!(super::module_id("   ").is_empty());
    }

    #[test]
    fn custom_module_creation_saves_program_and_arguments_in_one_display_edit_and_opens_it() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui, json!({"top":{"left":[],"center":[],"right":[]}}));
        app.nav.bar = Some(Bar::Top);
        app.nav.display = 1;
        app.panel.as_mut().unwrap().add_module = Some("custom:".into());
        assert!(!can_insert_module(&app, "custom:"));
        for (key, value) in [
            ("name", "Weather line"),
            ("program", "C:\\tools\\weather.exe"),
            ("args", "--city \"New York\""),
        ] {
            app.texts.insert(
                FieldId::Text(
                    DocId::Studio,
                    format!("/panel/draft/add-module/{key}").into(),
                ),
                value.into(),
            );
        }
        assert!(can_insert_module(&app, "custom:"));
        let before = app.docs.doc(DocId::Bars).raw.clone();
        insert_module(&mut app);
        assert_eq!(app.docs.undo_count(), 1);
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Module);
        assert_eq!(
            app.nav.module,
            Some(Slot {
                bar: Bar::Top,
                list: "right",
                index: 0
            })
        );
        assert_eq!(
            app.get(
                DocId::Bars,
                "/overrides/DISPLAY2/modules/custom:weather-line/command"
            ),
            Some(&json!("C:\\tools\\weather.exe"))
        );
        assert_eq!(
            app.get(
                DocId::Bars,
                "/overrides/DISPLAY2/modules/custom:weather-line/args"
            ),
            Some(&json!(["--city", "New York"]))
        );
        mypaper_bars::BarsConfig::parse(&app.docs.doc(DocId::Bars).raw.to_string()).unwrap();
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
    }

    #[test]
    fn full_module_catalog_has_an_explanation_instead_of_empty_groups() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let modules: Vec<_> = MODULES.iter().map(|(id, _)| *id).collect();
        let mut app = fixture(&ui, json!({"top":{"left":modules,"center":[],"right":[]}}));
        app.size = (480, 760);
        app.nav.bar = Some(Bar::Top);
        app.panel.as_mut().unwrap().page = Page::AddModule;
        let built = crate::panel::build(&app, &mut ui.text);
        let labels: Vec<_> = built
            .list
            .ops
            .iter()
            .filter_map(|op| {
                if let mypaper_ui::Op::Text { text, .. } = op {
                    Some(text.as_ref())
                } else {
                    None
                }
            })
            .collect();
        assert!(labels.contains(&"Every built-in module is already on this bar"));
        assert!(!labels.contains(&"Built in"));
        assert!(!labels.contains(&"Your apps"));
        assert!(
            built
                .targets
                .iter()
                .any(|target| target.action == Action::Panel(Command::PickModule("|".into())))
        );
        app.panel.as_mut().unwrap().add_module = Some("|".into());
        insert_module(&mut app);
        assert_eq!(bar_value(&app, "/top/right"), Some(json!(["|"])));
    }

    #[test]
    fn render_complete_module_catalog_and_custom_flow() {
        let out = std::env::var_os("MYPAPER_STUDIO_PANEL_SHOTS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| r"C:\mypaper-targets\studio-panel\shots".into())
            .join("states");
        std::fs::create_dir_all(&out).unwrap();
        let mut ui = mypaper_ui::Ui::new().unwrap();
        for dark in [true, false] {
            for (width, text_scale) in [(480, 1.0), (360, 1.5)] {
                for custom in [false, true] {
                    let modules: Vec<_> = MODULES.iter().map(|(id, _)| *id).collect();
                    let mut app =
                        fixture(&ui, json!({"top":{"left":modules,"center":[],"right":[]}}));
                    app.size = (
                        width,
                        if custom && text_scale > 1.0 {
                            1100
                        } else {
                            760
                        },
                    );
                    app.theme.ty.text_scale = text_scale;
                    app.theme.palette = if dark {
                        mypaper_ui::controls::Palette::dark()
                    } else {
                        mypaper_ui::controls::Palette::light()
                    };
                    app.nav.bar = Some(Bar::Top);
                    app.panel.as_mut().unwrap().page = Page::AddModule;
                    if custom {
                        app.panel.as_mut().unwrap().add_module = Some("custom:".into());
                        for (id, value) in [
                            ("name", "Weather line"),
                            ("program", r"C:\tools\weather.exe"),
                            ("args", "--compact"),
                        ] {
                            app.texts.insert(
                                FieldId::Text(
                                    DocId::Studio,
                                    format!("/panel/draft/add-module/{id}").into(),
                                ),
                                value.into(),
                            );
                        }
                    }
                    let built = crate::panel::build(&app, &mut ui.text);
                    let image =
                        mypaper_ui::render(&mut ui, &built.list, width, app.size.1).unwrap();
                    mypaper_ui::golden::write(
                        &image,
                        &out.join(format!(
                            "add-module-{}-{width}-{}.png",
                            if custom { "custom" } else { "full" },
                            if dark { "dark" } else { "light" }
                        )),
                    )
                    .unwrap();
                }
            }
        }
    }
}
