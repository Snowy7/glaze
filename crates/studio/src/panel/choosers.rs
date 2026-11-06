//! One readable specimen above name rows. Hover/focus is transient; only clicks commit.
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
    pointer_mode: bool,
    requested: Option<(Key, Action)>,
    pending: Option<(Key, Action, Instant)>,
    previewed: Option<Action>,
    suppressed: Option<Key>,
}

pub fn begin(app: &App) {
    app.panel.as_ref().unwrap().chooser.borrow_mut().requested = None;
}

/// Existing click actions also describe desktop previews, including per-display choices.
pub fn draw(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    picture_height: f32,
    choices: &[(&str, bool, Action)],
    picture: impl FnOnce(&mut B, usize, Rect),
) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    let rows_top = c.y + picture_height + b.px(8.0);
    let row_height = height(b, 34.0);
    let row_rect = |index| Rect::new(c.x, rows_top + index as f32 * row_height, c.w, row_height);
    let pointer = if app.fixture || app.window.is_none() {
        b.input.pointer
    } else {
        super::pick_client::panel_pointer(app)
    }
    .filter(|point| b.clip_rect().is_none_or(|clip| clip.contains(*point)));
    let keyboard =
        b.input.focus_visible && !app.panel.as_ref().unwrap().chooser.borrow().pointer_mode;
    let hot = pointer
        .and_then(|point| (0..choices.len()).find(|index| row_rect(*index).contains(point)))
        .or_else(|| {
            if keyboard {
                (0..choices.len()).find(|index| {
                    b.input.focus == Some(key((id, *index)))
                        && b.clip_rect()
                            .is_none_or(|clip| row_rect(*index).intersects(&clip))
                })
            } else {
                None
            }
        });
    let shown = hot
        .or_else(|| choices.iter().position(|(_, selected, _)| *selected))
        .unwrap_or(0);
    picture(b, shown, Rect::new(c.x, c.y, c.w, picture_height));
    c.y = rows_top;
    b.group(id, |b| {
        for (index, (name, selected, action)) in choices.iter().enumerate() {
            let k = key((id, index));
            let rect = Rect::new(c.x, c.y, c.w, row_height);
            b.plate(k, rect, b.px(7.0), *selected);
            let mark = Rect::new(
                rect.left + b.px(10.0),
                rect.top + (rect.height() - b.px(7.0)) / 2.0,
                b.px(7.0),
                b.px(7.0),
            );
            b.fill(mark, b.px(if index % 2 == 0 { 3.5 } else { 1.0 }), b.p.ink3);
            text(
                b,
                Rect::new(
                    rect.left + b.px(27.0),
                    rect.top,
                    rect.width() - b.px(60.0),
                    rect.height(),
                ),
                name,
                13.0,
                if *selected { 600 } else { 400 },
                b.p.ink1,
                Align::Start,
            );
            if *selected {
                b.icon(
                    Rect::new(rect.right - b.px(30.0), rect.top, b.px(20.0), rect.height()),
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
                Some(*selected),
                action.clone(),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(7.0));
            c.y += row_height;
        }
    });
    c.y += b.px(6.0);
    if let Some(index) = hot {
        app.panel.as_ref().unwrap().chooser.borrow_mut().requested =
            Some((key((id, index)), choices[index].2.clone()));
    }
    Some(shown)
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
