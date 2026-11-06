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

/// Shared readout grammar: percent closes up; physical and time units take one space.
pub fn unit(value: f64, unit: &str) -> String {
    let unit = unit.trim();
    format!("{value:.0}{}{unit}", if unit.is_empty() || unit == "%" { "" } else { " " })
}

/// All inline sliders reserve the same label column; narrow and large-text rows stack.
pub fn label_width(b: &B, c: &Col) -> f32 {
    b.px(140.0).min(c.w * 0.46)
}

pub fn displays(app: &App, b: &mut B, c: &mut Col) {
    if app.monitors.len() < 2 {
        return;
    }
    let labels: Vec<_> = (1..=app.monitors.len())
        .map(|i| format!("Display {i}"))
        .collect();
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let r = Rect::new(c.x, c.y + b.px(6.0), c.w, b.px(30.0));
    b.segmented(
        key(("panel-displays", c.view)),
        r,
        &labels,
        Some(app.nav.display),
        "Display",
        true,
        |i| Action::Panel(super::Command::Display(i)),
    );
    b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
    c.y = r.bottom + b.px(4.0);
}

/// File feedback stays beside the pages that can edit that file.
pub fn relevant_docs(page: super::Page) -> &'static [crate::docs::DocId] {
    use super::Page;
    use crate::docs::DocId;
    match page {
        Page::Widgets | Page::Widget | Page::AddWidget => &[DocId::Widgets],
        Page::Bars
        | Page::BarDesign
        | Page::Top
        | Page::Dock
        | Page::Module
        | Page::AddModule
        | Page::Panels
        | Page::BarPanel => &[DocId::Bars],
        Page::Connections | Page::Connection => &[
            DocId::Integrations,
            DocId::Plugins,
            DocId::Bars,
            DocId::Widgets,
        ],
        Page::History => &[DocId::Config, DocId::Bars, DocId::Widgets],
        Page::FirstRun => &[DocId::Config, DocId::Bars, DocId::Widgets, DocId::Studio],
        Page::General => &[DocId::Studio, DocId::Config],
        Page::Root
        | Page::Look
        | Page::Windows
        | Page::Performance
        | Page::DesktopMenu
        | Page::Wallpapers
        | Page::Wallpaper
        | Page::AddWallpaper => &[DocId::Config],
        _ => &[],
    }
}

pub fn save_banner(app: &App, b: &mut B, c: &mut Col, page: super::Page) {
    super::saving::banner(app, b, c, page);
    use crate::docs::ReadIssue;
    for &doc in relevant_docs(page) {
        let state = app.panel.as_ref().unwrap();
        let issue = app.docs.doc(doc).issue;
        let write_failed = state.save_errors.borrow().contains_key(&doc);
        if issue.is_none() && !write_failed {
            continue;
        }
        let message = app.docs.error(doc).unwrap_or("Couldn't save");
        let mut actions = vec![];
        match issue {
            Some(ReadIssue::Newer) => actions.push(("Show file", Action::ShowFile(doc))),
            Some(ReadIssue::Malformed { .. }) => {
                actions.push(("Show file", Action::ShowFile(doc)));
                actions.push(("Start over", Action::StartOver(doc)));
            }
            Some(ReadIssue::Unavailable) | None => {
                actions.push(("Retry", Action::Panel(super::Command::RetryDoc(doc))));
                actions.push((
                    "Show folder",
                    Action::Panel(super::Command::ShowFolder(doc)),
                ));
            }
        }
        feedback(b, c, message, actions);
    }
}

