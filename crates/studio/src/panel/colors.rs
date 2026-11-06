use super::{Command, controls::text, specimens};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    ui::{B, Col},
};
use mypaper_ui::controls::{Align, key};
use mypaper_ui::{Button, Event, Point, Rect, Rgba};
use mypaper_ui::color_row::popover::{Hsv, Part};
use serde_json::{Value, json};

pub fn swatches(
    app: &App,
    b: &mut B,
    c: &mut Col,
    edit: &Edit,
    current: Option<&Value>,
    auto: bool,
    _none: bool,
    presets: &[[u8; 3]],
) {
    use mypaper_ui::color_row::{Choice, draw};
    let current = current.and_then(Value::as_str).unwrap_or("");
    let is_windows = edit.path.starts_with("/look/windows/");
    let wallpaper = specimens::wallpaper_color(app);
    let wallpaper_hex = wallpaper.map(|color| {
        mypaper_config::Color([
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8,
        ])
        .hex()
    });
    let linked = current == "auto"
        || !auto
            && wallpaper_hex
                .as_deref()
                .is_some_and(|hex| current.eq_ignore_ascii_case(hex));
    let as_is = if is_windows {
        let field = edit.path.rsplit('/').next().unwrap_or("");
        Action::Set(
            Edit::new(
                crate::docs::DocId::Studio,
                format!("/panel/windows/{field}"),
                edit.label.clone(),
            ),
            Value::Null,
        )
    } else {
        Action::Unset(edit.clone())
    };
    let mut choices = vec![
        Choice {
            label: "As is".into(),
            color: None,
            selected: current.is_empty(),
            action: as_is,
        },
        Choice {
            label: "From wallpaper".into(),
            color: wallpaper,
            selected: linked,
            action: if auto {
                Action::Set(edit.clone(), json!("auto"))
            } else {
                wallpaper_hex
                    .as_ref()
                    .map_or(Action::None, |hex| Action::Set(edit.clone(), json!(hex)))
            },
        },
    ];
    choices.extend(presets.iter().map(|rgb| {
        let hex = mypaper_config::Color(*rgb).hex();
        Choice {
            label: format!("{} {hex}", edit.label),
            color: Some(Rgba::rgb8(*rgb, 1.0)),
            selected: !linked && current.eq_ignore_ascii_case(&hex),
            action: Action::Set(edit.clone(), json!(hex)),
        }
    }));
    choices.push(Choice {
        label: format!("Custom {}", edit.label),
        color: mypaper_config::Color::parse(current).map(|color| Rgba::rgb8(color.0, 1.0)),
        selected: !linked
            && current.starts_with('#')
            && !presets.iter().any(|rgb| {
                mypaper_config::Color(*rgb)
                    .hex()
                    .eq_ignore_ascii_case(current)
            }),
        action: Action::Panel(Command::PickColor(edit.clone())),
    });
    draw(
        b,
        key(("panel-colors", edit.doc, edit.path.as_ref())),
        Rect::new(c.x, c.y, c.w, b.px(34.0)),
        &choices,
    );
    c.y += b.px(40.0);
    if app.is_open(picker_key(edit)) {
        let rgb = mypaper_config::Color::parse(current).map_or([139,158,183], |color| color.0);
        let hsv = color_value(app, edit, current, rgb);
        let width = b.px(212.0).min(c.w);
        let rect = Rect::new(c.right()-width, c.y+b.px(3.0), width, width+b.px(98.0));
        let well = mypaper_ui::color_row::popover::draw(b, picker_key(edit), rect, hsv, &format!("Custom {}", edit.label.to_lowercase()), |part, area| Action::Panel(Command::ColorDrag(edit.clone(), part, area)));
        super::controls::icon_button(b, Rect::new(rect.right-b.px(36.0),rect.top+b.px(8.0),b.px(28.0),b.px(28.0)), mypaper_ui::icons::Icon::Close.into(), "Close custom color", true, Action::Panel(Command::PickColor(edit.clone())));
        text(b, Rect::new(well.left,well.top,b.px(30.0),well.height()), "Hex", 12.0, 400, b.p.ink2, Align::Start);
        let field = Rect::new(well.left+b.px(30.0),well.top,well.width()-b.px(30.0),well.height());
        crate::ui::text_field(app, b, field, FieldId::Text(edit.doc, edit.path.clone()), None, "#RRGGBB", if current.starts_with('#') { current } else { "" }, &format!("{} hex", edit.label));
        b.stroke(field,b.px(6.0),b.px(1.0),b.p.control);
        c.y = rect.bottom + b.px(10.0);
    }
}

