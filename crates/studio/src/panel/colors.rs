use super::{Command, controls::text, specimens};
use crate::panel::tokens::radius;
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    ui::{B, Col},
};
use mypaper_ui::color_row::popover::{Hsv, Part};
use mypaper_ui::controls::{Align, key};
use mypaper_ui::{Button, Event, Point, Rect, Rgba};
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
    use mypaper_ui::color_row::Choice;
    let current = app
        .pending_value
        .as_ref()
        .filter(|(pending, _)| pending.doc == edit.doc && pending.path == edit.path)
        .map(|(_, value)| value)
        .or(current)
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_windows = edit.path.starts_with("/look/windows/");
    let target_wallpaper = edit
        .path
        .strip_prefix("/theme:")
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(folder, _)| {
            let folder = std::path::PathBuf::from(folder.replace("~1", "/").replace("~0", "~"));
            app.library
                .items
                .iter()
                .find(|wallpaper| wallpaper.dir == folder)
        });
    let wallpaper = if let Some(wallpaper) = target_wallpaper {
        specimens::wallpaper_color_of(app, wallpaper)
    } else {
        specimens::wallpaper_color(app)
    };
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
            label: color_name(*rgb).into(),
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
    let row_height = b.px(36.0 + 20.0 * b.t.text_scale);
    let picture = if let Some(wallpaper) = target_wallpaper {
        specimens::wallpaper_image(app, wallpaper)
    } else {
        specimens::wallpaper(app)
    };
    draw_swatches(
        b,
        key(("panel-colors", edit.doc, edit.path.as_ref())),
        Rect::new(c.x, c.y, c.w, row_height),
        &choices,
        picture.as_ref(),
        &edit.label,
    );
    c.y += row_height + b.px(6.0);
    if app.is_open(picker_key(edit)) {
        let first_op = b.list.ops.len();
        let rgb = mypaper_config::Color::parse(current).map_or([139, 158, 183], |color| color.0);
        let hsv = color_value(app, edit, current, rgb);
        app.panel
            .as_ref()
            .unwrap()
            .color_values
            .borrow_mut()
            .insert(picker_key(edit), (current.into(), hsv));
        let viewport = b.clip_rect().unwrap_or(Rect::new(
            0.0,
            b.px(48.0),
            app.size.0 as f32,
            app.size.1 as f32 - b.px(48.0),
        ));
        let width = b
            .px(212.0)
            .min(c.w)
            .min((viewport.height() - b.px(114.0)).max(b.px(100.0)));
        let popover_height = width + b.px(98.0);
        let below = c.y + b.px(3.0);
        let flipped = below + popover_height > viewport.bottom - b.px(6.0);
        let top = if flipped {
            (c.y - row_height - popover_height - b.px(8.0)).clamp(
                viewport.top + b.px(6.0),
                (viewport.bottom - popover_height - b.px(6.0)).max(viewport.top + b.px(6.0)),
            )
        } else {
            below
        };
        let rect = Rect::new(c.right() - width, top, width, popover_height);
        b.hit(
            key((picker_key(edit), "background")),
            rect,
            mypaper_ui::Role::Group,
            "Custom color",
            Action::None,
            mypaper_ui::controls::Behavior::Click,
        );
        let well = mypaper_ui::color_row::popover::draw(
            b,
            picker_key(edit),
            rect,
            hsv,
            &format!("Custom {}", edit.label.to_lowercase()),
            |part, area| Action::Panel(Command::ColorDrag(edit.clone(), part, area)),
        );
        super::controls::icon_button(
            b,
            Rect::new(
                rect.right - b.px(36.0),
                rect.top + b.px(8.0),
                b.px(28.0),
                b.px(28.0),
            ),
            mypaper_ui::icons::Icon::Close.into(),
            "Close custom color",
            true,
            Action::Panel(Command::PickColor(edit.clone())),
        );
        let label_width = b.px(30.0 * b.t.text_scale);
        text(
            b,
            Rect::new(well.left, well.top, label_width, well.height()),
            "Hex",
            12.0,
            400,
            b.p.ink2,
            Align::Start,
        );
        let field = Rect::new(
            well.left + label_width,
            well.top,
            well.width() - label_width,
            well.height(),
        );
        crate::ui::text_field(
            app,
            b,
            field,
            FieldId::Text(edit.doc, edit.path.clone()),
            None,
            "#RRGGBB",
            &mypaper_config::Color(hsv.rgb()).hex(),
            &format!("{} hex", edit.label),
        );
        b.stroke(field, b.px(radius::SMALL), b.px(1.0), b.p.control);
        if !flipped {
            c.y = rect.bottom + b.px(10.0);
        }
        app.panel
            .as_ref()
            .unwrap()
            .color_overlay
            .borrow_mut()
            .extend(b.list.ops.drain(first_op..));
    }
}

