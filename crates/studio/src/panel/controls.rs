//! Panel row anatomy on the existing kit's drawing, focus, accessibility and hit machinery.

use mypaper_ui::controls::{Align, Behavior, Font, Glyph, key};
use mypaper_ui::{Point, Rect, Rgba, Role, TextStyle};
use serde_json::Value;

use crate::actions::{Action, Choices, Edit, FieldId};
use crate::app::{App, field_key};
use crate::settings::{Kind, Setting};
use crate::theme::glyph;
use crate::ui::{B, Col};

pub fn font(b: &B, size: f32, weight: u16) -> TextStyle {
    let mut style = b.font(Font::Body);
    style.size *= size / 14.0;
    style.weight = weight;
    style.optical = style.size / b.s * 0.75;
    style
}

pub fn text(b: &mut B, r: Rect, label: &str, size: f32, weight: u16, ink: Rgba, align: Align) {
    let style = font(b, size, weight);
    b.text_in(r, label, &style, ink, align);
}

pub fn height(b: &B, base: f32) -> f32 {
    b.px(base * b.t.text_scale)
}

pub fn title(b: &mut B, c: &mut Col, title: &str, subtitle: &str) {
    let h = height(b, 28.0);
    text(
        b,
        Rect::new(c.x, c.y, c.w, h),
        title,
        20.0,
        600,
        b.p.ink1,
        Align::Start,
    );
    c.y += h;
    if !subtitle.is_empty() {
        let h = height(b, 20.0);
        text(
            b,
            Rect::new(c.x, c.y, c.w, h),
            subtitle,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
        c.y += h;
    }
    c.y += b.px(4.0);
}

pub fn group(b: &mut B, c: &mut Col, heading: &str, content: impl FnOnce(&mut B, &mut Col)) {
    if !heading.is_empty() {
        c.y += b.px(16.0);
        let h = height(b, 20.0);
        text(
            b,
            Rect::new(c.x, c.y, c.w, h),
            heading,
            13.0,
            600,
            b.p.ink1,
            Align::Start,
        );
        c.y += h;
    }
    c.y += b.px(8.0);
    let start = c.y;
    let at = b.list.ops.len();
    let mut inner = Col {
        x: c.x + b.px(12.0),
        y: start + b.px(2.0),
        w: c.w - b.px(24.0),
        view: c.view,
    };
    content(b, &mut inner);
    // The plate is inserted under its contents after their height is known.
    b.list.ops.insert(
        at,
        mypaper_ui::Op::Fill {
            rect: Rect::new(c.x, start, c.w, inner.y - start + b.px(2.0)),
            radius: b.px(10.0),
            color: b.p.panel,
        },
    );
    b.fill(
        Rect::new(inner.x, inner.y - 1.0, inner.w, 1.0),
        0.0,
        b.p.panel,
    );
    c.y = inner.y + b.px(2.0);
}

pub fn button(
    b: &mut B,
    r: Rect,
    label: &str,
    icon: Option<Glyph>,
    primary: bool,
    enabled: bool,
    action: Action,
) {
    let k = key(("panel-button", label));
    let hot = b.hovered(k) || b.pressed(k);
    b.fill(
        r,
        b.px(7.0),
        if primary {
            b.p.solid
        } else if hot && enabled {
            b.p.hover
        } else {
            b.p.fill
        },
    );
    let ink = if !enabled {
        b.p.ink3
    } else if primary {
        b.p.on_solid
    } else {
        b.p.ink1
    };
    let left = if let Some(g) = icon {
        b.icon(
            Rect::new(r.left + b.px(9.0), r.top, b.px(14.0), r.height()),
            g,
            13.0,
            ink,
        );
        r.left + b.px(27.0)
    } else {
        r.left
    };
    text(
        b,
        Rect::new(
            left,
            r.top,
            r.right - left - if icon.is_some() { b.px(8.0) } else { 0.0 },
            r.height(),
        ),
        label,
        13.0,
        500,
        ink,
        Align::Center,
    );
    if enabled {
        b.hit(k, r, Role::Button, label, action, Behavior::Click);
        b.focus_ring(k, r, b.px(7.0));
    } else {
        // Keep deferred actions discoverable to screen readers without making them focus stops.
        b.list.hits.push(mypaper_ui::Hit {
            id: u32::MAX - b.list.hits.len() as u32,
            rect: r,
            role: Role::Button,
            label: format!("{label} (coming soon)").into(),
            enabled: false,
            checked: None,
        });
    }
}

pub fn icon_button(b: &mut B, r: Rect, icon: Glyph, label: &str, enabled: bool, action: Action) {
    let k = key(("panel-icon", label));
    if enabled {
        b.wash(k, r, b.px(6.0));
    }
    b.icon(
        r,
        icon,
        16.0,
        if enabled {
            b.p.ink2
        } else {
            b.p.ink3.with_alpha(0.6)
        },
    );
    if enabled {
        b.hit(k, r, Role::Button, label, action, Behavior::Click);
        b.focus_ring(k, r, b.px(6.0));
    } else {
        b.list.hits.push(mypaper_ui::Hit {
            id: u32::MAX - b.list.hits.len() as u32,
            rect: r,
            role: Role::Button,
            label: label.into(),
            enabled: false,
            checked: None,
        });
    }
}

/// A 40-DIP row, or 56 with a caption. Values yield space to the label and ellipsize.
pub fn row(
    b: &mut B,
    c: &mut Col,
    icon: Option<Glyph>,
    label: &str,
    value: &str,
    caption: &str,
    checked: Option<bool>,
    action: Action,
) {
    let h = height(b, if caption.is_empty() { 40.0 } else { 56.0 });
    let r = Rect::new(c.x, c.y, c.w, h);
    let k = key(("panel-row", c.view, label, caption));
    let on = checked == Some(true);
    let area = Rect::new(c.x - b.px(8.0), c.y, c.w + b.px(16.0), h);
    if on {
        b.fill(area, b.px(8.0), b.p.selected);
    } else {
        b.wash(k, area, b.px(8.0));
    }
    let mut x = r.left;
    if let Some(g) = icon {
        let gw = b.px(if caption.is_empty() { 18.0 } else { 28.0 });
        let tile = Rect::new(x, c.y + (h - gw) / 2.0, gw, gw);
        if !caption.is_empty() {
            b.fill(tile, b.px(7.0), b.p.fill);
        }
        b.icon(tile, g, 16.0, b.p.ink2);
        x += gw + b.px(10.0);
    }
    let tail = Rect::new(r.right - b.px(16.0), r.top, b.px(16.0), h);
    if let Some(checked) = checked {
        if checked {
            b.icon(tail, glyph::CHECK, 16.0, b.p.ink1);
        }
    } else {
        b.icon(tail, glyph::CHEVRON_RIGHT, 15.0, b.p.ink3);
    }
    let end = tail.left - b.px(8.0);
    let value_w = if value.is_empty() {
        0.0
    } else {
        let style = font(b, 13.0, 400);
        b.measure(value, &style)
            .0
            .min((end - x - b.px(140.0)).max(0.0))
    };
    let label_end = end - value_w - if value_w > 0.0 { b.px(12.0) } else { 0.0 };
    let label_h = if caption.is_empty() {
        h
    } else {
        height(b, 22.0)
    };
    let y = if caption.is_empty() {
        c.y
    } else {
        c.y + (h - height(b, 40.0)) / 2.0
    };
    text(
        b,
        Rect::new(x, y, (label_end - x).max(0.0), label_h),
        label,
        13.0,
        if on { 600 } else { 500 },
        b.p.ink1,
        Align::Start,
    );
    if !caption.is_empty() {
        text(
            b,
            Rect::new(x, y + label_h, end - x, height(b, 18.0)),
            caption,
            12.0,
            400,
            if on { b.p.ink2 } else { b.p.ink3 },
            Align::Start,
        );
    }
    if value_w > 0.0 {
        text(
            b,
            Rect::new(end - value_w, c.y, value_w, h),
            value,
            13.0,
            400,
            b.p.ink2,
            Align::End,
        );
    }
    let label = format!("{label} {value} {caption}");
    b.hit_checked(
        k,
        area,
        if checked.is_some() {
            Role::RadioButton
        } else {
            Role::Button
        },
        &label,
        checked,
        action,
        Behavior::Click,
    );
    b.focus_ring(k, area, b.px(8.0));
    b.hline(c.x, c.right(), r.bottom - 1.0, b.p.line);
    c.y += h;
}

pub fn setting(app: &App, b: &mut B, c: &mut Col, id: &'static str, label: &str, caption: &str) {
    let s = Setting::get(id).expect("panel uses known settings");
    let value = app.value(s);
    let edit = Edit::new(s.doc, s.path, s.label);
    let h = height(b, if caption.is_empty() { 40.0 } else { 56.0 });
    let r = Rect::new(c.x, c.y, c.w, h);
    let text_h = if caption.is_empty() {
        h
    } else {
        height(b, 24.0)
    };
    let y = if caption.is_empty() {
        c.y
    } else {
        c.y + b.px(6.0)
    };
    let label_w = b.px(140.0).min(c.w * 0.46);
    text(
        b,
        Rect::new(c.x, y, label_w, text_h),
        label,
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    if !caption.is_empty() {
        text(
            b,
            Rect::new(c.x, y + text_h, c.w - b.px(48.0), height(b, 18.0)),
            caption,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
    match s.kind {
        Kind::Switch { .. } => {
            let on = value.as_bool().unwrap_or(false);
            b.switch(
                key(("panel-setting", id)),
                Point::new(r.right - b.px(36.0), c.y + (h - b.px(20.0)) / 2.0),
                on,
                label,
                Action::Toggle(edit, on),
            );
            c.y += h;
        }
        Kind::Select { options, .. } => {
            let shown = options
                .iter()
                .find(|(v, _)| serde_json::from_str::<Value>(v).is_ok_and(|v| v == value))
                .map_or("Custom", |(_, l)| *l);
            let width = b.px(164.0).min(c.w - label_w - b.px(8.0));
            let control = Rect::new(
                r.right - width,
                c.y + (h - b.px(28.0)) / 2.0,
                width,
                b.px(28.0),
            );
            let k = key(("panel-setting", id));
            b.fill(control, b.px(6.0), b.p.fill);
            b.wash(k, control, b.px(6.0));
            text(
                b,
                Rect::new(
                    control.left + b.px(10.0),
                    control.top,
                    width - b.px(30.0),
                    control.height(),
                ),
                shown,
                13.0,
                400,
                b.p.ink1,
                Align::Start,
            );
            b.icon(
                Rect::new(
                    control.right - b.px(22.0),
                    control.top,
                    b.px(16.0),
                    control.height(),
                ),
                glyph::CHEVRON_DOWN,
                12.0,
                b.p.ink2,
            );
            b.hit(
                k,
                control,
                Role::ComboBox,
                label,
                Action::Choose(Choices {
                    edit,
                    options: options
                        .iter()
                        .map(|(v, l)| {
                            (
                                serde_json::from_str(v).expect("setting value"),
                                (*l).to_owned(),
                            )
                        })
                        .collect(),
                    current: Some(value),
                }),
                Behavior::Click,
            );
            b.focus_ring(k, control, b.px(6.0));
            c.y += h;
        }
        Kind::Choice { options, .. } => {
            // Keep three values on one line at 480; stack within the plate at laptop widths
            // or increased Windows text size rather than truncating any of the choices.
            let style = font(b, 12.0, 500);
            let cell = options
                .iter()
                .map(|(_, l)| b.measure(l, &style).0 + b.px(16.0))
                .fold(0.0, f32::max);
            let required = cell * options.len() as f32;
            let stacked = c.w - label_w - b.px(8.0) < required;
            let control = if stacked {
                c.y += height(b, 30.0);
                Rect::new(c.x, c.y, c.w, height(b, 30.0))
            } else {
                Rect::new(
                    r.right - required,
                    c.y + (h - height(b, 28.0)) / 2.0,
                    required,
                    height(b, 28.0),
                )
            };
            b.fill(control, b.px(7.0), b.p.fill);
            let sw = control.width() / options.len() as f32;
            b.group(key(("panel-segment", id)), |b| {
                for (i, (v, label)) in options.iter().enumerate() {
                    let v: Value = serde_json::from_str(v).expect("setting value");
                    let on = v == value;
                    let r = Rect::new(
                        control.left + sw * i as f32 + b.px(2.0),
                        control.top + b.px(2.0),
                        sw - b.px(4.0),
                        control.height() - b.px(4.0),
                    );
                    let k = key(("panel-segment", id, i));
                    if on {
                        b.fill(r, b.px(5.0), super::tokens::thumb(b.p.dark));
                    } else {
                        b.wash(k, r, b.px(5.0));
                    }
                    text(
                        b,
                        r,
                        label,
                        12.0,
                        500,
                        if on {
                            super::tokens::hex(0xffffff)
                        } else {
                            b.p.ink2
                        },
                        Align::Center,
                    );
                    b.hit_checked(
                        k,
                        r,
                        Role::RadioButton,
                        label,
                        Some(on),
                        Action::Set(edit.clone(), v),
                        Behavior::Click,
                    );
                    b.focus_ring(k, r, b.px(5.0));
                }
            });
            c.y = if stacked {
                control.bottom + b.px(8.0)
            } else {
                r.bottom
            };
        }
        _ => c.y += h,
    }
    b.hline(c.x, c.right(), c.y - 1.0, b.p.line);
}

pub fn search(app: &App, b: &mut B, c: &mut Col) {
    let id = FieldId::WallpaperSearch;
    let k = field_key(&id);
    let r = Rect::new(c.x, c.y, c.w, height(b, 32.0));
    b.fill(r, b.px(7.0), b.p.fill);
    b.icon(
        Rect::new(r.left + b.px(10.0), r.top, b.px(16.0), r.height()),
        glyph::SEARCH,
        15.0,
        b.p.ink3,
    );
    let inner = Rect::new(
        r.left + b.px(34.0),
        r.top + b.px(4.0),
        r.width() - b.px(96.0),
        r.height() - b.px(8.0),
    );
    if let Some(f) = app.field.as_ref().filter(|f| f.id == id) {
        f.field.set_rect(inner);
        b.list.ops.push(mypaper_ui::Op::TextField {
            rect: inner,
            field: f.field.clone(),
            revision: f.field.revision(),
        });
    } else {
        let value = app.text(&id);
        text(
            b,
            inner,
            if value.is_empty() {
                "Search everything"
            } else {
                value
            },
            13.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
    text(
        b,
        Rect::new(r.right - b.px(58.0), r.top, b.px(48.0), r.height()),
        "Ctrl K",
        11.0,
        500,
        b.p.ink3,
        Align::End,
    );
    b.hit(
        k,
        r,
        Role::TextInput,
        "Search everything",
        Action::Field(id),
        Behavior::Field,
    );
    b.focus_ring(k, r, b.px(7.0));
    c.y += r.height();
}

pub fn more(app: &App, b: &mut B, c: &mut Col, hint: &str) -> bool {
    c.y += b.px(8.0);
    let r = Rect::new(c.x, c.y, c.w, height(b, 36.0));
    let k = key(("panel-more", c.view));
    let open = app.is_open(k);
    text(b, r, "More", 13.0, 500, b.p.ink2, Align::Start);
    text(
        b,
        Rect::new(
            r.left + b.px(48.0),
            r.top,
            r.width() - b.px(76.0),
            r.height(),
        ),
        hint,
        12.0,
        400,
        b.p.ink3,
        Align::Start,
    );
    b.icon(
        Rect::new(r.right - b.px(16.0), r.top, b.px(16.0), r.height()),
        if open {
            mypaper_ui::icons::Icon::ChevronUp.into()
        } else {
            glyph::CHEVRON_DOWN
        },
        15.0,
        b.p.ink3,
    );
    b.hit_checked(
        k,
        r,
        Role::Button,
        "More",
        Some(open),
        Action::More(k),
        Behavior::Click,
    );
    b.focus_ring(k, r, b.px(7.0));
    c.y += r.height();
    open
}

pub fn notice(b: &mut B, c: &mut Col, message: &str) {
    let style = font(b, 12.0, 400);
    c.y += b.paragraph(c.x, c.y, c.w, message, &style, height(b, 18.0), b.p.ink3) + b.px(12.0);
}