fn picker_key(edit: &Edit) -> mypaper_ui::controls::Key {
    key(("panel-color-picker", edit.doc, edit.path.as_ref()))
}

fn color_value(app: &App, edit: &Edit, current: &str, rgb: [u8;3]) -> Hsv {
    app.panel.as_ref().unwrap().color_values.borrow().get(&picker_key(edit))
        .filter(|(hex, _)| hex.eq_ignore_ascii_case(current))
        .map_or_else(|| Hsv::from_rgb(rgb), |(_, hsv)| *hsv)
}

fn current_color(app: &App, edit: &Edit) -> Hsv {
    let current = app.get(edit.doc,&edit.path).and_then(Value::as_str).unwrap_or("");
    let rgb = mypaper_config::Color::parse(current).map_or([139,158,183], |color| color.0);
    color_value(app,edit,current,rgb)
}

fn remember_color(app: &App, edit: &Edit, hsv: Hsv) -> Value {
    let hex = mypaper_config::Color(hsv.rgb()).hex();
    let mut cache = app.panel.as_ref().unwrap().color_values.borrow_mut();
    if cache.len() > 32 { cache.clear(); }
    cache.insert(picker_key(edit),(hex.clone(),hsv));
    json!(hex)
}

/// The panel uses the kit picker; the legacy dialog and its owner remain untouched.
pub fn pick(app: &mut App, edit: &Edit) {
    let picker = picker_key(edit);
    let others: Vec<_> = app.frame.targets.iter().filter_map(|target| {
        if let Action::Panel(Command::ColorDrag(other,_,_)) = &target.action { Some(picker_key(other)) } else { None }
    }).collect();
    for other in others { if other != picker { app.nav.more.remove(&other); } }
    if app.field.as_ref().is_some_and(|field| matches!(&field.id, FieldId::Text(doc,path) if *doc == edit.doc && *path == edit.path)) {
        app.blur_field();
    }
    app.run(Action::More(picker));
}

#[derive(Debug, Clone)]
pub struct Drag {
    edit: Edit,
    part: Part,
    rect: Rect,
    hsv: Hsv,
}

fn live(app: &mut App, drag: &Drag, at: Point) {
    let hsv = drag.hsv.at(drag.part,drag.rect,at);
    let value = remember_color(app,&drag.edit,hsv);
    if drag.edit.doc == crate::docs::DocId::Studio {
        app.pending_value = Some((drag.edit.clone(),value));
    } else {
        let mut ops = vec![crate::docs::Op { doc: drag.edit.doc, path: drag.edit.path.to_string(), value: Some(value) }];
        if drag.edit.doc == crate::docs::DocId::Config && drag.edit.path.starts_with("/look") {
            ops.extend(crate::look::projection_after(&app.docs,&ops));
        }
        if let Err(error) = app.docs.live(&ops) {
            super::save_failed(app,Some(drag.edit.doc),&format!("{error:#}"));
        }
    }
    app.input.pointer = Some(at);
    app.dirty = true;
}

