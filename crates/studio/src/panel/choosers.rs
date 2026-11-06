//! Compact picture rows. Hover/focus is transient; only clicks commit.
use std::time::{Duration, Instant};

use super::controls::{height, text};
use crate::{
    actions::Action,
    app::App,
    ui::{B, Col},
};
use mypaper_ui::{
    Event, Rect, Role,
    controls::{Align, Behavior, Key, key},
};

const DELAY: Duration = Duration::from_millis(150);
const TIMER: usize = 0x6A31;

#[cfg(test)]
#[path = "chooser_review.rs"]
mod review;

#[derive(Debug, Default)]
pub struct State {
    groups: std::collections::HashSet<Key>,
    pointer_mode: bool,
    requested: Option<(Key, Action)>,
    pending: Option<(Key, Action, Instant)>,
    previewed: Option<Action>,
    suppressed: Option<Key>,
}

pub fn begin(app: &App) {
    let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
    state.requested = None;
    state.groups.clear();
}

/// Each choice owns a thumbnail; hover/focus previews on the real desktop.
/// Keep the height argument for the shared bar-editor callers.
pub fn draw(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    _picture_height: f32,
    choices: &[(&str, bool, Action)],
    mut picture: impl FnMut(&mut B, Option<usize>, Rect),
) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    app.panel
        .as_ref()
        .unwrap()
        .chooser
        .borrow_mut()
        .groups
        .insert(id);
    let selected = choices.iter().position(|(_, selected, _)| *selected);
    let count = choices.len() + usize::from(selected.is_none());
    let text_x = c.x + b.px(90.0);
    let text_width = (c.right() - text_x - b.px(30.0)).max(b.px(40.0));
    let caption_font = super::controls::font(b, 11.0, 400);
    let caption_lines = choices
        .iter()
        .map(|(name, _, _)| {
            b.text
                .lines(
                    &format!("Previewing {name} \u{00b7} Click to apply"),
                    &caption_font,
                    text_width,
                    3,
                )
                .len()
        })
        .max()
        .unwrap_or(1)
        .max(1);
    // Reserve captions so hovering never moves the following row.
    let row_height = b
        .px(48.0)
        .max(height(b, 22.0 + 15.0 * caption_lines as f32) + b.px(10.0));
    let top = c.y;
    let row_rect = |index| Rect::new(c.x, top + index as f32 * row_height, c.w, row_height);
    let pointer = if app.fixture || app.window.is_none() {
        b.input.pointer
    } else {
        super::pick_client::panel_pointer(app)
    }
    .filter(|point| b.clip_rect().is_none_or(|clip| clip.contains(*point)));
    let keyboard =
        b.input.focus_visible && !app.panel.as_ref().unwrap().chooser.borrow().pointer_mode;
    let hot = if keyboard {
        (0..count).find(|index| {
            b.input.focus == Some(key((id, *index)))
                && b.clip_rect()
                    .is_none_or(|clip| row_rect(*index).intersects(&clip))
        })
    } else {
        pointer.and_then(|point| {
            (0..count).find(|index| b.list_row_area(row_rect(*index)).contains(point))
        })
    };
    let highlighted = hot.filter(|index| {
        app.panel.as_ref().unwrap().chooser.borrow().suppressed != Some(key((id, *index)))
    });
    let shown = highlighted
        .or(selected)
        .filter(|index| *index < choices.len());
    b.group(id, |b| {
        let custom = ("Custom", true, Action::None);
        for (index, (name, applied, action)) in choices
            .iter()
            .chain(selected.is_none().then_some(&custom))
            .enumerate()
        {
            let k = key((id, index));
            let rect = row_rect(index);
            let area = b.list_row_area(rect);
            let hovered = highlighted == Some(index);
            if hovered {
                b.fill(area, b.px(7.0), b.p.hover);
            } else if *applied {
                b.fill(area, b.px(7.0), b.p.selected);
            }
            if b.clip_rect().is_none_or(|clip| rect.intersects(&clip)) {
                picture(
                    b,
                    (index < choices.len()).then_some(index),
                    Rect::new(
                        rect.left + b.px(6.0),
                        rect.top + (rect.height() - b.px(44.0)) / 2.0,
                        b.px(72.0),
                        b.px(44.0),
                    ),
                );
                let label = Rect::new(
                    text_x,
                    rect.top
                        + if hovered {
                            b.px(5.0)
                        } else {
                            (rect.height() - height(b, 22.0)) / 2.0
                        },
                    text_width,
                    height(b, 22.0),
                );
                text(
                    b,
                    label,
                    name,
                    13.0,
                    if *applied { 600 } else { 400 },
                    b.p.ink1,
                    Align::Start,
                );
                if hovered && index < choices.len() {
                    let caption = format!("Previewing {name} \u{00b7} Click to apply");
                    for (line, value) in b
                        .text
                        .lines(&caption, &caption_font, text_width, 3)
                        .iter()
                        .enumerate()
                    {
                        text(
                            b,
                            Rect::new(
                                text_x,
                                label.bottom + line as f32 * height(b, 15.0),
                                text_width,
                                height(b, 15.0),
                            ),
                            value,
                            11.0,
                            400,
                            b.p.ink2,
                            Align::Start,
                        );
                    }
                    b.announce(&caption, id);
                }
                if *applied {
                    b.icon(
                        Rect::new(rect.right - b.px(26.0), rect.top, b.px(20.0), rect.height()),
                        crate::theme::glyph::CHECK,
                        13.0,
                        b.p.ink1,
                    );
                }
            }
            b.hit_checked(
                k,
                area,
                Role::RadioButton,
                name,
                Some(*applied),
                action.clone(),
                Behavior::Click,
            );
            b.focus_ring(k, area, b.px(7.0));
        }
    });
    c.y = top + count as f32 * row_height + b.px(6.0);
    if let Some(index) = hot.filter(|index| *index < choices.len()) {
        app.panel.as_ref().unwrap().chooser.borrow_mut().requested =
            Some((key((id, index)), choices[index].2.clone()));
    }
    shown
}