fn color_name(rgb: [u8; 3]) -> &'static str {
    let hsv = Hsv::from_rgb(rgb);
    if hsv.saturation < 0.12 {
        return if hsv.value > 0.8 {
            "White"
        } else if hsv.value < 0.2 {
            "Black"
        } else {
            "Gray"
        };
    }
    match hsv.hue as u16 {
        0..=14 | 345..=359 => "Red",
        15..=49 => "Amber",
        50..=69 => "Yellow",
        70..=169 => "Green",
        170..=194 => "Teal",
        195..=254 => "Blue",
        255..=289 => "Purple",
        _ => "Pink",
    }
}

/// Equal circular choices, followed by a readable selected/hovered/focused name.
/// Grouped radio targets preserve AccessKit state and the app's arrow-key navigation.
fn draw_swatches(
    b: &mut B,
    id: mypaper_ui::controls::Key,
    rect: Rect,
    choices: &[mypaper_ui::color_row::Choice<Action>],
    wallpaper: Option<&std::sync::Arc<mypaper_ui::Image>>,
    label: &str,
) {
    let step = ((rect.width() - b.px(8.0)) / choices.len().max(1) as f32).min(b.px(38.0));
    let diameter = (step - b.px(10.0)).clamp(b.px(14.0), b.px(26.0));
    let mut caption = choices
        .iter()
        .find(|choice| choice.selected)
        .map(|choice| choice.label.as_str())
        .unwrap_or("As is");
    b.group(id, |b| {
        for (i, choice) in choices.iter().enumerate() {
            let k = key((id, i));
            let dot = Rect::new(
                rect.left + b.px(4.0) + step * i as f32 + (step - diameter) / 2.0,
                rect.top + (b.px(34.0) - diameter) / 2.0,
                diameter,
                diameter,
            );
            let radius = diameter / 2.0;
            let color = choice.color.unwrap_or(b.p.fill);
            b.fill(dot, radius, color);
            if i == 0 {
                let inset = diameter * 0.28;
                b.line(
                    Point::new(dot.left + inset, dot.bottom - inset),
                    Point::new(dot.right - inset, dot.top + inset),
                    b.px(1.5),
                    b.p.ink2,
                );
            } else if i == 1 {
                if let Some(image) = wallpaper {
                    let start = b.list.ops.len();
                    crate::stage::image_cover(b, dot, image, 1.0);
                    let ops = b.list.ops.split_off(start);
                    b.list.ops.push(mypaper_ui::Op::RoundedClipped {
                        rect: dot,
                        radius,
                        ops: ops.into(),
                    });
                }
            } else if i == choices.len() - 1 {
                let ink = if mypaper_ui::theme::luminance(mypaper_ui::theme::over(color, b.p.panel))
                    > 0.45
                {
                    Rgba::rgb8([25, 25, 25], 1.0)
                } else {
                    Rgba::rgb8([245, 245, 245], 1.0)
                };
                b.icon(dot, mypaper_ui::icons::Icon::Palette, 14.0, ink);
            }
            if choice.selected {
                // 1.5 DIP ring separated from the swatch by a genuine transparent gap.
                let ring = dot.inflate(b.px(3.5));
                b.stroke(ring, ring.height() / 2.0, b.px(1.5), b.p.ink1);
            }
            let hit = Rect::new(
                dot.left - (step - diameter) / 2.0,
                rect.top,
                step,
                b.px(34.0),
            );
            if b.hovered(k) || b.focused(k) {
                caption = &choice.label;
            }
            b.hit_checked(
                k,
                hit,
                mypaper_ui::Role::RadioButton,
                &format!("{label}: {}", choice.label),
                Some(choice.selected),
                choice.action.clone(),
                mypaper_ui::controls::Behavior::Click,
            );
            b.focus_ring(k, dot.inflate(b.px(5.5)), radius + b.px(5.5));
        }
    });
    text(
        b,
        Rect::new(
            rect.left + b.px(4.0),
            rect.top + b.px(36.0),
            rect.width() - b.px(8.0),
            rect.height() - b.px(36.0),
        ),
        caption,
        12.0,
        400,
        b.p.ink2,
        Align::Start,
    );
}