/// Reuse the app's slider commit path: each pointer move previews; release writes once.
pub fn event(app: &mut App, event: &Event) -> bool {
    if let Event::Press(pointer,Button::Left) = event {
        let target = app.frame.targets.iter().rev().find(|target| target.visible.is_some_and(|rect| rect.contains(pointer.at))).cloned();
        if let Some(target) = target
            && let Action::Panel(Command::ColorDrag(edit,part,rect)) = target.action
        {
            app.blur_field();
            let drag = Drag { hsv: current_color(app,&edit), edit: edit.clone(), part, rect };
            app.panel.as_mut().unwrap().color_drag = Some(drag.clone());
            app.input.pressed = Some(target.key);
            app.input.focus = Some(target.key);
            app.input.focus_visible = false;
            app.drag = Some(crate::app::Drag::Slider { edit, track:rect, min:0.0,max:1.0,step:0.01 });
            if let Some(window) = app.window {
                // SAFETY: capture only our own panel; the existing slider end releases it.
                unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(window); }
            }
            live(app,&drag,pointer.at);
            return true;
        }
    }
    let Some(drag) = app.panel.as_ref().unwrap().color_drag.clone() else { return false };
    match event {
        Event::Move(pointer) => { live(app,&drag,pointer.at); true }
        Event::Release(pointer,Button::Left) => {
            live(app,&drag,pointer.at);
            app.panel.as_mut().unwrap().color_drag = None;
            false // The app's normal release commits this drag and its History row.
        }
        Event::CaptureLost => { app.panel.as_mut().unwrap().color_drag = None; false }
        Event::Key(0x1B) => {
            app.panel.as_mut().unwrap().color_drag = None;
            app.event(Event::CaptureLost);
            false
        }
        _ => false,
    }
}

pub fn on_key(app: &mut App, vk: u16, ctrl: bool, shift: bool, alt: bool) -> bool {
    if ctrl || alt { return false; }
    if vk == 0x1B {
        let edit = app.frame.targets.iter().find_map(|target| {
            if let Action::Panel(Command::ColorDrag(edit,_,_)) = &target.action { Some(edit.clone()) } else { None }
        });
        if let Some(edit) = edit { pick(app,&edit); return true; }
    }
    if app.field.is_some() { return false; }
    let target = app.input.focus.and_then(|focus| app.frame.targets.iter().find(|target| target.key == focus)).cloned();
    let Some(target) = target else { return false };
    let Action::Panel(Command::ColorDrag(edit,part,_)) = target.action else { return false };
    if !matches!(vk,0x25..=0x28|0x24|0x23) { return false; }
    let mut hsv = current_color(app,&edit);
    let step = if shift { 0.1 } else { 0.01 };
    match part {
        Part::Hue => hsv.hue = match vk { 0x24 => 0.0, 0x23 => 359.0, 0x25|0x28 => (hsv.hue - step*100.0).rem_euclid(360.0), _ => (hsv.hue + step*100.0).rem_euclid(360.0) },
        Part::SaturationValue => match vk {
            0x25 => hsv.saturation = (hsv.saturation-step).max(0.0),
            0x27 => hsv.saturation = (hsv.saturation+step).min(1.0),
            0x26 => hsv.value = (hsv.value+step).min(1.0),
            0x28 => hsv.value = (hsv.value-step).max(0.0),
            0x24 => hsv.value=0.0,
            _ => hsv.value=1.0,
        }
    }
    let value = remember_color(app,&edit,hsv);
    app.set(&edit,value);
    true
}

pub fn accessibility(app: &mut App, target: &mypaper_ui::controls::Target<Action>, action: &mypaper_ui::AccessibilityAction) -> bool {
    use mypaper_ui::AccessibilityAction as A;
    let Action::Panel(Command::ColorDrag(edit,part,_)) = &target.action else { return false };
    let mut hsv = current_color(app,edit);
    match (part,action) {
        (Part::Hue,A::SetNumber(value)) if value.is_finite() => hsv.hue=(*value as f32).clamp(0.0,359.0),
        (Part::Hue,A::Increment) => hsv.hue=(hsv.hue+1.0).rem_euclid(360.0),
        (Part::Hue,A::Decrement) => hsv.hue=(hsv.hue-1.0).rem_euclid(360.0),
        (Part::SaturationValue,A::Increment) => hsv.saturation=(hsv.saturation+0.01).min(1.0),
        (Part::SaturationValue,A::Decrement) => hsv.saturation=(hsv.saturation-0.01).max(0.0),
        _ => return true,
    }
    let value = remember_color(app,edit,hsv);
    app.set(edit,value);
    true
}