/// A compact alert sentence with correctly sized recovery actions.
pub fn feedback(b: &mut B, c: &mut Col, message: &str, actions: Vec<(&str, Action)>) {
    let first_op = b.list.ops.len();
    let mut right = c.right();
    for (label, action) in actions.into_iter().rev() {
        let width = b.measure(label, &font(b, 13.0, 500)).0 + b.px(16.0);
        right -= width;
        button(
            b,
            Rect::new(right, c.y + b.px(4.0), width, b.px(28.0)),
            label,
            None,
            false,
            true,
            action,
        );
        right -= b.px(4.0);
    }
    b.icon(
        Rect::new(c.x, c.y + b.px(9.0), b.px(16.0), b.px(16.0)),
        mypaper_ui::icons::Icon::Warning,
        14.0,
        b.p.ink2,
    );
    let style = font(b, 11.0, 400);
    let style = mypaper_ui::TextStyle { size: style.size * 12.0 / 11.0, ..style };
    let lines = b
        .text
        .lines(message, &style, (right - c.x - b.px(21.0)).max(1.0), 2);
    let line = b.px(15.0) * b.t.text_scale;
    let h = (lines.len() as f32 * line + b.px(8.0)).max(b.px(36.0));
    for (i, label) in lines.iter().enumerate() {
        b.text_at(
            c.x + b.px(21.0),
            c.y + (h - lines.len() as f32 * line) / 2.0 + i as f32 * line,
            label,
            &style,
            b.p.ink2,
        );
    }
    c.y += h;
    super::saving::emphasize_feedback(b, first_op);
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
    b.group(key(("panel-group", c.view, heading, start as i32)), |b| {
        content(b, &mut inner)
    });
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
    let primary = primary && enabled;
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
    if !primary {
        b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
    }
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
            label: label.into(),
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
    crate::ui::anchor(app, id, c.view, c.y);
    let value = app.value(s);
    if id == "font" {
        crate::panel::pickers::font(
            app,
            b,
            c,
            Edit::new(s.doc, s.path, s.label),
            value.as_str().unwrap_or(""),
        );
        return;
    }

    let edit = if let Some(field) = s.path.strip_prefix("/look/windows/") {
        Edit::new(
            crate::docs::DocId::Studio,
            format!("/panel/windows/{field}"),
            s.label,
        )
    } else {
        Edit::new(s.doc, s.path, s.label)
    };
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
    let label_w = match s.kind {
        Kind::Switch { .. } => c.w - b.px(48.0),
        Kind::Text { .. } => c.w,
        _ if c.w < b.px(360.0) || b.t.text_scale > 1.0 => c.w,
        _ => label_width(b, c),
    };
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
            let stacked = c.w < b.px(360.0) || b.t.text_scale > 1.0;
            let width = if stacked {
                c.w
            } else {
                b.px(164.0).min(c.w - label_w - b.px(8.0))
            };
            let control = Rect::new(
                r.right - width,
                if stacked {
                    c.y + height(b, if caption.is_empty() { 28.0 } else { 52.0 })
                } else {
                    c.y + (h - b.px(28.0)) / 2.0
                },
                width,
                height(b, 28.0),
            );
            let k = key(("panel-setting", id));
            b.fill(control, b.px(6.0), b.p.fill);
            b.stroke(control, b.px(6.0), b.px(1.0), b.p.edge);
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
            c.y = if stacked {
                control.bottom + b.px(8.0)
            } else {
                r.bottom
            };
        }
        Kind::Choice { options, .. } => {
            let mut options: Vec<_> = options.iter().collect();
            options.sort_by_key(|(_, label)| *label != "As is");
            // Keep three values on one line at 480; stack within the plate at laptop widths
            // or increased Windows text size rather than truncating any of the choices.
            let style = font(b, 12.0, 500);
            let cell = options
                .iter()
                .map(|(_, l)| b.measure(l, &style).0 + b.px(16.0))
                .fold(0.0, f32::max);
            let cell = if matches!(id, "save_power" | "reduce_motion") {
                cell.max(b.measure("Like Windows", &style).0 + b.px(16.0))
            } else {
                cell
            };
            let required = cell * options.len() as f32;
            let stacked =
                c.w < b.px(360.0) || b.t.text_scale > 1.0 || c.w - label_w - b.px(8.0) < required;
            let vertical = required > c.w;
            let control = if stacked {
                c.y += height(b, 30.0);
                Rect::new(
                    c.x,
                    c.y,
                    c.w,
                    height(b, 30.0) * if vertical { options.len() as f32 } else { 1.0 },
                )
            } else {
                Rect::new(
                    r.right - required,
                    c.y + (h - height(b, 28.0)) / 2.0,
                    required,
                    height(b, 28.0),
                )
            };
            b.fill(control, b.px(7.0), b.p.fill);
            b.stroke(control, b.px(7.0), b.px(1.0), b.p.edge);
            let sw = if vertical {
                control.width()
            } else {
                control.width() / options.len() as f32
            };
            b.group(key(("panel-segment", id)), |b| {
                for (i, (v, label)) in options.iter().enumerate() {
                    let v: Value = serde_json::from_str(v).expect("setting value");
                    let on = v == value;
                    let r = Rect::new(
                        control.left + if vertical { 0.0 } else { sw * i as f32 } + b.px(2.0),
                        control.top
                            + if vertical {
                                height(b, 30.0) * i as f32
                            } else {
                                0.0
                            }
                            + b.px(2.0),
                        sw - b.px(4.0),
                        if vertical {
                            height(b, 30.0)
                        } else {
                            control.height()
                        } - b.px(4.0),
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
        Kind::Slider {
            min,
            max,
            step,
            unit,
            factor,
            default,
        } => {
            let value = value.as_f64().unwrap_or(default);
            let stacked = c.w < b.px(360.0) || b.t.text_scale > 1.0;
            let control = if stacked {
                Rect::new(c.x, c.y + height(b, 28.0), c.w, height(b, 36.0))
            } else {
                Rect::new(c.x + label_w + b.px(8.0), c.y, c.w - label_w - b.px(8.0), h)
            };
            b.slider(
                key(("panel-setting", id)),
                control,
                value,
                (min, max, step),
                &self::unit(value * factor, unit),
                b.px(48.0),
                label,
                Action::Slider(edit),
            );
            c.y = control.bottom + if stacked { b.px(4.0) } else { 0.0 };
        }
        Kind::Text { placeholder } => {
            let control = Rect::new(c.x, c.y + height(b, 28.0), c.w, height(b, 32.0));
            crate::ui::text_field(
                app,
                b,
                control,
                FieldId::Text(s.doc, s.path.into()),
                None,
                placeholder,
                value.as_str().unwrap_or(""),
                label,
            );
            b.stroke(control, b.px(6.0), b.px(1.0), b.p.edge);
            c.y = control.bottom + b.px(8.0);
        }
        Kind::Custom => c.y += h,
    }
    b.hline(c.x, c.right(), c.y - 1.0, b.p.line);
}

pub fn search(app: &App, b: &mut B, c: &mut Col) {
    let id = FieldId::Palette;
    let k = field_key(&id);
    let r = Rect::new(c.x, c.y, c.w, height(b, 32.0));
    b.fill(r, b.px(7.0), b.p.fill);
    b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
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
        Action::Field(id.clone()),
        Behavior::Field,
    );
    let value = app
        .field
        .as_ref()
        .filter(|field| field.id == id)
        .and_then(|field| field.field.text().ok())
        .unwrap_or_else(|| app.text(&id).to_owned());
    b.semantics(k, mypaper_ui::Semantics::Text(value.into()));
    b.focus_ring(k, r, b.px(7.0));
    c.y += r.height();
}

pub fn more(app: &App, b: &mut B, c: &mut Col, hint: &str) -> bool {
    c.y += b.px(8.0);
    let r = Rect::new(c.x, c.y, c.w, height(b, 36.0));
    let k = key(("panel-more", c.view));
    let open = app.is_open(k);
    let label_width = b.measure("More", &font(b, 13.0, 500)).0 + b.px(12.0);
    text(b, r, "More", 13.0, 500, b.p.ink2, Align::Start);
    text(
        b,
        Rect::new(
            r.left + label_width,
            r.top,
            r.width() - label_width - b.px(28.0),
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

/// Give reused kit controls the same visible boundary as the panel's native controls.
/// Run inside the body's scroll clip, after its controls have registered their geometry.
pub fn boundaries(b: &mut B) {
    let mut segments: std::collections::HashMap<_, (Rect, usize, bool)> =
        std::collections::HashMap::new();
    for target in b.targets.clone() {
        match target.behavior {
            Behavior::Slider {
                track,
                min,
                max,
                value,
                ..
            } => {
                b.stroke(track, b.px(2.0), b.px(1.0), b.p.edge);
                let t = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
                let knob = b.px(18.0).round();
                let r = Rect::new(
                    (track.left + track.width() * t - knob / 2.0).round(),
                    (track.top + track.height() / 2.0 - knob / 2.0).round(),
                    knob,
                    knob,
                );
                b.fill(r, knob / 2.0, b.kit.color.knob);
                b.stroke(r, knob / 2.0, b.px(1.0), b.p.edge);
            }
            Behavior::Toggle => b.stroke(
                target.rect.inflate(-b.px(4.0)),
                b.px(12.0),
                b.px(1.0),
                b.p.edge,
            ),
            Behavior::Field => b.stroke(target.rect, b.px(6.0), b.px(1.0), b.p.edge),
            _ => match target.action {
                Action::Choose(_) if target.rect.height() <= b.px(36.0) => {
                    b.stroke(target.rect, b.px(6.0), b.px(1.0), b.p.edge)
                }
                Action::Set(_, _)
                    if target.rect.height() <= b.px(36.0) && target.rect.width() > b.px(48.0) =>
                {
                    if let Some(group) = target.group {
                        segments
                            .entry(group)
                            .and_modify(|(r, count, same_row)| {
                                *same_row &= (r.top - target.rect.top).abs() < 1.0
                                    && (r.bottom - target.rect.bottom).abs() < 1.0;
                                *count += 1;
                                r.left = r.left.min(target.rect.left);
                                r.top = r.top.min(target.rect.top);
                                r.right = r.right.max(target.rect.right);
                                r.bottom = r.bottom.max(target.rect.bottom);
                            })
                            .or_insert((target.rect, 1, true));
                    }
                }
                _ => {}
            },
        }
    }
    for (r, count, same_row) in segments.into_values() {
        if count > 1 && same_row {
            b.stroke(r.inflate(b.px(2.0)), b.px(7.0), b.px(1.0), b.p.edge);
        }
    }
}