/// Popovers paint after the body's final control boundaries, including when they flip up.
pub fn finish(app: &App, b: &mut B) {
    b.list.ops.extend(
        app.panel
            .as_ref()
            .unwrap()
            .color_overlay
            .borrow_mut()
            .drain(..),
    );
}

fn picker_key(edit: &Edit) -> mypaper_ui::controls::Key {
    key(("panel-color-picker", edit.doc, edit.path.as_ref()))
}

fn color_value(app: &App, edit: &Edit, current: &str, rgb: [u8; 3]) -> Hsv {
    app.panel
        .as_ref()
        .unwrap()
        .color_values
        .borrow()
        .get(&picker_key(edit))
        .filter(|(hex, _)| hex.eq_ignore_ascii_case(current))
        .map_or_else(|| Hsv::from_rgb(rgb), |(_, hsv)| *hsv)
}

fn current_color(app: &App, edit: &Edit) -> Hsv {
    if edit.path.starts_with("/theme:")
        && let Some((_, hsv)) = app
            .panel
            .as_ref()
            .unwrap()
            .color_values
            .borrow()
            .get(&picker_key(edit))
    {
        return *hsv;
    }
    let current = app
        .get(edit.doc, &edit.path)
        .and_then(Value::as_str)
        .unwrap_or("");
    let rgb = mypaper_config::Color::parse(current).map_or([139, 158, 183], |color| color.0);
    color_value(app, edit, current, rgb)
}

/// Native text editing starts from the same displayed color, including wallpaper theme fields.
pub fn field_initial(app: &App, id: &FieldId) -> Option<String> {
    let FieldId::Text(doc, path) = id else {
        return None;
    };
    let edit = Edit::new(*doc, path.clone(), "");
    if !app.is_open(picker_key(&edit)) {
        return None;
    }
    app.panel
        .as_ref()?
        .color_values
        .borrow()
        .get(&picker_key(&edit))
        .map(|(_, hsv)| mypaper_config::Color(hsv.rgb()).hex())
}

fn remember_color(app: &App, edit: &Edit, hsv: Hsv) -> Value {
    let hex = mypaper_config::Color(hsv.rgb()).hex();
    let mut cache = app.panel.as_ref().unwrap().color_values.borrow_mut();
    if cache.len() > 32 {
        cache.clear();
    }
    cache.insert(picker_key(edit), (hex.clone(), hsv));
    json!(hex)
}

/// The panel uses the kit picker; the legacy dialog and its owner remain untouched.
pub fn pick(app: &mut App, edit: &Edit) {
    let picker = picker_key(edit);
    let opening = !app.is_open(picker);
    let return_focus = app
        .frame
        .targets
        .iter()
        .find(|target| target.action == Action::Panel(Command::PickColor(edit.clone())))
        .map(|target| target.key);
    let others: Vec<_> = app
        .frame
        .targets
        .iter()
        .filter_map(|target| {
            if let Action::Panel(Command::ColorDrag(other, _, _)) = &target.action {
                Some(picker_key(other))
            } else {
                None
            }
        })
        .collect();
    for other in others {
        if other != picker {
            app.nav.more.remove(&other);
        }
    }
    if app.field.as_ref().is_some_and(|field| matches!(&field.id, FieldId::Text(doc,path) if *doc == edit.doc && *path == edit.path)) {
        app.blur_field();
    }
    app.run(Action::More(picker));
    app.input.focus = if opening {
        Some(key((picker, "sv")))
    } else {
        return_focus
    };
}

#[derive(Debug, Clone)]
pub struct Drag {
    edit: Edit,
    part: Part,
    rect: Rect,
    hsv: Hsv,
}

fn first_tint(app: &App, edit: &Edit) -> bool {
    edit.doc == crate::docs::DocId::Config
        && edit.path.as_ref() == "/look/material/tint"
        && app.get(edit.doc, &edit.path).is_none()
        && app
            .get(edit.doc, "/look/material/clarity")
            .and_then(Value::as_f64)
            == Some(1.0)
}

