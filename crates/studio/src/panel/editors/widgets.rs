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
            "{} × {} · {} · Display {}",
            instance.size[0],
            instance.size[1],
            position(app, &layout, instance),
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
    for setting in manifest
        .settings
        .iter()
        .filter(|s| crate::panel::options::visible(&instance.widget, s, &resolved))
    {
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
                    && crate::panel::options::visible(&instance.widget, s, &resolved)
            }) {
                manifest_control(app, b, c, &base, &resolved, setting, instance, manifest);
            }
        });
    }
    let size = Rect::new(c.x, c.y + b.px(6.0), c.w, b.px(36.0));
    text(
        b,
        Rect::new(size.left, size.top, b.px(130.0), size.height()),
        &format!("Size   {} \u{00d7} {}", instance.size[0], instance.size[1]),
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    for (i, label, action) in [
        (0, "Narrower", Action::ResizeWidget(-1, 0)),
        (1, "Wider", Action::ResizeWidget(1, 0)),
        (2, "Shorter", Action::ResizeWidget(0, -1)),
        (3, "Taller", Action::ResizeWidget(0, 1)),
    ] {
        let r = Rect::new(
            size.right - b.px(128.0) + b.px(i as f32 * 32.0),
            size.top,
            b.px(28.0),
            b.px(28.0),
        );
        crate::panel::controls::icon_button(
            b,
            r,
            if i % 2 == 0 {
                mypaper_ui::icons::Icon::Remove.into()
            } else {
                glyph::ADD
            },
            label,
            true,
            action,
        );
    }
    c.y = size.bottom + b.px(5.0);
    notice(b, c, "Drag the corner brackets on the desktop to resize.");
    group(b, c, "Its look", |b, c| {
        let own = instance.appearance != Default::default();
        toggle(
            b,
            c,
            "Its own look",
            if own { "Your override" } else { "From Look" },
            own,
            if own {
                Action::Unset(Edit::new(
                    DocId::Widgets,
                    format!("{base}/appearance"),
                    "Follow Look",
                ))
            } else {
                Action::Set(
                    Edit::new(
                        DocId::Widgets,
                        format!("{base}/appearance/surface"),
                        "Its own look",
                    ),
                    json!("glass"),
                )
            },
        );
        if own {
            let style_path = format!("{base}/appearance/style");
            select(
                b,
                c,
                "Style",
                app.get(DocId::Widgets, &style_path)
                    .cloned()
                    .unwrap_or(json!("minimal")),
                Edit::new(DocId::Widgets, style_path, "Widget style"),
                [
                    ("liquid", "Liquid"),
                    ("minimal", "Minimal"),
                    ("depth", "Depth"),
                    ("neon", "Neon"),
                    ("paper", "Paper"),
                    ("aero", "Aero"),
                ]
                .into_iter()
                .map(|(v, l)| (json!(v), l.into()))
                .collect(),
            );
            select(
                b,
                c,
                "Background",
                app.get(DocId::Widgets, &format!("{base}/appearance/surface"))
                    .cloned()
                    .unwrap_or(json!("glass")),
                Edit::new(
                    DocId::Widgets,
                    format!("{base}/appearance/surface"),
                    "Background",
                ),
                [("glass", "Glass"), ("solid", "Solid"), ("none", "None")]
                    .into_iter()
                    .map(|(v, l)| (json!(v), l.into()))
                    .collect(),
            );
            for (name, label, range, fallback) in [
                ("opacity", "Tint strength", (0.0, 1.0, 0.01), 0.46),
                ("blur", "Blur", (0.0, 64.0, 1.0), 12.0),
                ("radius", "Corners", (0.0, 48.0, 1.0), 22.0),
            ] {
                number(
                    app,
                    b,
                    c,
                    DocId::Widgets,
                    &format!("{base}/appearance/{name}"),
                    label,
                    range,
                    fallback,
                );
            }
            crate::panel::colors::swatches(
                app,
                b,
                c,
                &Edit::new(DocId::Widgets, format!("{base}/appearance/tint"), "Tint"),
                app.get(DocId::Widgets, &format!("{base}/appearance/tint")),
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
        } if options.len() >= 3
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
            crate::ui::tiles(
                b,
                c,
                key(("panel-faces", name)),
                options.len(),
                false,
                &tiles,
                |b, i, pic| face(app, b, instance, manifest, name, options[i].0, pic),
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
                    .filter(|f| name != "zones" || f["key"] != "tz")
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
                                    &format!("{value:.0}"),
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
                }
            }
            if max_items.is_none_or(|max| items.len() < max) {
                if name == "zones" {
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
    let standalone = setting.is_empty();
    if !standalone {
        b.fill(pic, b.px(8.0), crate::panel::tokens::hex(0x293246));
        if let Some(path) = app
            .library
            .active()
            .and_then(|i| app.library.items[i].preview.as_ref())
            && let Some(image) = app
                .images
                .borrow_mut()
                .get(path, (pic.width() as u32, pic.height() as u32))
        {
            crate::stage::image_round(b, pic, &image, 0.65, b.px(8.0));
        }
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
    let placed = mypaper_widgets::layout::Placed {
        instance: "panel-face-preview".into(),
        widget: instance.widget.clone(),
        cell: [0, 0],
        size: manifest.size.default,
        settings: manifest.resolve_settings(&settings),
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
        return;
    };
    if instance.widget == "weather" {
        model.data = json!({"current":{"temperature":22,"condition":"Partly cloudy","code":2,"isDay":true,"location":{"name":"Hong Kong"}},"forecast":{"days":[{"date":"2026-11-06","max":24,"min":19,"code":2},{"date":"2026-11-07","max":23,"min":18,"code":3},{"date":"2026-11-08","max":25,"min":20,"code":0}]}});
    }
    if instance.widget == "photo" {
        model.photo.current = app
            .library
            .active()
            .and_then(|i| app.library.items[i].preview.as_ref())
            .and_then(|p| app.images.borrow_mut().get(p, (256, 160)));
    }
    // Render at native scale before reducing. The shell's text fitting works in DIPs;
    // tiny fractional scales can otherwise turn the greeting into “Good…”.
    let (width, height) = if standalone {
        (
            360,
            (360 * instance.size[1] / instance.size[0].max(1)).clamp(80, 360),
        )
    } else if instance.widget == "weather" && value == "compact" {
        (450, 100)
    } else {
        (360, 220)
    };
    if instance.widget == "clock" && value == "words" {
        model.placed.settings.insert("name".into(), json!("Ada"));
    }
    let cache_key = format!(
        "face:{}:{setting}:{value}:{width}:{height}:{}:{}:{}:{:p}",
        instance.widget,
        serde_json::to_string(&appearance).unwrap_or_default(),
        serde_json::to_string(&model.placed.settings).unwrap_or_default(),
        app.library
            .active()
            .map_or("", |i| app.library.items[i].id.as_str()),
        model
            .photo
            .current
            .as_ref()
            .map_or(std::ptr::null(), std::sync::Arc::as_ptr)
    );
    let state = app.panel.as_ref().unwrap();
    let cached = state.specimens.borrow().get(&cache_key).cloned();
    let image = cached.or_else(|| {
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
            b.text,
        );
        list.ops.insert(
            0,
            mypaper_ui::Op::Fill {
                rect: Rect::new(0.0, 0.0, width as f32, height as f32),
                radius: 22.0,
                color: resolved_appearance
                    .tint
                    .map_or(crate::panel::tokens::hex(0x344052), |color| {
                        mypaper_ui::Rgba::rgb8(color.0, 1.0)
                    })
                    .with_alpha(resolved_appearance.opacity.unwrap_or(0.85)),
            },
        );
        let mut ui = mypaper_ui::Ui::new().ok()?;
        let image = std::sync::Arc::new(mypaper_ui::render(&mut ui, &list, width, height).ok()?);
        crate::panel::specimens::remember(app, cache_key, &image);
        Some(image)
    });
    if let Some(image) = image {
        let inner = if standalone {
            pic
        } else {
            pic.inflate(-b.px(6.0))
        };
        let factor = (inner.width() / width as f32).min(inner.height() / height as f32);
        let (w, h) = (width as f32 * factor, height as f32 * factor);
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
    }
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
    let builtins = [
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
    let mut categories = Vec::new();
    for (id, widget) in &app.registry.widgets {
        if builtins.contains(&id.as_str()) {
            let category = widget.manifest.category.clone().unwrap_or("Other".into());
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
    }
    categories.push("Your apps".to_owned());
    let layout = legacy::layout(app);
    let mut count = 0;
    for category in categories {
        let widgets: Vec<_> = app
            .registry
            .widgets
            .iter()
            .filter(|(id, w)| {
                let group = if builtins.contains(&id.as_str()) {
                    w.manifest.category.as_deref().unwrap_or("Other")
                } else {
                    "Your apps"
                };
                group == category
                    && format!(
                        "{} {}",
                        w.manifest.name,
                        w.manifest.description.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        if widgets.is_empty() {
            continue;
        }
        group(b, c, &category, |b, c| {
            for (id, widget) in widgets {
                let manifest = &widget.manifest;
                let already = layout.instances.iter().filter(|i| i.widget == *id).count();
                let detail = if already > 0 {
                    format!(
                        "{} · {already} on your desktop",
                        manifest.description.as_deref().unwrap_or("")
                    )
                } else {
                    manifest.description.clone().unwrap_or_default()
                };
                crate::panel::icons::row(
                    app,
                    b,
                    c,
                    id,
                    legacy::glyph_of(id),
                    &manifest.name,
                    &detail,
                    selected.as_deref() == Some(id),
                    Action::Panel(Command::PickWidget(id.clone())),
                );
                count += 1;
            }
        });
    }
    if count == 0 {
        notice(b, c, "No widgets match this search.");
    }
    notice(
        b,
        c,
        "New widgets land in the first free space. You can move them on the desktop.",
    );
}
