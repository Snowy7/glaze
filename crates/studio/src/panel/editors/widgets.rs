use mypaper_ui::Rect;
use mypaper_ui::controls::{Align, key};
use mypaper_widgets::layout::Instance;
use mypaper_widgets::manifest::{Setting, WidgetManifest};
use serde_json::{Map, Value, json};

use super::*;
use crate::pages::widgets as legacy;

pub fn widget(app: &App, b: &mut B, c: &mut Col) {
    let layout = legacy::layout(app);
    let Some(instance) = app.nav.widget.as_deref().and_then(|id| layout.get(id)) else {
        title(b, c, "Widget", "No longer on this display");
        action_button(b, c, "All widgets", open(Page::Widgets));
        return;
    };
    let Some(installed) = app.registry.get(&instance.widget) else {
        title(b, c, &instance.widget, "Not installed");
        notice(
            b,
            c,
            "Bring its folder back, or remove this widget from your desktop.",
        );
        action_button(b, c, "Remove", Action::RemoveWidget);
        return;
    };
    let manifest = &installed.manifest;
    title(
        b,
        c,
        &manifest.name,
        &format!(
            "{} · Display {}",
            position(app, &layout, instance)
                .split(',')
                .next()
                .unwrap_or(""),
            app.nav.display + 1
        ),
    );
    let index = layout
        .instances
        .iter()
        .position(|i| i.instance == instance.instance)
        .unwrap();
    let base = format!("/instances/{index}");
    let resolved = manifest.resolve_settings(&instance.settings);
    let mut groups = Vec::new();
    for setting in manifest.settings.iter().filter(|s| {
        !matches!(s, Setting::Font { .. })
            && crate::panel::options::visible(&instance.widget, s, &resolved)
    }) {
        let group = legacy::group_of(setting).unwrap_or("Options");
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    for heading in groups {
        group(b, c, heading, |b, c| {
            if instance.widget == "photo" && heading == "Photos" {
                photos(app, b, c, instance, &resolved, &base);
                return;
            }
            for setting in manifest.settings.iter().filter(|s| {
                legacy::group_of(s).unwrap_or("Options") == heading
                    && !matches!(s, Setting::Font { .. })
                    && crate::panel::options::visible(&instance.widget, s, &resolved)
            }) {
                manifest_control(app, b, c, &base, &resolved, setting, instance, manifest);
            }
        });
    }
    group(b, c, "Size", |b, c| {
        size_controls(b, c, instance);
        notice(b, c, "Drag to move · arrows move one cell");
        notice(b, c, "Drag the corner brackets on the desktop to resize.");
    });
    group(b, c, "Its look", |b, c| {
        let own = instance.appearance != Default::default();
        let inherited = crate::panel::widget_look::inherited_style(app);
        let reset = Action::Unset(Edit::new(
            DocId::Widgets,
            format!("{base}/appearance"),
            "Follow Look",
        ));
        toggle(
            b,
            c,
            "Its own look",
            if own { "Only this widget" } else { "From Look" },
            own,
            if own {
                reset.clone()
            } else {
                Action::Set(
                    Edit::new(DocId::Widgets, format!("{base}/appearance"), "Its own look"),
                    crate::panel::widget_look::appearance(app, instance, inherited),
                )
            },
        );
        if own {
            let selected = crate::panel::widget_look::selected(&instance.appearance);
            let choices: Vec<_> = super::look::STYLES.iter().map(|(style, name, _)| {
                (*name, selected == Some(*style), Action::Set(
                    Edit::new(DocId::Widgets, format!("{base}/appearance"), "Widget style"),
                    crate::panel::widget_look::appearance(app, instance, *style),
                ))
            }).collect();
            crate::panel::choosers::draw(
                app, b, c, key(("panel-widget-styles", &instance.instance)), b.px(184.0), &choices,
                |b, index, rect| {
                    let mut preview = instance.clone();
                    if let Some(index) = index.filter(|index| selected != Some(super::look::STYLES[*index].0)) {
                        preview.appearance = serde_json::from_value(
                            crate::panel::widget_look::appearance(app, instance, super::look::STYLES[index].0)
                        ).unwrap_or_default();
                    }
                    let (setting, value) = crate::panel::options::face_choice(manifest, &resolved)
                        .unwrap_or(("variant", ""));
                    face(app, b, &preview, manifest, setting, value, rect);
                },
            );
            let style = selected.unwrap_or(inherited);
            notice(b, c, &format!("This widget's {} settings", selected.map_or("Custom", super::look::style_name)));
            let uses = crate::look::uses(style);
            let effective = instance
                .appearance
                .over(&layout.appearance.over(&mypaper_widgets::look::defaults()));
            let mut sliders = Vec::new();
            if uses[0] {
                sliders.push(crate::panel::material::Slider {
                    label: "Clarity",
                    value: 1.0 - effective.opacity.unwrap_or(0.46) as f64,
                    max: 1.0,
                    edit: Edit::new(
                        DocId::Widgets,
                        format!("/_panel/clarity{base}/appearance/opacity"),
                        "Clarity",
                    ),
                });
            }
            if uses[1] {
                sliders.push(crate::panel::material::Slider {
                    label: "Blur",
                    value: effective.blur.unwrap_or(12.0) as f64,
                    max: 32.0,
                    edit: Edit::new(DocId::Widgets, format!("{base}/appearance/blur"), "Blur"),
                });
            }
            crate::panel::material::block(
                app,
                b,
                c,
                &sliders,
                Edit::new(DocId::Widgets, format!("{base}/appearance/tint"), "Tint"),
                app.get(DocId::Widgets, &format!("{base}/appearance/tint")),
                false,
            );
        }
        select(
            b,
            c,
            "Background",
            app.get(DocId::Widgets, &format!("{base}/appearance/surface"))
                .cloned()
                .unwrap_or(Value::Null),
            Edit::new(
                DocId::Widgets,
                format!("{base}/appearance/surface"),
                "Background",
            ),
            vec![
                (Value::Null, "From Look".into()),
                (json!("none"), "None".into()),
                (json!("glass"), "Glass".into()),
                (json!("solid"), "Solid".into()),
            ],
        );
        for setting in manifest.settings.iter().filter(|s| {
            matches!(s, Setting::Font { .. })
                && crate::panel::options::visible(&instance.widget, s, &resolved)
        }) {
            manifest_control(app, b, c, &base, &resolved, setting, instance, manifest);
        }
    });
    let half = (c.w - b.px(8.0)) / 2.0;
    for (n, label, icon, action) in [
        (0, "Duplicate", glyph::COPY, Action::DuplicateWidget),
        (1, "Remove", glyph::DELETE, Action::RemoveWidget),
    ] {
        button(
            b,
            Rect::new(
                c.x + n as f32 * (half + b.px(8.0)),
                c.y + b.px(6.0),
                half,
                b.px(30.0),
            ),
            label,
            Some(icon),
            false,
            true,
            action,
        );
    }
    c.y += b.px(44.0);
}

/// Wide and Tall measure grid cells, with separate named steppers on the same line.
fn size_controls(b: &mut B, c: &mut Col, instance: &Instance) {
    let gap = b.px(12.0);
    let width = (c.w - gap) / 2.0;
    let h = height(b, 34.0);
    for (axis, label) in ["Wide", "Tall"].into_iter().enumerate() {
        let r = Rect::new(c.x + axis as f32 * (width + gap), c.y, width, h);
        let button_width = b.px(28.0);
        text(
            b,
            Rect::new(
                r.left,
                r.top,
                r.width() - button_width * 2.0 - b.px(4.0),
                r.height(),
            ),
            &format!("{label} {}", instance.size[axis]),
            13.0,
            400,
            b.p.ink1,
            Align::Start,
        );
        for (i, delta) in [-1, 1].into_iter().enumerate() {
            let action = if axis == 0 {
                Action::ResizeWidget(delta, 0)
            } else {
                Action::ResizeWidget(0, delta)
            };
            crate::panel::controls::icon_button(
                b,
                Rect::new(
                    r.right - button_width * (2 - i) as f32,
                    r.top,
                    button_width,
                    h,
                ),
                if delta < 0 {
                    mypaper_ui::icons::Icon::Remove.into()
                } else {
                    glyph::ADD
                },
                &format!(
                    "{label}: {}",
                    if delta < 0 {
                        "one cell less"
                    } else {
                        "one cell more"
                    }
                ),
                instance.size[axis] > 1 || delta > 0,
                action,
            );
        }
    }
    c.y += h + b.px(6.0);
}

#[allow(clippy::too_many_arguments)]
fn manifest_control(
    app: &App,
    b: &mut B,
    c: &mut Col,
    base: &str,
    resolved: &Map<String, Value>,
    setting: &Setting,
    instance: &Instance,
    manifest: &WidgetManifest,
) {
    let placeholder = crate::panel::options::placeholder(&instance.widget, setting);
    let presentation = crate::panel::options::presentation(setting);
    let setting = &presentation;
    match setting {
        Setting::Select {
            key: name,
            label,
            options,
            ..
        } if options.len() >= 2
            && matches!(
                name.as_str(),
                "variant" | "face" | "layout" | "style" | "look" | "shape" | "display" | "frame"
            ) =>
        {
            let current = resolved.get(name).and_then(Value::as_str).unwrap_or("");
            let options: Vec<_> = options
                .iter()
                .map(|o| match o {
                    mypaper_widgets::manifest::SelectOption::Plain(value) => {
                        (value.as_str(), value.as_str())
                    }
                    mypaper_widgets::manifest::SelectOption::Detailed(value) => (
                        value.value.as_str(),
                        value.label.as_deref().unwrap_or(&value.value),
                    ),
                })
                .collect();
            let edit = Edit::new(
                DocId::Widgets,
                format!("{base}/settings/{}", crate::docs::token(name)),
                label.as_str(),
            );
            let tiles: Vec<_> = options
                .iter()
                .map(|(value, title)| {
                    (
                        *title,
                        *value == current,
                        Action::Set(edit.clone(), json!(value)),
                    )
                })
                .collect();
            text(
                b,
                Rect::new(c.x, c.y, c.w, height(b, 24.0)),
                label,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            c.y += height(b, 28.0);
            let picture_height = b.px(184.0);
            crate::panel::choosers::draw(
                app,
                b,
                c,
                key(("panel-faces", &instance.instance, name)),
                picture_height,
                &tiles,
                |b, index, rect| face(app, b, instance, manifest, name, index.map_or(current, |index| options[index].0), rect),
            );
        }
        Setting::Number {
            key: name,
            label,
            step,
            ..
        }
        | Setting::Slider {
            key: name,
            label,
            min: _,
            max: _,
            step,
            ..
        } => {
            let schema = serde_json::to_value(setting).expect("manifest option");
            let value = resolved.get(name).and_then(Value::as_f64).unwrap_or(0.0);
            let range = (
                schema["min"].as_f64().unwrap_or(0.0),
                schema["max"].as_f64().unwrap_or(100.0),
                step.unwrap_or(1.0),
            );
            let unit = number_unit(name, &schema);
            let shown = number_readout(value, &unit);
            crate::panel::material::row(
                b,
                c,
                label,
                value,
                range,
                &shown,
                Edit::new(
                    DocId::Widgets,
                    format!("{base}/settings/{}", crate::docs::token(name)),
                    label.as_str(),
                ),
            );
        }
        Setting::Boolean {
            key: name, label, ..
        } => {
            let on = resolved.get(name).and_then(Value::as_bool).unwrap_or(false);
            toggle(
                b,
                c,
                label,
                "",
                on,
                Action::Toggle(
                    Edit::new(
                        DocId::Widgets,
                        format!("{base}/settings/{}", crate::docs::token(name)),
                        label.as_str(),
                    ),
                    on,
                ),
            );
        }
        Setting::Font {
            key: name, label, ..
        } => {
            crate::panel::pickers::font(
                app,
                b,
                c,
                Edit::new(
                    DocId::Widgets,
                    format!("{base}/settings/{}", crate::docs::token(name)),
                    label.as_str(),
                ),
                resolved.get(name).and_then(Value::as_str).unwrap_or(""),
            );
        }
        Setting::String {
            key: name, label, ..
        } => {
            field(
                app,
                b,
                c,
                FieldId::Text(
                    DocId::Widgets,
                    format!("{base}/settings/{}", crate::docs::token(name)).into(),
                ),
                label,
                &placeholder,
                instance
                    .settings
                    .get(name)
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        }
        Setting::List {
            key: name,
            label,
            fields,
            max_items,
            ..
        } => {
            let path = format!("{base}/settings/{}", crate::docs::token(name));
            let items = resolved
                .get(name)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            text(
                b,
                Rect::new(c.x, c.y, c.w, height(b, 24.0)),
                label,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            c.y += height(b, 28.0);
            let fields: Vec<_> = fields
                .iter()
                .map(|f| serde_json::to_value(f).expect("manifest field"))
                .collect();
            for (i, item) in items.iter().enumerate() {
                let summary = fields
                    .iter()
                    .filter(|f| {
                        (name != "zones" || f["key"] != "tz")
                            && (instance.widget != "launcher" || f["key"] != "target")
                    })
                    .filter_map(|f| f["key"].as_str().and_then(|k| item[k].as_str()))
                    .collect::<Vec<_>>()
                    .join(" · ");
                let disclosure = key(("panel-list-item", path.as_str(), i));
                crate::panel::lists::row(
                    app,
                    b,
                    c,
                    &crate::panel::SortList::Widget(path.clone()),
                    i,
                    &summary,
                    false,
                    None,
                    None,
                );
                if app.is_open(disclosure) {
                    if name == "zones" {
                        crate::panel::pickers::zones(
                            app,
                            b,
                            c,
                            crate::panel::pickers::ZoneTarget::Record(format!("{path}/{i}")),
                            "Choose a city",
                        );
                        continue;
                    }
                    for schema in &fields {
                        let Some(name) = schema["key"].as_str() else {
                            continue;
                        };
                        let label = schema["label"].as_str().unwrap_or(name);
                        if instance.widget == "launcher" && name == "target" {
                            continue;
                        }
                        let field_path = format!("{path}/{i}/{}", crate::docs::token(name));
                        let value = item
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| schema["default"].clone());
                        match schema["type"].as_str() {
                            Some("boolean") => {
                                let on = value.as_bool().unwrap_or(false);
                                toggle(
                                    b,
                                    c,
                                    label,
                                    "",
                                    on,
                                    Action::Toggle(
                                        Edit::new(DocId::Widgets, field_path, label),
                                        on,
                                    ),
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
                                select(
                                    b,
                                    c,
                                    label,
                                    value,
                                    Edit::new(DocId::Widgets, field_path, label),
                                    options,
                                );
                            }
                            Some("number" | "slider") => {
                                text(
                                    b,
                                    Rect::new(c.x, c.y, c.w, height(b, 24.0)),
                                    label,
                                    13.0,
                                    400,
                                    b.p.ink1,
                                    Align::Start,
                                );
                                c.y += height(b, 24.0);
                                let value = value.as_f64().unwrap_or(0.0);
                                b.slider(
                                    key(("panel-list-number", field_path.as_str())),
                                    Rect::new(c.x, c.y, c.w, height(b, 36.0)),
                                    value,
                                    (
                                        schema["min"].as_f64().unwrap_or(0.0),
                                        schema["max"].as_f64().unwrap_or(100.0),
                                        schema["step"].as_f64().unwrap_or(1.0),
                                    ),
                                    &number_readout(value, &number_unit(name, schema)),
                                    b.px(48.0),
                                    label,
                                    Action::Slider(Edit::new(DocId::Widgets, field_path, label)),
                                );
                                c.y += height(b, 40.0);
                            }
                            _ => field(
                                app,
                                b,
                                c,
                                FieldId::Text(DocId::Widgets, field_path.into()),
                                label,
                                "",
                                value.as_str().unwrap_or(""),
                            ),
                        }
                    }
                    if instance.widget == "launcher" && name == "shortcuts" {
                        crate::panel::pickers::launcher(app, b, c, &path, &items, Some(i));
                        let advanced = key(("launcher-target", path.as_str(), i));
                        action_button(b, c, "Advanced target", Action::More(advanced));
                        if app.is_open(advanced) {
                            field(
                                app,
                                b,
                                c,
                                FieldId::Text(DocId::Widgets, format!("{path}/{i}/target").into()),
                                "Target",
                                "Path, app ID or link",
                                item["target"].as_str().unwrap_or(""),
                            );
                        }
                    }
                }
            }
            if max_items.is_none_or(|max| items.len() < max) {
                if instance.widget == "launcher" && name == "shortcuts" {
                    crate::panel::pickers::launcher(app, b, c, &path, &items, None);
                } else if name == "zones" {
                    crate::panel::pickers::zones(
                        app,
                        b,
                        c,
                        crate::panel::pickers::ZoneTarget::AddRecord(path),
                        "Add a city",
                    );
                } else {
                    let noun = match name.as_str() {
                        "shortcuts" => "app",
                        "feeds" => "feed",
                        "coins" => "coin",
                        "items" => "item",
                        "tasks" => "task",
                        _ => "entry",
                    };
                    field(
                        app,
                        b,
                        c,
                        FieldId::Text(DocId::Studio, format!("/list_new{path}").into()),
                        &format!("New {noun}"),
                        "Name",
                        "",
                    );
                    action_button(b, c, &format!("Add {noun}"), Action::ListAdd(path));
                }
            }
        }
        Setting::File {
            key: name, label, ..
        }
        | Setting::Image {
            key: name, label, ..
        } => {
            let path = format!("{base}/settings/{}", crate::docs::token(name));
            let current = resolved.get(name).and_then(Value::as_str).unwrap_or("");
            let name = mypaper_widgets::pick::parse_data_url(current)
                .map(|url| url.name)
                .unwrap_or("None".into());
            row(
                b,
                c,
                Some(glyph::PHOTO),
                label,
                "Choose",
                &name,
                None,
                Action::Panel(Command::PickAsset(path)),
            );
        }
        Setting::Select {
            key: name,
            label,
            options,
            ..
        } if c.w < b.px(360.0) || b.t.text_scale > 1.0 => {
            let edit = Edit::new(
                DocId::Widgets,
                format!("{base}/settings/{}", crate::docs::token(name)),
                label.as_str(),
            );
            let options = options
                .iter()
                .map(|o| match o {
                    mypaper_widgets::manifest::SelectOption::Plain(value) => {
                        (json!(value), value.clone())
                    }
                    mypaper_widgets::manifest::SelectOption::Detailed(value) => (
                        json!(value.value),
                        value.label.clone().unwrap_or(value.value.clone()),
                    ),
                })
                .collect();
            select(
                b,
                c,
                label,
                resolved.get(name).cloned().unwrap_or(Value::Null),
                edit,
                options,
            );
        }
        Setting::Place { .. } => {
            let mode = Rect::new(c.x, c.y, c.w, b.px(32.0));
            legacy::widget_setting(app, b, c, base, resolved, setting, instance, manifest);
            b.stroke(mode, b.px(7.0), b.px(1.0), b.p.edge);
        }
        _ => legacy::widget_setting(app, b, c, base, resolved, setting, instance, manifest),
    }
}

fn number_unit(name: &str, schema: &Value) -> String {
    if let Some(unit) = schema
        .get("unit")
        .and_then(Value::as_str)
        .filter(|unit| !unit.is_empty())
    {
        return unit.into();
    }
    if name.ends_with("Minutes") {
        "min"
    } else if name.ends_with("Seconds") {
        "s"
    } else if name.to_lowercase().contains("scale") || name == "sensitivity" {
        "\u{00d7}"
    } else if name == "maxItems" {
        "headlines"
    } else if name == "sessionsBeforeLong" {
        "sessions"
    } else {
        ""
    }
    .into()
}

fn number_readout(value: f64, unit: &str) -> String {
    let number = if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}

/// Face tiles use the actual shell view with an isolated sample model; no services or writes.
pub(super) fn face(
    app: &App,
    b: &mut B,
    instance: &Instance,
    manifest: &WidgetManifest,
    setting: &str,
    value: &str,
    pic: Rect,
) {
    use mypaper_shell::preview::widgets::{Frame, Model, Stamp, tokens, view};
    if b.clip_rect().is_some_and(|clip| {
        pic.bottom < clip.top
            || pic.top > clip.bottom
            || pic.right < clip.left
            || pic.left > clip.right
    }) {
        return;
    }
    let standalone = setting.is_empty();
    if !standalone {
        b.fill(pic, b.px(8.0), b.p.well);
    }
    let mut settings = instance.settings.clone();
    if !setting.is_empty() {
        settings.insert(setting.into(), json!(value));
    }
    let user = serde_json::from_value(
        app.docs
            .doc(DocId::Widgets)
            .raw
            .get("appearance")
            .cloned()
            .unwrap_or(json!({})),
    )
    .unwrap_or_default();
    let mut resolved_appearance = mypaper_widgets::look::resolve(
        &instance.appearance,
        &user,
        &mypaper_widgets::look::defaults(),
        mypaper_widgets::look::DEFAULT_TINT,
    );
    if !crate::panel::specimens::desktop_dark(app)
        && instance.appearance.text.is_none()
        && user.text.is_none()
    {
        resolved_appearance.text = Some(mypaper_theme::TextTone::Dark);
        if instance.appearance.tint.is_none() && user.tint.is_none() {
            resolved_appearance.tint = Some(mypaper_config::Color([242, 242, 247]));
        }
    }
    let appearance = mypaper_widgets::look::json_of(&resolved_appearance);
    let mut resolved_settings = manifest.resolve_settings(&settings);
    if instance.widget == "clock" && value == "words" {
        resolved_settings.insert("name".into(), json!("Ada"));
    }
    let photo = (instance.widget == "photo")
        .then(|| app.library.active()
            .and_then(|i| app.library.items[i].preview.as_ref())
            .and_then(|p| app.images.borrow_mut().get(p, (256, 160))))
        .flatten();
    // Render at native scale before reducing. The shell's text fitting works in DIPs;
    // tiny fractional scales can otherwise turn the greeting into “Good…”.
    let variant = resolved_settings
        .get("variant")
        .and_then(Value::as_str)
        .unwrap_or("");
    let (width, height) = if standalone {
        (
            360,
            (360 * instance.size[1] / instance.size[0].max(1)).clamp(80, 360),
        )
    } else {
        match (instance.widget.as_str(), variant) {
            ("weather", "compact") => (360, 108),
            ("clock", "analog") => (240, 220),
            ("clock", "world") => (320, 220),
            ("media-pill", _) => (360, 90),
            ("countdown", _) => (360, 120),
            ("now-playing", "strip") => (360, 112),
            ("notes" | "calendar" | "photo", _) => (280, 240),
            _ => (360, 220),
        }
    };
    let cache_key = format!(
        "face:{}:{setting}:{value}:{width}:{height}:{:.0}x{:.0}:{}:{}:{}:{:p}",
        instance.widget,
        pic.width(), pic.height(),
        serde_json::to_string(&appearance).unwrap_or_default(),
        serde_json::to_string(&resolved_settings).unwrap_or_default(),
        app.library
            .active()
            .map_or("", |i| app.library.items[i].id.as_str()),
        photo.as_ref()
            .map_or(std::ptr::null(), std::sync::Arc::as_ptr)
    );
    let state = app.panel.as_ref().unwrap();
    let cached = state.specimens.borrow().get(&cache_key).cloned();
    // Construct the isolated runtime only on a bitmap miss; cached gallery rows own pixels.
    let image = cached.or_else(|| {
    let placed = mypaper_widgets::layout::Placed {
        instance: "panel-face-preview".into(),
        widget: instance.widget.clone(),
        cell: [0, 0],
        size: manifest.size.default,
        settings: resolved_settings,
        appearance: appearance.clone(),
        own: Map::new(),
    };
    let now = Stamp {
        date: mypaper_ui::calendar::Date {
            year: 2026,
            month: 11,
            day: 6,
        },
        hour: 10,
        minute: 42,
        second: 18,
        ms: 1793958138000,
    };
    let mut storage = mypaper_widgets::storage::Storage::new(
        std::env::temp_dir().join("mypaper-panel-face-preview"),
    );
    let Ok(mut model) = Model::new(placed, &mut storage, now) else {
        return None;
    };
    if instance.widget.contains("--") && model.data.get("face").is_none() {
        return None;
    }
    sample_content(&mut model, now);
    if instance.widget == "weather" {
        model.data = json!({"current":{"temperature":22,"condition":"Partly cloudy","code":2,"isDay":true,"location":{"name":"Hong Kong"}},"forecast":{"days":[{"date":"2026-11-06","max":24,"min":19,"code":2},{"date":"2026-11-07","max":23,"min":18,"code":3},{"date":"2026-11-08","max":25,"min":20,"code":0}]}});
    }
    model.photo.current = photo;
        let mut ui = crate::panel::measure::raster_ui(app)?;
        ui.text.clear_cache();
        let mut list = view(
            &model,
            &tokens(&appearance),
            Frame {
                size: (width, height),
                scale: 1.0,
                now,
                instant: std::time::Instant::now(),
                editing: false,
            },
            &mut ui.text,
        );
        if !standalone
            && resolved_appearance.surface != Some(mypaper_theme::Surface::None)
            && let Some(image) = crate::panel::specimens::frosted_wallpaper(
                app,
                resolved_appearance.blur.unwrap_or(12.0),
            )
        {
            list.ops.insert(
                0,
                mypaper_ui::Op::RoundedImage {
                    rect: Rect::new(0.0, 0.0, width as f32, height as f32),
                    image,
                    opacity: 1.0,
                    source: None,
                    radius: resolved_appearance.radius.unwrap_or(22.0),
                },
            );
        }
        let plate_index = usize::from(
            !standalone && matches!(list.ops.first(), Some(mypaper_ui::Op::RoundedImage { .. })),
        );
        if resolved_appearance.surface != Some(mypaper_theme::Surface::None) {
            list.ops.insert(
                plate_index,
                mypaper_ui::Op::Fill {
                    rect: Rect::new(0.0, 0.0, width as f32, height as f32),
                    radius: resolved_appearance.radius.unwrap_or(22.0),
                    color: resolved_appearance
                        .tint
                        .map_or(crate::panel::tokens::hex(0x344052), |color| {
                            mypaper_ui::Rgba::rgb8(color.0, 1.0)
                        })
                        .with_alpha(resolved_appearance.opacity.unwrap_or(0.85)),
                },
            );
        }
        let thumb_width = pic.width().ceil().max(1.0) as u32;
        let thumb_height = (thumb_width * height / width).max(1);
        crate::panel::measure::scale_picture(&mut list.ops, thumb_width as f32 / width as f32);
        let image = std::sync::Arc::new(mypaper_ui::render(&mut ui, &list, thumb_width, thumb_height).ok()?);
        ui.text.clear_cache();
        crate::panel::specimens::remember(app, cache_key, &image);
        Some(image)
    });
    if let Some(image) = image {
        let inner = if standalone {
            pic
        } else {
            pic.inflate(-b.px(3.0))
        };
        let factor = (inner.width() / width as f32).min(inner.height() / height as f32);
        let (w, h) = (width as f32 * factor, height as f32 * factor);
        if !standalone {
            crate::panel::specimens::backdrop(
                app,
                b,
                Rect::new(
                    inner.left + (inner.width() - w) / 2.0 - b.px(3.0),
                    inner.top + (inner.height() - h) / 2.0 - b.px(3.0),
                    w + b.px(6.0),
                    h + b.px(6.0),
                ),
            );
        }
        crate::stage::image_round(
            b,
            Rect::new(
                inner.left + (inner.width() - w) / 2.0,
                inner.top + (inner.height() - h) / 2.0,
                w,
                h,
            ),
            &image,
            1.0,
            b.px(8.0),
        );
    } else {
        unavailable_face(app, b, manifest, pic);
    }
}

/// Plugin views arrive from the host at runtime. Keep an unavailable picture useful without
/// starting a plugin or inventing its live data while browsing the catalog.
fn unavailable_face(app: &App, b: &mut B, manifest: &WidgetManifest, pic: Rect) {
    b.fill(pic, b.px(8.0), b.p.well);
    let size = (pic.height() * 0.38).min(b.px(42.0));
    let icon = Rect::new(
        pic.left + (pic.width() - size) / 2.0,
        pic.top + pic.height() * 0.13,
        size,
        size,
    );
    if let Some(image) = crate::panel::icons::plugin_image(app, &manifest.id) {
        crate::stage::image_round(b, icon, &image, 1.0, b.px(5.0));
    } else {
        b.icon(icon, legacy::glyph_of(&manifest.id), size / b.s, b.p.ink2);
    }
    text(
        b,
        Rect::new(
            pic.left + b.px(6.0),
            icon.bottom + b.px(5.0),
            pic.width() - b.px(12.0),
            b.px(24.0),
        ),
        "Preview unavailable",
        11.0,
        400,
        b.p.ink3,
        Align::Center,
    );
}

/// Representative inputs for pictures only. Never queries or starts a service or writes data.
fn sample_content(
    model: &mut mypaper_shell::preview::widgets::Model,
    now: mypaper_shell::preview::widgets::Stamp,
) {
    match model.placed.widget.as_str() {
        "now-playing" | "media-pill" => {
            model.data = json!({"title":"Night Drive", "artist":"Kairo", "app":"Music", "playing":true, "position":83, "duration":246, "_at":now.ms});
            model.artwork = Some(sample_artwork());
            model.artwork_backdrop = model.artwork.clone();
            model.artwork_tint = Some(crate::panel::tokens::hex(0x7da8db));
            model.spectrum = (0..48)
                .map(|i| 0.18 + ((i as f32 * 0.67).sin().abs() * 0.65))
                .collect();
        }
        "notes" => {
            model.note =
                "# A little space\n\nMake something you love.\nTake a walk at sunset.".into()
        }
        "todo" => {
            model.tasks = serde_json::from_value(json!([{"text":"Plan the weekend", "done":false}, {"text":"Read a chapter", "done":false}, {"text":"Go for a walk", "done":true}])).unwrap_or_default();
        }
        "crypto" => {
            model.data = json!({"coins":[{"id":"bitcoin","name":"Bitcoin","symbol":"BTC","price":68420.0,"change":2.4},{"id":"ethereum","name":"Ethereum","symbol":"ETH","price":2480.0,"change":1.2}],"at":now.ms})
        }
        "rss" => {
            model.data = json!({"items":[{"title":"A new view of the night sky","source":"Science","published":now.ms-1800000},{"title":"Small ideas, beautifully made","source":"Design","published":now.ms-3600000}]})
        }
        "system-monitor" => {
            use mypaper_widgets::stats::{Net, SystemStats, Usage};
            model.stats = Some(SystemStats {
                cpu: 23.0,
                memory: Usage {
                    used: 12 << 30,
                    total: 32 << 30,
                },
                gpu: Some(18.0),
                net: Some(Net {
                    down: 2457600.0,
                    up: 393216.0,
                }),
                disks: None,
                battery: None,
                uptime: 86400.0,
                network: None,
            });
            for (id, level) in [("cpu", 23.0), ("ram", 38.0), ("gpu", 18.0), ("net", 42.0)] {
                model.history.insert(
                    id.into(),
                    (0..24)
                        .map(|i| level + (i as f64 * 0.45).sin() * 9.0)
                        .collect(),
                );
            }
        }
        "heatmap" => {
            for day in 1..=28 {
                model
                    .counts
                    .insert(format!("2026-10-{day:02}"), (day * 7) % 5);
            }
        }
        _ => {}
    }
}

fn sample_artwork() -> std::sync::Arc<mypaper_ui::Image> {
    use std::sync::{Arc, OnceLock};
    static ARTWORK: OnceLock<Arc<mypaper_ui::Image>> = OnceLock::new();
    ARTWORK
        .get_or_init(|| {
            let mut pixels = Vec::with_capacity(128 * 128 * 4);
            for y in 0..128 {
                for x in 0..128 {
                    let glow = ((x as f32 - 82.0).hypot(y as f32 - 36.0) / 100.0).clamp(0.0, 1.0);
                    let stripe = ((x + y / 2) / 14) % 3 == 0;
                    let [r, g, b] = if stripe {
                        [244, 141, 111]
                    } else {
                        [
                            (35.0 + 35.0 * (1.0 - glow)) as u8,
                            (61.0 + 90.0 * (1.0 - glow)) as u8,
                            (112.0 + 90.0 * (1.0 - glow)) as u8,
                        ]
                    };
                    pixels.extend_from_slice(&[b, g, r, 255]);
                }
            }
            Arc::new(mypaper_ui::Image {
                width: 128,
                height: 128,
                pixels,
            })
        })
        .clone()
}

fn photos(
    app: &App,
    b: &mut B,
    c: &mut Col,
    instance: &Instance,
    resolved: &Map<String, Value>,
    base: &str,
) {
    let photos: Vec<_> = (1..=6)
        .filter_map(|n| {
            resolved
                .get(&format!("image{n}"))
                .and_then(Value::as_str)
                .and_then(mypaper_widgets::pick::parse_data_url)
        })
        .collect();
    for (index, url) in photos.iter().enumerate() {
        let pic = crate::panel::lists::row(
            app,
            b,
            c,
            &crate::panel::SortList::Photos,
            index,
            &url.name,
            false,
            Some(glyph::PHOTO),
            None,
        );
        let path = if app.fixture {
            app.library
                .items
                .iter()
                .filter_map(|w| w.preview.clone())
                .nth(index)
        } else {
            mypaper_widgets::files::user("widget-data")
                .ok()
                .map(|root| root.join(&instance.instance).join("files").join(&url.name))
        };
        if let Some(path) = path
            && let Some(image) = app.images.borrow_mut().get(&path, (72, 56))
        {
            crate::stage::image_round(b, pic, &image, 1.0, b.px(4.0));
        }
    }
    if let Some(n) = (1..=6).find(|n| {
        resolved
            .get(&format!("image{n}"))
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    }) {
        action_button(
            b,
            c,
            "Add photo",
            Action::Panel(Command::PickAsset(format!("{base}/settings/image{n}"))),
        );
    }
}

pub fn pick_asset(app: &mut App, path: &str) {
    if app.fixture {
        return;
    }
    let layout = legacy::layout(app);
    let Some(instance) = app.nav.widget.as_deref().and_then(|id| layout.get(id)) else {
        return;
    };
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .replace("~1", "/")
        .replace("~0", "~");
    let image = app
        .registry
        .get(&instance.widget)
        .and_then(|w| w.manifest.setting(&name))
        .is_some_and(|s| matches!(s, Setting::Image { .. }));
    let Some(source) = crate::dialogs::pick(
        app.window,
        if image {
            "Choose a picture"
        } else {
            "Choose a file"
        },
        false,
        if image { crate::dialogs::PICTURES } else { &[] },
    ) else {
        return;
    };
    let result = mypaper_widgets::files::user("widget-data")
        .map_err(|e| e.to_string())
        .and_then(|root| {
            mypaper_widgets::pick::import(&root, &instance.widget, &instance.instance, &source)
                .map_err(|e| e.message)
        });
    match result {
        Ok(picked) => app.set(
            &Edit::new(DocId::Widgets, path, "Picture"),
            json!(picked.url),
        ),
        Err(error) => app.alert(error),
    }
}

pub fn can_add_widget(app: &App, id: &str) -> bool {
    let Some(manifest) = app.registry.get(id).map(|w| &w.manifest) else {
        return false;
    };
    let layout = legacy::layout(app);
    let total = layout.instances.iter().filter(|i| i.widget == id).count();
    let here = legacy::here(app, &layout)
        .iter()
        .filter(|i| i.widget == id)
        .count();
    manifest.instances.allows(total, here)
}

const BUILTINS: [&str; 17] = [
    "clock",
    "calendar",
    "countdown",
    "crypto",
    "heatmap",
    "launcher",
    "media-pill",
    "notes",
    "now-playing",
    "photo",
    "quick-toggles",
    "quote",
    "rss",
    "system-monitor",
    "timer",
    "todo",
    "weather",
];

fn developer_mode(app: &App) -> bool {
    app.get(DocId::Studio, "/developer_mode")
        .and_then(Value::as_bool)
        == Some(true)
        || std::env::var("MYPAPER_DEVELOPER_MODE").is_ok_and(|value| value == "1")
}

fn development_plugin(id: &str, name: &str) -> bool {
    matches!(id, "hello" | "hello-ts" | "native-echo")
        || format!("{id} {name}")
            .to_lowercase()
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| matches!(word, "demo" | "test" | "example" | "experimental"))
}

#[cfg(test)]
mod gallery_tests {
    use super::*;

    #[test]
    fn development_catalog_entries_are_hidden_until_developer_mode_is_enabled() {
        for (id, name) in [
            ("hello", "Hello"),
            ("hello-ts", "Hello TypeScript"),
            ("native-echo", "Echo"),
            ("weather-demo", "Weather"),
            ("clock", "Clock (experimental)"),
            ("test-face", "Example"),
        ] {
            assert!(development_plugin(id, name));
        }
        for (id, name) in [
            ("t3", "T3 Code"),
            ("tailscale", "Tailscale"),
            ("obs", "OBS Studio"),
        ] {
            assert!(!development_plugin(id, name));
        }
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.run(Action::Set(
            Edit::new(DocId::Studio, "/developer_mode", "Developer mode"),
            json!(true),
        ));
        assert!(developer_mode(&app));
    }
}

pub fn add(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Add a widget", "Choose one, then Add to your desktop");
    field(
        app,
        b,
        c,
        FieldId::WidgetSearch,
        "Search widgets",
        "Name or description",
        "",
    );
    let query = app.text(&FieldId::WidgetSearch).trim().to_lowercase();
    let selected = &app.panel.as_ref().unwrap().add_widget;
    let layout = legacy::layout(app);
    let mut sections: std::collections::BTreeMap<(u8, String), Vec<_>> = Default::default();
    for (id, installed) in &app.registry.widgets {
        let manifest = &installed.manifest;
        let section = if BUILTINS.contains(&id.as_str()) {
            (0, "Built in".to_owned())
        } else if let Some((plugin, _)) = id.split_once("--") {
            let name = app
                .plugins
                .installed
                .iter()
                .find(|p| p.id == plugin)
                .map(|p| p.name.as_str())
                .unwrap_or_else(|| manifest.name.split(" · ").next().unwrap_or(plugin));
            if !developer_mode(app) && development_plugin(plugin, name) {
                continue;
            }
            (2, name.to_owned())
        } else {
            if !developer_mode(app) && development_plugin(id, &manifest.name) {
                continue;
            }
            (1, "Your widgets".to_owned())
        };
        if !format!(
            "{} {} {}",
            manifest.name,
            manifest.description.as_deref().unwrap_or(""),
            section.1
        )
        .to_lowercase()
        .contains(&query)
        {
            continue;
        }
        sections.entry(section).or_default().push((id, manifest));
    }
    if sections.is_empty() {
        notice(b, c, "No widgets match this search.");
    }
    for ((_, heading), widgets) in sections {
        group(b, c, &heading, |b, c| {
            let gap = b.px(10.0);
            let width = (c.w - gap) / 2.0;
            let picture_height = width * 0.67;
            let caption_height = height(b, 66.0);
            let row_height = picture_height + caption_height + b.px(12.0);
            b.group(key(("widget-gallery", heading.as_str())), |b| {
                for (i, (id, manifest)) in widgets.iter().enumerate() {
                    let rect = Rect::new(
                        c.x + (i % 2) as f32 * (width + gap),
                        c.y + (i / 2) as f32 * row_height,
                        width,
                        picture_height + caption_height,
                    );
                    let k = key(("widget-gallery-tile", id.as_str()));
                    let on = selected.as_deref() == Some(id.as_str());
                    b.plate(k, rect.inflate(b.px(3.0)), b.px(10.0), on);
                    let instance = Instance {
                        instance: format!("catalog-{id}"),
                        widget: (*id).clone(),
                        monitor: String::new(),
                        cell: [0, 0],
                        size: manifest.size.default,
                        settings: Default::default(),
                        appearance: Default::default(),
                    };
                    // Use the editor's exact face renderer, with its default option values.
                    let resolved = manifest.resolve_settings(&Map::new());
                    let face_key = [
                        "variant", "face", "style", "layout", "look", "shape", "display", "frame",
                    ]
                    .into_iter()
                    .find(|key| resolved.get(*key).is_some_and(Value::is_string))
                    .unwrap_or("variant");
                    let value = resolved.get(face_key).and_then(Value::as_str).unwrap_or("");
                    face(
                        app,
                        b,
                        &instance,
                        manifest,
                        face_key,
                        value,
                        Rect::new(rect.left, rect.top, width, picture_height),
                    );
                    let name = manifest
                        .name
                        .split_once(" · ")
                        .map_or(manifest.name.as_str(), |(_, face)| face);
                    text(
                        b,
                        Rect::new(
                            rect.left + b.px(4.0),
                            rect.top + picture_height + b.px(5.0),
                            width - b.px(8.0),
                            height(b, 22.0),
                        ),
                        name,
                        13.0,
                        600,
                        b.p.ink1,
                        Align::Start,
                    );
                    let already = layout
                        .instances
                        .iter()
                        .filter(|instance| instance.widget == **id)
                        .count();
                    let detail = if already > 0 {
                        format!("{already} on your desktop")
                    } else {
                        manifest.description.clone().unwrap_or_default()
                    };
                    let mut font = b.font(mypaper_ui::controls::Font::Caption);
                    font.size = b.px(11.0) * b.t.text_scale;
                    let lines = b.text.lines(&detail, &font, width - b.px(8.0), 2);
                    for (line, value) in lines.iter().enumerate() {
                        b.text_in(
                            Rect::new(
                                rect.left + b.px(4.0),
                                rect.top
                                    + picture_height
                                    + height(b, 28.0)
                                    + line as f32 * height(b, 15.0),
                                width - b.px(8.0),
                                height(b, 15.0),
                            ),
                            value,
                            &font,
                            b.p.ink3,
                            Align::Start,
                        );
                    }
                    b.hit_checked(
                        k,
                        rect,
                        mypaper_ui::Role::RadioButton,
                        &format!("{name}: {detail}"),
                        Some(on),
                        Action::Panel(Command::PickWidget((*id).clone())),
                        mypaper_ui::controls::Behavior::Click,
                    );
                    b.focus_ring(k, rect, b.px(10.0));
                }
            });
            c.y += widgets.len().div_ceil(2) as f32 * row_height;
        });
    }
    notice(
        b,
        c,
        "New widgets land in the first free space. You can move them on the desktop.",
    );
}