fn color_ops(app: &App, edit: &Edit, value: Value) -> Vec<crate::docs::Op> {
    let mut ops = vec![crate::docs::Op {
        doc: edit.doc,
        path: edit.path.to_string(),
        value: Some(value),
    }];
    // The first tint must be visible on completely clear glass, as in the previous picker.
    if first_tint(app, edit) {
        ops.push(crate::docs::Op {
            doc: edit.doc,
            path: "/look/material/clarity".into(),
            value: Some(json!(0.88)),
        });
    }
    ops
}

pub fn intercept_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if !first_tint(app, edit)
        || value
            .as_str()
            .and_then(mypaper_config::Color::parse)
            .is_none()
    {
        return false;
    }
    app.apply(&edit.label, color_ops(app, edit, value.clone()));
    true
}

fn live(app: &mut App, drag: &Drag, at: Point) {
    let hsv = drag.hsv.at(drag.part, drag.rect, at);
    let value = remember_color(app, &drag.edit, hsv);
    if drag.edit.doc == crate::docs::DocId::Studio {
        app.pending_value = Some((drag.edit.clone(), value));
    } else {
        let mut ops = color_ops(app, &drag.edit, value);
        if drag.edit.doc == crate::docs::DocId::Config && drag.edit.path.starts_with("/look") {
            ops.extend(crate::look::projection_after(&app.docs, &ops));
        }
        if let Err(error) = app.docs.live(&ops) {
            super::save_failed(app, Some(drag.edit.doc), &format!("{error:#}"));
        }
    }
    app.input.pointer = Some(at);
    app.dirty = true;
}

/// Reuse the app's slider commit path: each pointer move previews; release writes once.
pub fn event(app: &mut App, event: &Event) -> bool {
    if let Event::Press(pointer, Button::Left) = event {
        let target = app
            .frame
            .targets
            .iter()
            .rev()
            .find(|target| target.visible.is_some_and(|rect| rect.contains(pointer.at)))
            .cloned();
        if let Some(target) = target
            && let Action::Panel(Command::ColorDrag(edit, part, rect)) = target.action
        {
            app.blur_field();
            let drag = Drag {
                hsv: current_color(app, &edit),
                edit: edit.clone(),
                part,
                rect,
            };
            app.panel.as_mut().unwrap().color_drag = Some(drag.clone());
            app.input.pressed = Some(target.key);
            app.input.focus = Some(target.key);
            app.input.focus_visible = false;
            app.drag = Some(crate::app::Drag::Slider {
                edit,
                track: rect,
                min: 0.0,
                max: 1.0,
                step: 0.01,
            });
            if let Some(window) = app.window {
                // SAFETY: capture only our own panel; the existing slider end releases it.
                unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(window);
                }
            }
            live(app, &drag, pointer.at);
            return true;
        }
    }
    let Some(drag) = app.panel.as_ref().unwrap().color_drag.clone() else {
        return false;
    };
    match event {
        Event::Move(pointer) => {
            live(app, &drag, pointer.at);
            true
        }
        Event::Release(pointer, Button::Left) => {
            live(app, &drag, pointer.at);
            app.panel.as_mut().unwrap().color_drag = None;
            false // The app's normal release commits this drag and its History row.
        }
        Event::CaptureLost => {
            app.panel.as_mut().unwrap().color_drag = None;
            false
        }
        Event::Key(0x1B) => {
            app.panel.as_mut().unwrap().color_drag = None;
            app.event(Event::CaptureLost);
            false
        }
        _ => false,
    }
}

