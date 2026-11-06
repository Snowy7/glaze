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

#[derive(Debug, Default)]
pub struct State {
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
    let hot = choices.iter().enumerate().find_map(|(index, _)| {
        let rect = Rect::new(c.x, rows_top + index as f32 * row_height, c.w, row_height);
        let k = key((id, index));
        let visible = b.clip_rect().is_none_or(|clip| rect.intersects(&clip));
        (visible
            && (b.input.pointer.is_some_and(|point| {
                rect.contains(point) && b.clip_rect().is_none_or(|clip| clip.contains(point))
            }) || b.input.focus_visible && b.input.focus == Some(k)))
        .then_some(index)
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