/// Two columns of miniature desktops for the four first-run starters.
pub fn tiles(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    choices: &[(&str, bool, Action)],
    mut picture: impl FnMut(&mut B, usize, Rect),
) {
    app.panel
        .as_ref()
        .unwrap()
        .chooser
        .borrow_mut()
        .groups
        .insert(id);
    let gap = b.px(10.0);
    let width = (c.w - gap) / 2.0;
    let picture_height = width * 9.0 / 16.0;
    let row_height = picture_height + height(b, 32.0) + gap;
    let pointer = if app.fixture || app.window.is_none() {
        b.input.pointer
    } else {
        super::pick_client::panel_pointer(app)
    }
    .filter(|point| b.clip_rect().is_none_or(|clip| clip.contains(*point)));
    let keyboard =
        b.input.focus_visible && !app.panel.as_ref().unwrap().chooser.borrow().pointer_mode;
    b.group(id, |b| {
        for (i, (name, applied, action)) in choices.iter().enumerate() {
            let k = key((id, i));
            let rect = Rect::new(
                c.x + (i % 2) as f32 * (width + gap),
                c.y + (i / 2) as f32 * row_height,
                width,
                row_height - gap,
            );
            let hot = if keyboard {
                b.input.focus == Some(k)
            } else {
                pointer.is_some_and(|point| rect.contains(point))
            };
            let hot = hot && app.panel.as_ref().unwrap().chooser.borrow().suppressed != Some(k);
            if *applied {
                b.fill(rect, b.px(7.0), b.p.selected);
            }
            if hot {
                b.fill(rect, b.px(7.0), b.p.hover);
            }
            picture(b, i, Rect::new(rect.left, rect.top, width, picture_height));
            text(
                b,
                Rect::new(
                    rect.left + b.px(8.0),
                    rect.top + picture_height,
                    width - b.px(32.0),
                    height(b, 32.0),
                ),
                name,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            if *applied {
                b.icon(
                    Rect::new(
                        rect.right - b.px(24.0),
                        rect.top + picture_height,
                        b.px(20.0),
                        height(b, 32.0),
                    ),
                    crate::theme::glyph::CHECK,
                    13.0,
                    b.p.ink1,
                );
            }
            b.hit_checked(
                k,
                rect,
                Role::RadioButton,
                name,
                Some(*applied),
                action.clone(),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(7.0));
            if hot {
                app.panel.as_ref().unwrap().chooser.borrow_mut().requested =
                    Some((k, action.clone()));
            }
        }
    });
    c.y += choices.len().div_ceil(2) as f32 * row_height;
}

/// Arrows browse a chooser; Enter/Space and clicks apply through the existing target action.
pub fn on_key(app: &mut App, vk: u16, _ctrl: bool, _shift: bool, alt: bool) -> bool {
    if alt
        || !matches!(vk, 0x25..=0x28)
        || app.field.is_some()
        || app
            .overlay
            .as_ref()
            .is_some_and(|overlay| !matches!(overlay, crate::app::Overlay::FirstRun { .. }))
    {
        return false;
    }
    let Some(panel) = &app.panel else {
        return false;
    };
    let Some(current) = app
        .input
        .focus
        .and_then(|key| app.frame.targets.iter().find(|target| target.key == key))
    else {
        return false;
    };
    let Some(group) = current
        .group
        .filter(|group| panel.chooser.borrow().groups.contains(group))
    else {
        return false;
    };
    let rows: Vec<_> = app
        .frame
        .targets
        .iter()
        .filter(|target| target.group == Some(group))
        .collect();
    let at = rows
        .iter()
        .position(|target| target.key == current.key)
        .unwrap();
    let next = (at as isize + if matches!(vk, 0x25 | 0x26) { -1 } else { 1 })
        .clamp(0, rows.len() as isize - 1) as usize;
    let target = rows[next];
    app.input.focus = Some(target.key);
    app.input.focus_visible = true;
    panel.chooser.borrow_mut().pointer_mode = false;
    if let Some(view) = target.scroll
        && let Some(area) = app.frame.scrolls.iter().find(|area| area.key == view)
    {
        let offset = app.nav.scroll(view);
        let pad = app.px(16.0);
        let y = if target.rect.top < area.viewport.top {
            Some((offset - area.viewport.top + target.rect.top - pad).max(0.0))
        } else if target.rect.bottom > area.viewport.bottom {
            Some(offset + target.rect.bottom - area.viewport.bottom + pad)
        } else {
            None
        };
        if let Some(y) = y {
            app.nav.scroll.insert(
                view,
                y.min((area.content - area.viewport.height()).max(0.0)),
            );
        }
    }
    app.dirty = true;
    true
}

pub fn finish(app: &App) {
    let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
    let request = state.requested.clone();
    if state.suppressed.is_some() && request.as_ref().map(|(key, _)| *key) != state.suppressed {
        state.suppressed = None;
    }
    let Some((key, action)) = request.filter(|(key, _)| Some(*key) != state.suppressed) else {
        if state.pending.take().is_some() || state.previewed.take().is_some() {
            super::pick_client::end_preview();
        }
        cancel_timer(app);
        return;
    };
    if state
        .pending
        .as_ref()
        .is_none_or(|(old, previous, _)| *old != key || *previous != action)
    {
        state.pending = Some((key, action, Instant::now()));
        schedule(app);
        return;
    }
    if state
        .pending
        .as_ref()
        .is_some_and(|(_, _, at)| at.elapsed() >= DELAY)
        && state.previewed.as_ref() != Some(&action)
    {
        super::pick_client::preview_action(app, &action);
        state.previewed = Some(action);
    }
}

pub fn end(app: &App) -> bool {
    let Some(panel) = app.panel.as_ref() else {
        return false;
    };
    let mut state = panel.chooser.borrow_mut();
    let active = state.pending.is_some() || state.previewed.is_some();
    state.suppressed = state.requested.as_ref().map(|(key, _)| *key);
    state.pending = None;
    state.previewed = None;
    cancel_timer(app);
    super::pick_client::end_preview();
    active
}

pub fn event(app: &mut App, event: &Event) {
    match event {
        Event::Move(_) => {
            let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
            if !state.pointer_mode && state.pending.is_some() {
                app.dirty = true;
            }
            state.pointer_mode = true;
        }
        Event::Key(_) | Event::Focus(_) => {
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow_mut()
                .pointer_mode = false
        }
        Event::Frame => app.dirty = true,
        Event::Leave | Event::Deactivated | Event::Close | Event::Cancelled => {
            end(app);
        }
        _ => {}
    }
}

fn cancel_timer(app: &App) {
    if let Some(window) = app.window {
        // SAFETY: this ID is reserved for our panel's own one-shot chooser timer.
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(window), TIMER) };
    }
}