pub fn on_key(app: &mut App, vk: u16, ctrl: bool, shift: bool, alt: bool) -> bool {
    if ctrl || alt {
        return false;
    }
    if vk == 0x1B {
        let edit = app.frame.targets.iter().find_map(|target| {
            if let Action::Panel(Command::ColorDrag(edit, _, _)) = &target.action {
                Some(edit.clone())
            } else {
                None
            }
        });
        if let Some(edit) = edit {
            pick(app, &edit);
            return true;
        }
    }
    if app.field.is_some() {
        return false;
    }
    let target = app
        .input
        .focus
        .and_then(|focus| app.frame.targets.iter().find(|target| target.key == focus))
        .cloned();
    let Some(target) = target else { return false };
    let Action::Panel(Command::ColorDrag(edit, part, _)) = target.action else {
        return false;
    };
    if !matches!(vk, 0x25..=0x28 | 0x24 | 0x23) {
        return false;
    }
    let mut hsv = current_color(app, &edit);
    let step = if shift { 0.1 } else { 0.01 };
    match part {
        Part::Hue => {
            hsv.hue = match vk {
                0x24 => 0.0,
                0x23 => 359.0,
                0x25 | 0x28 => (hsv.hue - step * 100.0).rem_euclid(360.0),
                _ => (hsv.hue + step * 100.0).rem_euclid(360.0),
            }
        }
        Part::SaturationValue => match vk {
            0x25 => hsv.saturation = (hsv.saturation - step).max(0.0),
            0x27 => hsv.saturation = (hsv.saturation + step).min(1.0),
            0x26 => hsv.value = (hsv.value + step).min(1.0),
            0x28 => hsv.value = (hsv.value - step).max(0.0),
            0x24 => hsv.value = 0.0,
            _ => hsv.value = 1.0,
        },
    }
    let value = remember_color(app, &edit, hsv);
    app.set(&edit, value);
    true
}

