//! A single color control shared by native editors. The owner supplies persistence and picker actions.
use crate::controls::{Align, Behavior, Builder, Font, Key, key};
use crate::{Rect, Rgba, Role};

pub mod popover;

pub struct Choice<A> {
    pub label: String,
    pub color: Option<Rgba>,
    pub selected: bool,
    pub action: A,
}

/// Larger Windows text gives the two worded choices their own line above the swatches.
pub fn height<A: Clone>(b: &Builder<'_, A>) -> f32 {
    if b.t.text_scale > 1.0 {
        b.px((14.0 * b.t.text_scale + 10.0).max(26.0) + 44.0)
    } else {
        b.px(34.0)
    }
}

/// Choices are ordered As is, wallpaper, presets, custom. The custom dot opens the owner's picker.
pub fn draw<A: Clone>(b: &mut Builder<'_, A>, id: Key, rect: Rect, choices: &[Choice<A>]) {
    let compact = rect.width() < b.px(340.0);
    let stacked = b.t.text_scale > 1.0;
    let gap = b.px(if compact { 4.0 } else { 5.0 });
    let first = b.px(if stacked {
        64.0
    } else if compact {
        36.0
    } else {
        42.0
    });
    let wallpaper = if stacked {
        rect.width() - first - gap
    } else {
        b.px(if compact { 108.0 } else { 116.0 })
    };
    let labeled = first + wallpaper;
    let dots = choices.len().saturating_sub(2).max(1) as f32;
    let diameter =
        ((rect.width() - if stacked { 0.0 } else { labeled } - gap * (choices.len() - 1) as f32)
            / dots)
            .clamp(b.px(16.0), b.px(26.0));
    let words_height = b.px((14.0 * b.t.text_scale + 10.0).max(26.0));
    let mut x = rect.left;
    let mut font = b.font(Font::Caption);
    font.size = b.px(10.0) * b.t.text_scale;
    b.group(id, |b| {
        for (i, choice) in choices.iter().enumerate() {
            if stacked && i == 2 {
                x = rect.left;
            }
            let width = match i {
                0 => first,
                1 => wallpaper,
                _ => diameter,
            };
            let r = Rect::new(
                x,
                if stacked {
                    rect.top
                        + if i < 2 {
                            0.0
                        } else {
                            words_height + b.px(12.0)
                        }
                } else {
                    rect.top + (rect.height() - diameter) / 2.0
                },
                width,
                if stacked && i < 2 {
                    words_height
                } else {
                    diameter
                },
            );
            let k = key((id, i));
            let radius = diameter / 2.0;
            if i < 2 {
                b.fill(r, b.px(5.0), b.p.fill);
                b.stroke(r, b.px(5.0), b.px(1.0), b.p.edge);
                let label_rect = if i == 1 {
                    let dot = Rect::new(
                        r.left + b.px(4.0),
                        r.top + (r.height() - diameter + b.px(6.0)) / 2.0,
                        diameter - b.px(6.0),
                        diameter - b.px(6.0),
                    );
                    b.fill(dot, radius, choice.color.unwrap_or(b.p.fill));
                    for offset in [0.0, 4.0] {
                        b.stroke(
                            Rect::new(
                                dot.left + b.px(3.0 + offset),
                                dot.top + b.px(6.0),
                                b.px(6.0),
                                b.px(4.0),
                            ),
                            b.px(2.0),
                            b.px(1.0),
                            b.p.ink1,
                        );
                    }
                    Rect::new(
                        dot.right + b.px(3.0),
                        r.top,
                        r.right - dot.right - b.px(5.0),
                        r.height(),
                    )
                } else {
                    r
                };
                b.text_in(label_rect, &choice.label, &font, b.p.ink2, Align::Center);
            } else {
                b.fill(r, radius, choice.color.unwrap_or(b.p.fill));
                b.stroke(r, radius, b.px(1.0), b.p.edge);
                if i == choices.len() - 1 {
                    b.icon(r, crate::icons::Icon::Palette, 14.0, b.p.ink1);
                }
            }
            if choice.selected {
                b.stroke(
                    r.inflate(b.px(2.0)),
                    if i < 2 { b.px(7.0) } else { radius + b.px(2.0) },
                    b.px(1.5),
                    b.p.ink1,
                );
            }
            b.hit_checked(
                k,
                r,
                Role::RadioButton,
                &choice.label,
                Some(choice.selected),
                choice.action.clone(),
                Behavior::Click,
            );
            b.focus_ring(k, r, radius);
            x += width + gap;
        }
    });
}
