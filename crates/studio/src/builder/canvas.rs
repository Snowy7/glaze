//! Canvas positions use the stack contract's top-left, normalized screen coordinates.
use super::{active, raw, state};
use crate::{app::App, docs::Op, panel::Page, ui::B};
use glaze_ui::{Button, Event, Point, Rect};
use serde_json::{Value, json};

#[derive(Debug)]
pub(super) struct Drag {
    canvas: Rect,
    start: Point,
    path: String,
    before: Value,
    focus: bool,
}

pub(super) fn geometry(app: &App, area: Rect) -> Rect {
    crate::stage::frame(
        app,
        Rect::new(
            area.left + app.px(28.0),
            area.top + app.px(24.0),
            area.width() - app.px(56.0),
            area.height() - app.px(88.0),
        ),
    )
}

fn layer_rect(canvas: Rect, value: &Value) -> Rect {
    let n = |i| value[i].as_f64().unwrap_or(0.0) as f32;
    Rect::new(
        canvas.left + n(0) * canvas.width(),
        canvas.top + n(1) * canvas.height(),
        n(2) * canvas.width(),
        n(3) * canvas.height(),
    )
}

pub(super) fn overlay(app: &App, b: &mut B, canvas: Rect) {
    if crate::panel::current_page(app) != Page::Create {
        return;
    }
    let value = raw(app);
    let editor = active(app).unwrap();
    let ink = b.p.ink1;
    b.list.ops.push(glaze_ui::Op::Clip { rect: canvas });
    if let Some(i) = editor.selected.filter(|&i| {
        matches!(
            value["wallpaper"]["layers"][i]["kind"].as_str(),
            Some("image" | "model")
        )
    }) {
        b.stroke(
            layer_rect(canvas, &value["wallpaper"]["layers"][i]["rect"]),
            0.0,
            b.px(1.5),
            ink,
        );
    } else if editor.selected.is_none() && value["wallpaper"]["base"]["kind"] == "image" {
        let focus = &value["wallpaper"]["base"]["focus"];
        let x = canvas.left + focus[0].as_f64().unwrap_or(0.5) as f32 * canvas.width();
        let y = canvas.top + focus[1].as_f64().unwrap_or(0.5) as f32 * canvas.height();
        b.fill(
            Rect::new(x - b.px(9.0), y - b.px(0.75), b.px(18.0), b.px(1.5)),
            0.0,
            ink,
        );
        b.fill(
            Rect::new(x - b.px(0.75), y - b.px(9.0), b.px(1.5), b.px(18.0)),
            0.0,
            ink,
        );
    }
    b.list.ops.push(glaze_ui::Op::Unclip);
}

fn moved(drag: &Drag, at: Point) -> Value {
    let mut value = drag.before.clone();
    if drag.focus {
        value = json!([
            ((at.x - drag.canvas.left) / drag.canvas.width()).clamp(0.0, 1.0),
            ((at.y - drag.canvas.top) / drag.canvas.height()).clamp(0.0, 1.0)
        ]);
    } else {
        value[0] = json!(
            (drag.before[0].as_f64().unwrap_or(0.0)
                + f64::from((at.x - drag.start.x) / drag.canvas.width()))
            .clamp(0.0, 1.0)
        );
        value[1] = json!(
            (drag.before[1].as_f64().unwrap_or(0.0)
                + f64::from((at.y - drag.start.y) / drag.canvas.height()))
            .clamp(0.0, 1.0)
        );
    }
    value
}

fn update(app: &mut App, at: Point) {
    let editor = active(app).unwrap();
    let drag = editor.canvas_drag.as_ref().unwrap();
    let op = Op {
        doc: editor.doc,
        path: drag.path.clone(),
        value: Some(moved(drag, at)),
    };
    if let Err(error) = app.docs.live(&[op]) {
        app.alert(format!("Position: {error:#}"));
    }
    app.input.pointer = Some(at);
    app.dirty = true;
}