pub fn accessibility(
    app: &mut App,
    target: &mypaper_ui::controls::Target<Action>,
    action: &mypaper_ui::AccessibilityAction,
) -> bool {
    use mypaper_ui::AccessibilityAction as A;
    let Action::Panel(Command::ColorDrag(edit, part, _)) = &target.action else {
        return false;
    };
    let mut hsv = current_color(app, edit);
    match (part, action) {
        (Part::Hue, A::SetNumber(value)) if value.is_finite() => {
            hsv.hue = (*value as f32).clamp(0.0, 359.0)
        }
        (Part::Hue, A::Increment) => hsv.hue = (hsv.hue + 1.0).rem_euclid(360.0),
        (Part::Hue, A::Decrement) => hsv.hue = (hsv.hue - 1.0).rem_euclid(360.0),
        (Part::SaturationValue, A::Increment) => hsv.saturation = (hsv.saturation + 0.01).min(1.0),
        (Part::SaturationValue, A::Decrement) => hsv.saturation = (hsv.saturation - 0.01).max(0.0),
        _ => return true,
    }
    let value = remember_color(app, edit, hsv);
    app.set(edit, value);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docs::DocId,
        panel::{Page, State},
    };
    use mypaper_ui::{Pointer, Ui};

    fn app(ui: &mut Ui) -> App {
        let mut app = crate::goldens::app(true, ui);
        app.panel = Some(State {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 1800);
        app.nav.more.insert(picker_key(&Edit::new(
            DocId::Config,
            "/look/material/tint",
            "Tint",
        )));
        app.frame.targets = crate::panel::build(&app, &mut ui.text).targets;
        app
    }

    #[test]
    fn color_picker_flips_inside_the_viewport_and_hex_starts_with_the_displayed_color() {
        let mut ui = Ui::new().unwrap();
        let mut app = app(&mut ui);
        let edit = Edit::new(DocId::Config, "/look/material/tint", "Tint");
        app.run(Action::Set(edit.clone(), json!("#2379AB")));
        app.size = (360, 620);
        let built = crate::panel::build(&app, &mut ui.text);
        for target in built
            .targets
            .iter()
            .filter(|target| matches!(&target.action, Action::Panel(Command::ColorDrag(..))))
        {
            assert!(
                target.rect.top >= 48.0 && target.rect.bottom <= 620.0,
                "{:?}",
                target.rect
            );
        }
        assert_eq!(
            field_initial(&app, &FieldId::Text(edit.doc, edit.path)),
            Some("#2379ab".into())
        );
    }

    #[test]
    fn color_drag_previews_all_projected_documents_and_commits_one_undo_step() {
        let mut ui = Ui::new().unwrap();
        let mut app = app(&mut ui);
        app.run(Action::Set(
            Edit::new(DocId::Config, "/look/material/clarity", "Clarity"),
            json!(1.0),
        ));
        let before_count = app.docs.undo_count();
        let original: Vec<_> = [DocId::Config, DocId::Bars, DocId::Widgets]
            .into_iter()
            .map(|doc| (doc, app.docs.doc(doc).raw.clone()))
            .collect();
        let square = app
            .frame
            .targets
            .iter()
            .find_map(|target| {
                if let Action::Panel(Command::ColorDrag(_, Part::SaturationValue, rect)) =
                    target.action
                {
                    Some(rect)
                } else {
                    None
                }
            })
            .unwrap();
        let pointer = |x, y| Pointer {
            at: Point::new(
                square.left + square.width() * x,
                square.top + square.height() * y,
            ),
            screen: (0, 0),
        };
        app.event(Event::Press(pointer(0.25, 0.25), Button::Left));
        app.event(Event::Move(pointer(0.75, 0.5)));
        assert_eq!(
            app.docs.undo_count(),
            before_count,
            "movement only previews"
        );
        assert_eq!(
            app.get(DocId::Config, "/look/material/clarity"),
            Some(&json!(0.88))
        );
        let tint = app
            .get(DocId::Config, "/look/material/tint")
            .cloned()
            .unwrap();
        assert_eq!(app.get(DocId::Widgets, "/appearance/tint"), Some(&tint));
        assert_eq!(app.get(DocId::Bars, "/appearance/tint"), Some(&tint));
        app.event(Event::Release(pointer(0.75, 0.5), Button::Left));
        assert_eq!(app.docs.undo_count(), before_count + 1);
        assert!(app.drag.is_none());
        app.run(Action::Undo);
        for (doc, raw) in original {
            assert_eq!(app.docs.doc(doc).raw, raw);
        }
        assert!(
            crate::docs::DocId::ALL
                .into_iter()
                .all(|doc| app.docs.doc(doc).path.is_none())
        );
    }

    #[test]
    fn escape_closes_the_picker_before_leaving_look_and_hue_survives_grey() {
        let mut ui = Ui::new().unwrap();
        let mut app = app(&mut ui);
        app.run(Action::Set(
            Edit::new(DocId::Config, "/look/material/clarity", "Clarity"),
            json!(1.0),
        ));
        let edit = Edit::new(DocId::Config, "/look/material/tint", "Tint");
        let hsv = Hsv {
            hue: 210.0,
            saturation: 0.0,
            value: 0.7,
        };
        let value = remember_color(&app, &edit, hsv);
        app.set(&edit, value);
        assert_eq!(
            app.get(DocId::Config, "/look/material/clarity"),
            Some(&json!(0.88))
        );
        assert_eq!(current_color(&app, &edit).hue, 210.0);
        app.key_with(0x1B, (false, false, false));
        assert!(!app.is_open(picker_key(&edit)));
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Look);
        assert!(!app.quit);
    }
}
#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::{docs::DocId, panel::Page};
    use mypaper_ui::Ui;

    #[test]
    fn circular_choices_keep_names_selection_and_arrow_navigation() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::window_app(true, &ui, (1280, 800));
        let edit = Edit::new(DocId::Config, "/look/accent", "Accent");
        let mut b = crate::ui::builder(
            &mut ui.text,
            &app.theme.palette,
            &app.theme.ty,
            1.25,
            &app.input,
        );
        let mut c = Col {
            x: 10.0,
            y: 10.0,
            w: 360.0,
            view: key("swatch-test"),
        };
        swatches(
            &app,
            &mut b,
            &mut c,
            &edit,
            Some(&json!("#8b9eb7")),
            true,
            false,
            &super::super::material::SWATCHES,
        );
        assert_eq!(b.targets.len(), 9);
        assert_eq!(b.list.hits[2].label.as_ref(), "Accent: Blue");
        assert_eq!(b.list.hits[2].checked, Some(true));
        let group = b.targets[0].group;
        assert!(group.is_some() && b.targets.iter().all(|target| target.group == group));
        assert!(b.targets.iter().all(|target| target.rect
            == Rect::new(
                target.rect.left,
                b.targets[0].rect.top,
                b.targets[0].rect.width(),
                b.targets[0].rect.height()
            )));
        let next = b.targets[3].key;
        let targets = b.targets;
        app.input.focus = Some(targets[2].key);
        app.frame.targets = targets;
        app.key_with(0x27, (false, false, false));
        assert_eq!(app.input.focus, Some(next));
    }

    /// Review output stays outside the checkout; shared golden baselines are never rewritten.
    #[test]
    #[ignore = "writes the explicit Studio control review matrix"]
    fn render_controls_review() {
        use mypaper_ui::{golden, render};
        let out =
            std::path::PathBuf::from(std::env::var_os("MYPAPER_STUDIO_WINDOW_SHOTS").unwrap());
        let mut ui = Ui::new().unwrap();
        for dark in [true, false] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for state in ["rest", "hover", "drag", "focus"] {
                    let app = crate::goldens::window_app(dark, &ui, (1280, 800));
                    let palette = crate::panel::palette_for(&app);
                    let mut input = app.input.clone();
                    match state {
                        "hover" => input.hover = Some(key("slider-review")),
                        "drag" => input.pressed = Some(key("slider-review")),
                        "focus" => {
                            input.focus = Some(key("slider-review"));
                            input.focus_visible = true;
                        }
                        _ => {}
                    }
                    let mut b =
                        crate::ui::builder(&mut ui.text, &palette, &app.theme.ty, scale, &input);
                    b.fill(
                        Rect::new(0.0, 0.0, 640.0 * scale, 300.0 * scale),
                        0.0,
                        b.p.panel,
                    );
                    b.segmented(
                        key("segments-review"),
                        Rect::new(20.0 * scale, 20.0 * scale, 600.0 * scale, 32.0 * scale),
                        &["Integrated", "High performance", "Windows' choice"],
                        Some(0),
                        "Graphics card",
                        true,
                        |_| Action::None,
                    );
                    for (row, value) in [0.0, 37.3, 100.0].into_iter().enumerate() {
                        let slider_key = if row == 1 {
                            key("slider-review")
                        } else {
                            key(("slider-review", row))
                        };
                        b.slider(
                            slider_key,
                            Rect::new(
                                20.0 * scale,
                                (65.0 + row as f32 * 40.0) * scale,
                                600.0 * scale,
                                32.0 * scale,
                            ),
                            value,
                            (0.0, 100.0, 1.0),
                            &format!("{value}%"),
                            48.0 * scale,
                            "Opacity",
                            Action::None,
                        );
                    }
                    let mut c = Col {
                        x: 20.0 * scale,
                        y: 200.0 * scale,
                        w: 340.0 * scale,
                        view: key("colors-review"),
                    };
                    swatches(
                        &app,
                        &mut b,
                        &mut c,
                        &Edit::new(DocId::Config, "/look/accent", "Accent"),
                        Some(&json!("auto")),
                        true,
                        false,
                        &super::super::material::SWATCHES,
                    );
                    assert_eq!(
                        b.list
                            .hits
                            .iter()
                            .filter(|hit| hit.role == mypaper_ui::Role::Slider)
                            .count(),
                        3
                    );
                    let list = b.list;
                    let image = render(
                        &mut ui,
                        &list,
                        (640.0 * scale) as u32,
                        (300.0 * scale) as u32,
                    )
                    .unwrap();
                    golden::write(
                        &image,
                        &out.join(format!(
                            "controls-{}-{}-{state}.png",
                            if dark { "dark" } else { "light" },
                            (scale * 100.0) as u32
                        )),
                    )
                    .unwrap();
                }
            }
        }
        for size in [(1280, 800), (1920, 1080)] {
            for dark in [true, false] {
                for collapsed in [false, true] {
                    for (page, name) in [
                        (Page::Root, "home"),
                        (Page::Wallpapers, "wallpapers"),
                        (Page::Look, "look"),
                        (Page::Windows, "windows"),
                        (Page::Bars, "bars"),
                        (Page::Widget, "widget"),
                        (Page::Settings, "settings"),
                        (Page::Performance, "performance"),
                        (Page::History, "history"),
                        (Page::FirstRun, "first-run"),
                        (Page::Look, "off"),
                    ] {
                        if matches!(page, Page::History) {
                            continue;
                        }
                        let mut app = crate::goldens::window_app(dark, &ui, size);
                        if collapsed {
                            crate::view::toggle_sidebar(&mut app);
                            app.toast = None;
                        }
                        app.run(Action::Panel(Command::Open(page)));
                        if page == Page::Widget {
                            app.nav.widget = Some("clock-1".into());
                        }
                        if name == "off" {
                            app.system
                                .parts
                                .iter_mut()
                                .for_each(|part| part.running = false);
                        }
                        let image = crate::goldens::shot(&mut app, &mut ui);
                        golden::write(
                            &image,
                            &out.join(format!(
                                "{name}{}-{}x{}-{}.png",
                                if collapsed { "-collapsed" } else { "" },
                                size.0,
                                size.1,
                                if dark { "dark" } else { "light" }
                            )),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}