fn schedule(app: &App) {
    if let Some(window) = app.window {
        // SAFETY: the timer targets our own HWND and queues an ordinary frame on its UI thread.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetTimer(
                Some(window),
                TIMER,
                DELAY.as_millis() as u32,
                Some(wake),
            );
        }
    }
}

unsafe extern "system" fn wake(
    window: windows::Win32::Foundation::HWND,
    _: u32,
    id: usize,
    _: u32,
) {
    // SAFETY: a callback for our own one-shot HWND timer, delivered on the owning UI thread.
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(window), id) };
    mypaper_ui::queue_event(window, Event::Frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docs::DocId,
        panel::{Page, State as PanelState},
    };
    use mypaper_config::look::Style;
    use mypaper_ui::{Point, Ui};

    #[test]
    fn style_hover_disables_material_and_color_until_leaving_or_applying() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 800);
        let frame = crate::panel::build(&app, &mut ui.text);
        let is_material = |action: &Action| matches!(action, Action::Slider(edit) if edit.path.starts_with("/look/material/"));
        assert!(
            frame
                .targets
                .iter()
                .any(|target| is_material(&target.action))
        );
        let tint = frame.targets.iter().filter(|target| matches!(&target.action,
            Action::Set(edit, _) | Action::Unset(edit) if edit.path.as_ref() == "/look/material/tint"))
            .map(|target| target.rect.bottom).reduce(f32::max).unwrap();
        assert!(
            tint < 752.0,
            "Style and Material fit on the first 480-DIP-wide screen: {tint}"
        );
        let action = Action::SetStyle(Some(Style::Fluent));
        let target = frame
            .targets
            .iter()
            .find(|target| target.action == action)
            .unwrap();
        app.input.pointer = Some(Point::new(target.rect.left + 20.0, target.rect.top + 10.0));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(
            !frame
                .targets
                .iter()
                .any(|target| is_material(&target.action))
        );
        assert!(!frame.targets.iter().any(|target| matches!(&target.action,
            Action::Set(edit, _) if edit.path.as_ref() == "/look/appearance")));
        assert!(
            frame
                .list
                .ops
                .iter()
                .any(|op| matches!(op, mypaper_ui::Op::Text { text, .. }
            if text.as_ref() == "Click Fluent to edit its material"))
        );
        app.input.pointer = None;
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(
            frame
                .targets
                .iter()
                .any(|target| is_material(&target.action))
        );
        app.run(action);
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(
            frame
                .targets
                .iter()
                .any(|target| is_material(&target.action))
        );
        assert!(
            frame
                .list
                .ops
                .iter()
                .any(|op| matches!(op, mypaper_ui::Op::Text { text, .. }
            if text.as_ref() == "Your Fluent settings"))
        );
    }

    #[test]
    fn stale_input_hover_does_not_split_row_highlight_from_the_preview() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 1200);
        let frame = crate::panel::build(&app, &mut ui.text);
        let target = frame
            .targets
            .iter()
            .find(|target| target.action == Action::SetStyle(Some(Style::Fluent)))
            .unwrap();
        let rect = target.rect;
        app.input.pointer = Some(Point::new(rect.left + 1.0, rect.top + 10.0));
        app.input.hover = None;
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(frame.list.ops.iter().any(
            |op| matches!(op, mypaper_ui::Op::Fill { rect: drawn, color, .. }
            if drawn.contains(app.input.pointer.unwrap())
                && drawn.top >= rect.top && drawn.bottom <= rect.bottom
                && *color == crate::panel::palette_for(&app).hover)
        ));
        assert!(frame.semantics.iter().any(
            |(_, semantic)| matches!(semantic, mypaper_ui::Semantics::Status(message)
            if message.as_ref() == "Previewing Fluent · Click to apply")
        ));
        assert!(crate::panel::on_key(&mut app, 0x1B, (false, false, false)));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(!frame.semantics.iter().any(
            |(_, semantic)| matches!(semantic, mypaper_ui::Semantics::Status(message)
            if message.starts_with("Previewing"))
        ));
    }

    #[test]
    fn arrows_preview_without_applying_and_keyboard_wins_over_a_stationary_pointer() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 1200);
        let frame = crate::panel::build(&app, &mut ui.text);
        let current = frame
            .targets
            .iter()
            .find(|target| target.action == Action::SetStyle(Some(Style::Liquid)))
            .unwrap();
        app.input.focus = Some(current.key);
        app.input.pointer = Some(Point::new(
            current.rect.left + 20.0,
            current.rect.top + 10.0,
        ));
        app.frame.targets = frame.targets;
        app.frame.scrolls = frame.scrolls;
        let before = app.docs.doc(DocId::Config).raw.clone();
        assert!(crate::panel::on_key(&mut app, 0x28, (false, false, false)));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert_eq!(app.docs.doc(DocId::Config).raw, before);
        assert_eq!(app.docs.undo_count(), 0);
        assert!(frame.semantics.iter().any(
            |(_, semantic)| matches!(semantic, mypaper_ui::Semantics::Status(message)
            if message.as_ref() == "Previewing One UI · Click to apply")
        ));
        app.key_with(0x28, (false, true, false));
        app.key_with(0x28, (true, false, false));
        assert_eq!(app.docs.doc(DocId::Config).raw, before);
        assert_eq!(
            app.docs.undo_count(),
            0,
            "modifiers never turn row focus into a pick"
        );
    }

    #[test]
    fn unmatched_windows_settings_have_one_checked_custom_row() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Windows,
            ..Default::default()
        });
        app.size = (480, 1200);
        app.run(Action::Set(
            crate::actions::Edit::new(DocId::Config, "/look/windows", "Windows look"),
            serde_json::json!({"caption":"#123456","corners":"square","apps":"dark"}),
        ));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert_eq!(
            frame
                .list
                .hits
                .iter()
                .filter(|hit| hit.label.as_ref() == "Custom" && hit.checked == Some(true))
                .count(),
            1
        );
        for name in ["As is", "Match mypaper", "Glass", "Sharp"] {
            assert!(
                frame
                    .list
                    .hits
                    .iter()
                    .any(|hit| hit.label.as_ref() == name && hit.checked == Some(false))
            );
        }
    }

    #[test]
    fn pointer_and_keyboard_choices_preview_without_editing_and_escape_restores() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 1200);
        let before: Vec<_> = DocId::ALL
            .into_iter()
            .map(|id| app.docs.doc(id).raw.clone())
            .collect();
        let frame = crate::panel::build(&app, &mut ui.text);
        let action = Action::SetStyle(Some(Style::Fluent));
        let target = frame
            .targets
            .iter()
            .find(|target| target.action == action)
            .unwrap();
        app.input.pointer = Some(Point::new(target.rect.left + 30.0, target.rect.top + 10.0));
        app.input.hover = Some(target.key);
        crate::panel::build(&app, &mut ui.text);
        {
            let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
            assert_eq!(
                state.pending.as_ref().map(|(_, action, _)| action),
                Some(&action)
            );
            assert!(state.previewed.is_none(), "stable hover must wait 150 ms");
            state.pending.as_mut().unwrap().2 = Instant::now() - DELAY;
        }
        finish(&app);
        assert_eq!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .previewed
                .as_ref(),
            Some(&action)
        );
        assert!(crate::panel::on_key(&mut app, 0x1B, (false, false, false)));
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Look);
        crate::panel::build(&app, &mut ui.text);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .pending
                .is_none(),
            "Esc suppresses the same hovered row until it is left"
        );
        app.input.pointer = None;
        app.input.focus = Some(target.key);
        app.input.focus_visible = true;
        app.panel.as_ref().unwrap().chooser.borrow_mut().suppressed = None;
        crate::panel::build(&app, &mut ui.text);
        assert_eq!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .pending
                .as_ref()
                .map(|(_, action, _)| action),
            Some(&action)
        );
        event(
            &mut app,
            &Event::Move(mypaper_ui::Pointer {
                at: Point::new(475.0, 20.0),
                screen: (0, 0),
            }),
        );
        app.input.pointer = Some(Point::new(475.0, 20.0));
        app.input.focus_visible = false;
        crate::panel::build(&app, &mut ui.text);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .pending
                .is_none()
        );
        assert_eq!(app.docs.undo_count(), 0);
        assert_eq!(
            before,
            DocId::ALL
                .into_iter()
                .map(|id| app.docs.doc(id).raw.clone())
                .collect::<Vec<_>>()
        );
    }
}
