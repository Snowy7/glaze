use mypaper_ui::Rect;
use serde_json::json;

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
        notice(b, c, "Bring its folder back, or remove this widget from your desktop.");
        action_button(b, c, "Remove", Action::RemoveWidget);
        return;
    };
    let manifest = &installed.manifest;
    title(b, c, &manifest.name, &format!("{} × {} · {} · Display {}", instance.size[0], instance.size[1], position(app, &layout, instance), app.nav.display + 1));
    let half = (c.w - b.px(8.0)) / 2.0;
    for (n, label, icon, action) in [(0, "Duplicate", glyph::COPY, Action::DuplicateWidget), (1, "Remove", glyph::DELETE, Action::RemoveWidget)] {
        button(b, Rect::new(c.x + n as f32 * (half + b.px(8.0)), c.y + b.px(6.0), half, b.px(30.0)), label, Some(icon), false, true, action);
    }
    c.y += b.px(44.0);
    let index = layout.instances.iter().position(|i| i.instance == instance.instance).unwrap();
    let base = format!("/instances/{index}");
    let resolved = manifest.resolve_settings(&instance.settings);
    let mut groups = Vec::new();
    for setting in &manifest.settings {
        let group = legacy::group_of(setting).unwrap_or("Options");
        if !groups.contains(&group) { groups.push(group); }
    }
    for heading in groups {
        group(b, c, heading, |b, c| {
            for setting in manifest.settings.iter().filter(|s| legacy::group_of(s).unwrap_or("Options") == heading) {
                legacy::widget_setting(app, b, c, &base, &resolved, setting, instance, manifest);
            }
        });
    }
    group(b, c, "Size", |b, c| {
        // Stacking keeps the labels and both stepper buttons legible at 360 DIPs.
        for (label, value, minus, plus) in [
            ("Wide", instance.size[0], Action::ResizeWidget(-1, 0), Action::ResizeWidget(1, 0)),
            ("Tall", instance.size[1], Action::ResizeWidget(0, -1), Action::ResizeWidget(0, 1)),
        ] {
            let r = Rect::new(c.x, c.y + b.px(4.0), c.w, b.px(32.0));
            legacy::stepper(b, r, label, value, minus, plus);
            b.stroke(r, b.px(8.0), b.px(1.0), b.p.edge);
            c.y += b.px(40.0);
        }
    });
    group(b, c, "Its look", |b, c| {
        let own = instance.appearance != Default::default();
        toggle(b, c, "Its own look", if own { "Your override" } else { "From Look" }, own,
            if own { Action::Unset(Edit::new(DocId::Widgets, format!("{base}/appearance"), "Follow Look")) }
            else { Action::Set(Edit::new(DocId::Widgets, format!("{base}/appearance/surface"), "Its own look"), json!("glass")) });
        if own {
            select(b, c, "Background", app.get(DocId::Widgets, &format!("{base}/appearance/surface")).cloned().unwrap_or(json!("glass")),
                Edit::new(DocId::Widgets, format!("{base}/appearance/surface"), "Background"),
                [("glass", "Glass"), ("solid", "Solid"), ("none", "None")].into_iter().map(|(v, l)| (json!(v), l.into())).collect());
            crate::ui::swatches(app, b, c, &Edit::new(DocId::Widgets, format!("{base}/appearance/tint"), "Tint"),
                app.get(DocId::Widgets, &format!("{base}/appearance/tint")), None,
                &[[139,158,183], [127,174,142], [200,141,60], [230,230,230]]);
            action_button(b, c, "Follow Look", Action::Unset(Edit::new(DocId::Widgets, format!("{base}/appearance"), "Follow Look")));
        } else { row(b, c, Some(glyph::LOOK), "Look", "From Look", "", None, open(Page::Look)); }
    });
}

pub fn add(app: &App, b: &mut B, c: &mut Col) {
    title(b, c, "Add a widget", "Choose one, then Add to your desktop");
    field(app, b, c, FieldId::WidgetSearch, "Search widgets", "Name or description", "");
    let query = app.text(&FieldId::WidgetSearch).trim().to_lowercase();
    let selected = &app.panel.as_ref().unwrap().add_widget;
    let builtins = ["clock", "calendar", "countdown", "crypto", "heatmap", "launcher", "media-pill", "notes", "now-playing", "photo", "quick-toggles", "quote", "rss", "system-monitor", "timer", "todo", "weather"];
    let mut categories = vec!["Your apps".to_owned()];
    for (id, widget) in &app.registry.widgets {
        if builtins.contains(&id.as_str()) {
            let category = widget.manifest.category.clone().unwrap_or("Other".into());
            if !categories.contains(&category) { categories.push(category); }
        }
    }
    let layout = legacy::layout(app);
    let mut count = 0;
    for category in categories {
        let widgets: Vec<_> = app.registry.widgets.iter().filter(|(id, w)| {
            let group = if builtins.contains(&id.as_str()) { w.manifest.category.as_deref().unwrap_or("Other") } else { "Your apps" };
            group == category && format!("{} {}", w.manifest.name, w.manifest.description.as_deref().unwrap_or("")).to_lowercase().contains(&query)
        }).collect();
        if widgets.is_empty() { continue }
        group(b, c, &category, |b, c| {
            for (id, widget) in widgets {
                let manifest = &widget.manifest;
                let already = layout.instances.iter().filter(|i| i.widget == *id).count();
                let detail = if already > 0 { format!("{} · {already} on your desktop", manifest.description.as_deref().unwrap_or("")) } else { manifest.description.clone().unwrap_or_default() };
                row(b, c, Some(legacy::glyph_of(id)), &manifest.name, "", &detail, Some(selected.as_deref() == Some(id)), Action::Panel(Command::PickWidget(id.clone())));
                count += 1;
            }
        });
    }
    if count == 0 { notice(b, c, "No widgets match this search."); }
    notice(b, c, "New widgets land in the first free space. You can move them on the desktop.");
}