pub(super) fn event(app: &mut App, event: &Event) -> bool {
    let Some(editor) = active(app) else {
        return false;
    };
    if editor.canvas_drag.is_some() {
        match event {
            Event::Move(p) => update(app, p.at),
            Event::Release(p, Button::Left) => {
                update(app, p.at);
                state(app).canvas_drag = None;
                if let Err(error) = app.docs.commit_live("Position layer") {
                    app.alert(format!("Position: {error:#}"));
                }
                release(app);
            }
            Event::CaptureLost | Event::Key(0x1B) => {
                let drag = state(app).canvas_drag.take().unwrap();
                let doc = active(app).unwrap().doc;
                let _ = app.docs.live(&[Op {
                    doc,
                    path: drag.path,
                    value: Some(drag.before),
                }]);
                let _ = app.docs.commit_live("Position layer");
                let _ = app.docs.preview_live(doc);
                release(app);
            }
            _ => return false,
        }
        return true;
    }
    let Event::Press(p, Button::Left) = event else {
        return false;
    };
    if crate::panel::current_page(app) != Page::Create || editor.fresh {
        return false;
    }
    let Some(area) = crate::view::regions(app).stage else {
        return false;
    };
    let canvas = geometry(app, area);
    if !canvas.contains(p.at) {
        return false;
    }
    let value = raw(app);
    let layers = value["wallpaper"]["layers"].as_array().unwrap();
    let hit = |i: usize| {
        matches!(layers[i]["kind"].as_str(), Some("image" | "model"))
            && layers[i]["enabled"] != false
            && layer_rect(canvas, &layers[i]["rect"]).contains(p.at)
    };
    let selected = editor
        .selected
        .filter(|&i| i < layers.len() && hit(i))
        .or_else(|| (0..layers.len()).rev().find(|&i| hit(i)));
    let (path, before, focus) = if let Some(i) = selected {
        (
            format!("/wallpaper/layers/{i}/rect"),
            layers[i]["rect"].clone(),
            false,
        )
    } else if value["wallpaper"]["base"]["kind"] == "image" {
        (
            "/wallpaper/base/focus".into(),
            value["wallpaper"]["base"]["focus"].clone(),
            true,
        )
    } else {
        return false;
    };
    app.blur_field();
    state(app).selected = selected;
    state(app).canvas_drag = Some(Drag {
        canvas,
        start: p.at,
        path,
        before,
        focus,
    });
    if let Some(window) = app.window {
        // SAFETY: only this editor's own HWND captures this drag.
        unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(window);
        }
    }
    app.dirty = true;
    true
}

fn release(app: &mut App) {
    if app.window.is_some() {
        // SAFETY: paired with the editor's SetCapture above.
        let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
    }
    app.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_uses_screen_fractions_and_preserves_layer_size() {
        let drag = Drag {
            canvas: Rect::new(50.0, 100.0, 800.0, 400.0),
            start: Point { x: 250.0, y: 200.0 },
            path: String::new(),
            before: json!([0.2, 0.1, 0.3, 0.4]),
            focus: false,
        };
        let value = moved(&drag, Point { x: 450.0, y: 300.0 });
        assert!((value[0].as_f64().unwrap() - 0.45).abs() < 0.0001);
        assert!((value[1].as_f64().unwrap() - 0.35).abs() < 0.0001);
        assert_eq!(&value.as_array().unwrap()[2..], &[json!(0.3), json!(0.4)]);
        assert_eq!(
            moved(
                &drag,
                Point {
                    x: -900.0,
                    y: 2000.0
                }
            )[0],
            0.0
        );
    }

    #[test]
    fn photo_focus_tracks_the_pointer_and_stays_inside_the_image() {
        let drag = Drag {
            canvas: Rect::new(50.0, 100.0, 800.0, 400.0),
            start: Point { x: 0.0, y: 0.0 },
            path: String::new(),
            before: json!([0.5, 0.5]),
            focus: true,
        };
        assert_eq!(
            moved(&drag, Point { x: 250.0, y: 400.0 }),
            json!([0.25, 0.75])
        );
        assert_eq!(
            moved(&drag, Point { x: -20.0, y: 900.0 }),
            json!([0.0, 1.0])
        );
    }
}
