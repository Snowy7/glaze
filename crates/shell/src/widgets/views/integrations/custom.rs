use super::*;
use crate::integrations::custom::{EDIT, REFRESH};
use mypaper_ui::{Op, layout::SizeClass};
use mypaper_widgets::custom::Template;

fn value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "—".into(),
        other => other.to_string(),
    }
}
pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect) {
    let state = &m.integration.custom;
    let class = SizeClass::of(rect.width() / f.s, rect.height() / f.s);
    if rect.height() / f.s < 110.0 || rect.width() / f.s < 110.0 {
        tiny(f,m,rect);
        return;
    }
    let inner = rect.inset(Insets::all(f.px(if class == SizeClass::Compact {
        10.0
    } else {
        16.0
    })));
    let (header, content) = inner.take_top(f.px(23.0), f.px(7.0));
    let (refresh, header) = header.take_right(f.px(24.0), f.px(2.0));
    f.icon_button(
        REFRESH,
        refresh,
        Icon::Refresh,
        22.0,
        "Refresh custom widget",
        false,
    );
    let title = state
        .definition
        .as_ref()
        .map_or("Custom widget", |d| d.title.as_str());
    f.text_in(
        header,
        title,
        TextRole::Subheadline,
        f.theme.color.secondary,
        Align::Start,
    );
    let (foot, body) = content.take_bottom(f.px(20.0), f.px(5.0));
    let (edit, caption) = foot.take_right(f.px(23.0), f.px(2.0));
    f.icon_button(
        EDIT,
        edit,
        Icon::Edit,
        20.0,
        "Edit custom widget definition",
        false,
    );
    f.text_in(
        caption,
        state.error.as_deref().unwrap_or_else(|| {
            if state.busy() {
                "Updating…"
            } else {
                state.definition.as_ref().map_or("", |d| d.caption.as_str())
            }
        }),
        TextRole::Caption,
        if state.error.is_some() {
            f.theme.color.warning
        } else {
            f.theme.color.tertiary
        },
        Align::Start,
    );
    let Some(definition) = &state.definition else {
        f.text_in(
            body,
            if state.busy() {
                "Loading…"
            } else {
                "Check the widget file"
            },
            TextRole::Callout,
            f.theme.color.secondary,
            Align::Start,
        );
        return;
    };
    match definition.layout {
        Template::Stat => {
            let text = format!("{}{}", value(&state.value), definition.suffix);
            f.text_in(
                body,
                &text,
                if class == SizeClass::Compact {
                    TextRole::Title2
                } else {
                    TextRole::LargeTitle
                },
                f.theme.color.label,
                Align::Start,
            );
        }
        Template::Text => {
            let line = f.line(TextRole::Body);
            let count = (body.height() / line).floor().max(1.0) as usize;
            f.paragraph(
                body.left,
                body.top,
                body.width(),
                &value(&state.value),
                TextRole::Body,
                f.theme.color.label,
                count.min(40),
            );
        }
        Template::List => {
            let lines = if let Some(items) = state.value.as_array() {
                items
                    .iter()
                    .map(|item| {
                        item["title"]
                            .as_str()
                            .or(item["name"].as_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| value(item))
                    })
                    .collect::<Vec<_>>()
            } else {
                value(&state.value).lines().map(str::to_owned).collect()
            };
            let row = f.px(if class == SizeClass::Compact {
                23.0
            } else {
                30.0
            });
            for (index, text) in lines
                .iter()
                .take((body.height() / row).floor().max(0.0) as usize)
                .enumerate()
            {
                let at = Rect::new(body.left, body.top + index as f32 * row, body.width(), row);
                let (dot, label) = at.take_left(f.px(8.0), f.px(5.0));
                f.fill(
                    dot.place(f.px(4.0), f.px(4.0), Align::Center, Align::Center),
                    f.px(2.0),
                    f.theme.color.accent,
                );
                f.text_in(
                    label,
                    text,
                    TextRole::Callout,
                    f.theme.color.label,
                    Align::Start,
                );
            }
        }
        Template::Gauge => {
            let number = state
                .value
                .as_f64()
                .or_else(|| state.value.as_str()?.parse().ok())
                .or(state.value["value"].as_f64())
                .unwrap_or(0.0);
            let max = state.value["max"].as_f64().unwrap_or(100.0).max(1.0);
            let side = body.width().min(body.height());
            let radius = (side * 0.5 - f.px(6.0)).max(f.px(10.0));
            f.ring(
                body.center(),
                radius,
                f.px(6.0),
                ((number / max) as f32).clamp(0.0, 1.0),
                Some(f.theme.color.accent),
            );
            f.text_in(
                body,
                &format!("{number:.0}{}", definition.suffix),
                if class == SizeClass::Compact {
                    TextRole::Headline
                } else {
                    TextRole::Title1
                },
                f.theme.color.label,
                Align::Center,
            );
        }
        Template::Image => {
            if let Some(image) = &state.image {
                let scale =
                    (body.width() / image.width as f32).min(body.height() / image.height as f32);
                let at = body.place(
                    image.width as f32 * scale,
                    image.height as f32 * scale,
                    Align::Center,
                    Align::Center,
                );
                f.push(Op::Image {
                    rect: at,
                    image: image.clone(),
                    opacity: 1.0,
                    source: None,
                });
            } else {
                f.text_in(
                    body,
                    "No image",
                    TextRole::Callout,
                    f.theme.color.tertiary,
                    Align::Center,
                );
            }
        }
    }
}

fn tiny(f:&mut Frame<'_>,m:&Model,rect:Rect){
    let state=&m.integration.custom;
    let inner=rect.inset(Insets::all(f.px(6.0)));
    let (header,body)=inner.take_top(f.px(16.0),f.px(3.0));
    let (refresh,title)=header.take_right(f.px(16.0),f.px(2.0));
    f.icon_button(REFRESH,refresh,Icon::Refresh,14.0,"Refresh custom widget",false);
    f.text_in(title,state.definition.as_ref().map_or("Custom",|d|d.title.as_str()),TextRole::Caption,f.theme.color.secondary,Align::Start);
    let Some(definition)=&state.definition else{f.text_in(body,"…",TextRole::Body,f.theme.color.tertiary,Align::Start);return;};
    match definition.layout{
        Template::Image=>if let Some(image)=&state.image{
            let scale=(body.width()/image.width as f32).min(body.height()/image.height as f32);
            f.push(Op::Image{rect:body.place(image.width as f32*scale,image.height as f32*scale,Align::Center,Align::Center),image:image.clone(),opacity:1.0,source:None});
        },
        Template::Gauge=>{
            let number=state.value.as_f64().or_else(||state.value.as_str()?.parse().ok()).or(state.value["value"].as_f64()).unwrap_or(0.0);
            let (label,track)=body.take_top(f.px(20.0).min(body.height()),f.px(2.0));
            f.text_in(label,&format!("{number:.0}{}",definition.suffix),TextRole::Headline,f.theme.color.label,Align::Start);
            if track.height()>=f.px(3.0){f.progress(track.place(track.width(),f.px(3.0),Align::Start,Align::Center),(number/100.0)as f32,Some(f.theme.color.accent));}
        }
        _=>{
            let text=if let Some(values)=state.value.as_array(){values.first().map(value).unwrap_or_default()}else{value(&state.value)};
            f.text_in(body,&format!("{}{}",text,definition.suffix),if definition.layout==Template::Stat{TextRole::Headline}else{TextRole::Caption},f.theme.color.label,Align::Start);
        }
    }
}
