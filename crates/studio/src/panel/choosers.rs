//! Picture cards. Hover/focus is transient; only clicks commit.
use crate::panel::tokens::radius;
use std::time::{Duration, Instant};

use super::controls::{height, text};
use crate::{
    actions::Action,
    app::App,
    ui::{B, Col},
};
use glaze_ui::{
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
    groups: std::collections::HashMap<Key, usize>,
    pointer_mode: bool,
    requested: Option<(Key, Action)>,
    pending: Option<(Key, Action, Instant)>,
    previewed: Option<Action>,
    suppressed: Option<Key>,
    rasters_left: Option<u8>,
    pub(super) drawings: std::collections::HashMap<String, super::specimens::Drawing>,
    drawing_live: std::collections::HashSet<String>,
    pub(super) image_live: std::collections::HashSet<String>,
    drawing_page: Option<super::Page>,
    pub(super) plugin_faces: Option<serde_json::Value>,
}

pub fn begin(app: &App) {
    app.images.borrow_mut().begin_frame();
    let panel = app.panel.as_ref().unwrap();
    let mut state = panel.chooser.borrow_mut();
    state.requested = None;
    state.groups.clear();
    state.drawing_live.clear();
    state.image_live.clear();
    if state.drawing_page.replace(panel.page) != Some(panel.page) {
        state.drawings.clear();
        state.plugin_faces = None;
    }
}

pub fn presented(app: &App) {
    let panel = app.panel.as_ref().unwrap();
    if app.images.borrow_mut().finish_frame() {
        panel.reclaim_heap.set(true);
    }
    let mut state = panel.chooser.borrow_mut();
    state.rasters_left = None;
    let before = state.drawings.len();
    let State {
        drawings,
        drawing_live,
        ..
    } = &mut *state;
    drawings.retain(|key, _| drawing_live.contains(key));
    if drawings.len() != before {
        panel.reclaim_heap.set(true);
    }
    let mut images = panel.specimens.borrow_mut();
    let before = images.len();
    images.retain(|key, _| state.image_live.contains(key));
    if images.len() != before {
        panel.reclaim_heap.set(true);
    }
}

pub(super) fn drawing(app: &App, key: &str) -> Option<super::specimens::Drawing> {
    let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
    state.drawing_live.insert(key.to_owned());
    state.drawings.get(key).cloned()
}

pub(super) fn remember_drawing(
    app: &App,
    key: String,
    drawing: super::specimens::Drawing,
    identity: Option<&str>,
) {
    let panel = app.panel.as_ref().unwrap();
    let mut state = panel.chooser.borrow_mut();
    if let Some(identity) = identity {
        state.drawings.retain(|key, _| !key.starts_with(identity));
    }
    if state.drawings.len() >= 24 {
        state.drawings.clear();
    }
    state.drawing_live.insert(key.clone());
    state.drawings.insert(key, drawing);
    panel.reclaim_heap.set(true);
}

/// Limit new picture jobs per presented frame; the next pair follows on an idle frame.
pub fn claim_raster(app: &App) -> bool {
    if app.fixture
        || !matches!(
            app.panel.as_ref().unwrap().page,
            super::Page::AddWidget | super::Page::Widget | super::Page::BarDesign
        )
    {
        return true;
    }
    let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
    let remaining = state.rasters_left.get_or_insert(2);
    if *remaining == 0 {
        if let Some(window) = app.window {
            // Separate from hover debounce: finishing a chooser must not cancel gallery work.
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    Some(window),
                    0x6A35,
                    32,
                    Some(wake),
                );
            }
        }
        return false;
    }
    *remaining -= 1;
    true
}

/// Each choice has a readable picture; hover/focus previews on the real desktop.
/// Keep the height argument for existing callers; cards share a 16:9 picture area.
pub fn draw(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    _picture_height: f32,
    choices: &[(&str, bool, Action)],
    picture: impl FnMut(&mut B, Option<usize>, Rect),
) -> Option<usize> {
    cards(app, b, c, id, None, choices, picture)
}

/// Compact material cards reserve a permanent caption, so hover changes no layout.
pub fn captioned(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    choices: &[(&str, bool, Action)],
    captions: &[&str],
    picture: impl FnMut(&mut B, Option<usize>, Rect),
) -> Option<usize> {
    cards(app, b, c, id, Some(captions), choices, picture)
}

/// History rows share the cards' pointer/focus arbitration and preview debounce.
pub fn preview_row(app: &App, b: &B, id: Key, rect: Rect, action: Action) {
    let state = app.panel.as_ref().unwrap().chooser.borrow();
    let keyboard = b.input.focus_visible && !state.pointer_mode;
    let pointer = if app.fixture || app.window.is_none() {
        b.input.pointer
    } else {
        super::pick_client::panel_pointer(app)
    };
    let visible = b.clip_rect().map_or(rect, |clip| rect.intersect(clip));
    let hot = if keyboard {
        b.input.focus == Some(id) && visible.width() > 0.0 && visible.height() > 0.0
    } else {
        pointer.is_some_and(|point| visible.contains(point))
    };
    drop(state);
    if hot {
        app.panel.as_ref().unwrap().chooser.borrow_mut().requested = Some((id, action));
    }
}

/// Track an immediate row preview so leaving it cancels just like a debounced hover.
pub fn track_preview(app: &App, id: Key, action: Action) {
    let mut state = app.panel.as_ref().unwrap().chooser.borrow_mut();
    state.requested = Some((id, action.clone()));
    state.pending = Some((id, action.clone(), Instant::now()));
    state.previewed = Some(action);
    state.suppressed = None;
}

fn cards(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    captions: Option<&[&str]>,
    choices: &[(&str, bool, Action)],
    mut picture: impl FnMut(&mut B, Option<usize>, Rect),
) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    let columns = if b.t.text_scale > 1.0 || c.w < b.px(252.0) {
        1
    } else if c.w >= b.px(500.0) {
        3
    } else {
        2
    };
    app.panel
        .as_ref()
        .unwrap()
        .chooser
        .borrow_mut()
        .groups
        .insert(id, columns);
    let selected = choices.iter().position(|(_, selected, _)| *selected);
    let count = choices.len() + usize::from(selected.is_none());
    let gap = b.px(10.0);
    let width = (c.w - (columns - 1) as f32 * gap) / columns as f32;
    let picture_height = if captions.is_some() {
        (width * 10.0 / 16.0).max(b.px(72.0))
    } else {
        width * 9.0 / 16.0
    };
    let title_font = super::controls::font(b, 13.0, 600);
    let label_lines = choices
        .iter()
        .map(|(name, _, _)| b.text.lines(name, &title_font, width - b.px(36.0), 2).len())
        .max()
        .unwrap_or(1)
        .max(1);
    let label_height = height(b, 22.0 * label_lines as f32);
    let caption_font = super::controls::font(b, 11.0, 400);
    let caption_lines = captions.map_or(1, |captions| {
        captions
            .iter()
            .map(|caption| {
                b.text
                    .lines(caption, &caption_font, width - b.px(16.0), 2)
                    .len()
            })
            .max()
            .unwrap_or(1)
            .max(1)
    });
    // Reserve the preview caption so hovering never moves another card.
    let row_height = picture_height
        + b.px(12.0)
        + label_height
        + height(b, 17.0 * caption_lines as f32)
        + b.px(8.0)
        + gap;
    let top = c.y;
    let row_rect = |index: usize| {
        Rect::new(
            c.x + (index % columns) as f32 * (width + gap),
            top + (index / columns) as f32 * row_height,
            width,
            row_height - gap,
        )
    };
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
        pointer.and_then(|point| (0..count).find(|index| row_rect(*index).contains(point)))
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
            let area = rect;
            let hovered = highlighted == Some(index);
            // Bound every specimen to its picture; captions own the space below it.
            let pic = Rect::new(rect.left, rect.top, width, picture_height);
            if b.clip_rect().is_none_or(|clip| rect.intersects(&clip)) {
                super::controls::card_picture(b, pic, hovered, |b, pic| {
                    picture(b, (index < choices.len()).then_some(index), pic);
                });
                let label = Rect::new(
                    rect.left,
                    super::controls::card_label_top(b, pic),
                    width - b.px(28.0),
                    label_height,
                );
                for (line, value) in b
                    .text
                    .lines(name, &title_font, label.width(), 2)
                    .iter()
                    .enumerate()
                {
                    text(
                        b,
                        Rect::new(
                            label.left,
                            label.top + line as f32 * height(b, 22.0),
                            label.width(),
                            height(b, 22.0),
                        ),
                        value,
                        13.0,
                        if *applied { 600 } else { 400 },
                        b.p.ink1,
                        Align::Start,
                    );
                }
                if let Some(caption) = captions.and_then(|captions| captions.get(index)) {
                    let lines = b.text.lines(caption, &caption_font, width - b.px(16.0), 2);
                    for (line, value) in lines.iter().enumerate() {
                        text(
                            b,
                            Rect::new(
                                label.left,
                                label.bottom + line as f32 * height(b, 17.0),
                                width - b.px(16.0),
                                height(b, 17.0),
                            ),
                            value,
                            11.0,
                            400,
                            b.p.ink2,
                            Align::Start,
                        );
                    }
                }
                if hovered && index < choices.len() {
                    if captions.is_none() {
                        text(
                            b,
                            Rect::new(
                                label.left,
                                label.bottom,
                                width - b.px(16.0),
                                height(b, 18.0),
                            ),
                            if width < b.px(175.0) || b.t.text_scale > 1.0 {
                                "Previewing"
                            } else {
                                "Previewing · Apply"
                            },
                            11.0,
                            400,
                            b.p.ink2,
                            Align::Start,
                        );
                    }
                    b.announce(&format!("Previewing {name} · Click to apply"), id);
                }
            }
            if *applied {
                super::controls::card_selection(b, pic);
                super::controls::applied_badge(b, pic);
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
            b.focus_ring(k, area, b.px(radius::CARD));
        }
    });
    c.y = top + count.div_ceil(columns) as f32 * row_height + b.px(6.0);
    if let Some(index) = hot.filter(|index| *index < choices.len()) {
        app.panel.as_ref().unwrap().chooser.borrow_mut().requested =
            Some((key((id, index)), choices[index].2.clone()));
    }
    shown
}

/// A widget's current choice stays compact; its catalog expands in place and folds on pick.
pub fn current(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    label: &str,
    choices: &[(&str, bool, Action)],
    mut picture: impl FnMut(&mut B, Option<usize>, Rect),
) {
    let selected = choices.iter().position(|(_, selected, _)| *selected);
    let name = selected.map_or("Custom", |index| choices[index].0);
    let expanded = app.is_open(id);
    let h = height(b, 68.0);
    let rect = Rect::new(c.x, c.y, c.w, h);
    let picture_width = (c.w * 0.37).min(b.px(140.0));
    picture(
        b,
        selected,
        Rect::new(c.x, c.y + b.px(4.0), picture_width, h - b.px(8.0)),
    );
    let x = c.x + picture_width + b.px(12.0);
    text(
        b,
        Rect::new(x, c.y + (h - height(b, 22.0)) / 2.0, c.right() - x - b.px(110.0), height(b, 22.0)),
        &format!("{label} · {name}"),
        13.0,
        500,
        b.p.ink1,
        Align::Start,
    );
    // The same trailing secondary button as other Change actions.
    let label_w = b.measure(if expanded { "Done" } else { "Change" }, &super::controls::font(b, 13.0, 500)).0 + b.px(28.0);
    let change = Rect::new(c.right() - label_w, c.y + (h - height(b, 32.0)) / 2.0, label_w, height(b, 32.0));
    super::controls::button(
        b,
        change,
        if expanded { "Done" } else { "Change" },
        None,
        false,
        true,
        Action::More(id),
    );
    // Read aloud (and found by search) as what it changes, not just "Change".
    if let Some(hit) = b.list.hits.last_mut() {
        hit.label = format!("Change {label}: {name}").into();
    }
    c.y = rect.bottom + b.px(6.0);
    if expanded {
        let start = b.targets.len();
        draw(app, b, c, id, b.px(112.0), choices, picture);
        for target in &mut b.targets[start..] {
            if target.action != Action::None {
                target.action = Action::Panel(super::Command::ApplyChoice(
                    id,
                    Box::new(target.action.clone()),
                ));
            }
        }
    }
}

/// Starter desktops use a single column when larger type needs room for the contents line.
pub fn tiles(
    app: &App,
    b: &mut B,
    c: &mut Col,
    id: Key,
    choices: &[(&str, bool, Action)],
    details: &[String],
    mut picture: impl FnMut(&mut B, usize, Rect),
) {
    let columns = if c.w < b.px(340.0) || b.t.text_scale > 1.0 {
        1
    } else {
        2
    };
    app.panel
        .as_ref()
        .unwrap()
        .chooser
        .borrow_mut()
        .groups
        .insert(id, columns);
    let gap = b.px(10.0);
    let width = (c.w - (columns - 1) as f32 * gap) / columns as f32;
    let picture_height = width * 9.0 / 16.0;
    let row_height = picture_height + height(b, 50.0) + gap;
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
                c.x + (i % columns) as f32 * (width + gap),
                c.y + (i / columns) as f32 * row_height,
                width,
                row_height - gap,
            );
            let hot = if keyboard {
                b.input.focus == Some(k)
            } else {
                pointer.is_some_and(|point| rect.contains(point))
            };
            let hot = hot && app.panel.as_ref().unwrap().chooser.borrow().suppressed != Some(k);
            let pic = Rect::new(rect.left, rect.top, width, picture_height);
            super::controls::card_picture(b, pic, hot, |b, pic| picture(b, i, pic));
            let top = super::controls::card_label_top(b, pic);
            if let Some(detail) = details.get(i) {
                text(
                    b,
                    Rect::new(
                        rect.left,
                        top + height(b, 22.0),
                        width,
                        height(b, 18.0),
                    ),
                    detail,
                    10.0,
                    400,
                    b.p.ink2,
                    Align::Start,
                );
            }
            text(
                b,
                Rect::new(rect.left, top, width - b.px(24.0), height(b, 22.0)),
                name,
                13.0,
                500,
                b.p.ink1,
                Align::Start,
            );
            if *applied {
                super::controls::card_selection(b, pic);
                super::controls::applied_badge(b, pic);
            }
            b.hit_checked(
                k,
                rect,
                Role::RadioButton,
                &details
                    .get(i)
                    .map_or_else(|| (*name).to_owned(), |detail| format!("{name}: {detail}")),
                Some(*applied),
                action.clone(),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(radius::CARD));
            if hot {
                app.panel.as_ref().unwrap().chooser.borrow_mut().requested =
                    Some((k, action.clone()));
            }
        }
    });
    c.y += choices.len().div_ceil(columns) as f32 * row_height;
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
        .filter(|group| panel.chooser.borrow().groups.contains_key(group))
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
    let step = if matches!(vk, 0x26 | 0x28) {
        panel.chooser.borrow().groups[&group] as isize
    } else {
        1
    };
    let next = (at as isize
        + if matches!(vk, 0x25 | 0x26) {
            -step
        } else {
            step
        })
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
        let pending = state.pending.take().is_some();
        let previewed = state.previewed.take().is_some();
        if pending || previewed {
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
    glaze_ui::queue_event(window, Event::Frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docs::DocId,
        panel::{Page, State as PanelState},
    };
    use glaze_config::look::Style;
    use glaze_ui::{Point, Ui};

    #[test]
    fn gallery_drawings_are_released_when_rows_or_the_page_leave() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::AddWidget,
            ..Default::default()
        });
        begin(&app);
        remember_drawing(
            &app,
            "face:clock".into(),
            super::super::specimens::Drawing {
                size: (200.0, 100.0),
                ops: std::sync::Arc::from([]),
            },
            None,
        );
        presented(&app);
        assert_eq!(
            app.panel.as_ref().unwrap().chooser.borrow().drawings.len(),
            1
        );
        begin(&app);
        presented(&app);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .drawings
                .is_empty()
        );
        remember_drawing(
            &app,
            "face:clock".into(),
            super::super::specimens::Drawing {
                size: (200.0, 100.0),
                ops: std::sync::Arc::from([]),
            },
            None,
        );
        app.panel.as_mut().unwrap().page = Page::Root;
        begin(&app);
        assert!(
            app.panel
                .as_ref()
                .unwrap()
                .chooser
                .borrow()
                .drawings
                .is_empty()
        );
    }

    #[test]
    fn auto_height_rebuild_does_not_restart_the_raster_budget_before_presenting() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.fixture = false;
        for page in [Page::AddWidget, Page::Widget, Page::BarDesign] {
            app.panel = Some(PanelState {
                page,
                ..Default::default()
            });
            begin(&app);
            assert!(claim_raster(&app));
            assert!(claim_raster(&app));
            begin(&app); // The same frame is rebuilt after fitting the window's height.
            assert!(!claim_raster(&app));
            presented(&app);
            assert!(claim_raster(&app));
        }
    }

    #[test]
    fn window_history_hover_and_focus_preview_without_selecting_or_saving() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::window_app(true, &ui, (1280, 800));
        super::super::history::observe(&app);
        app.run(Action::SetStyle(Some(Style::Solid)));
        app.run(Action::Panel(super::super::Command::Open(Page::Look)));
        app.run(Action::SetStyle(Some(Style::Mono)));
        app.run(Action::Panel(super::super::Command::Open(Page::History)));
        let before = DocId::ALL.map(|id| app.docs.doc(id).raw.clone());
        let undo = app.docs.undo_count();
        let frame = crate::view::build(&app, &mut ui);
        let target = frame.targets.iter().find(|target| {
            target.action == Action::Panel(super::super::Command::History(1))
        }).unwrap();
        let target_key = target.key;
        app.input.pointer = Some(Point::new(target.rect.left + 20.0, target.rect.top + 20.0));
        crate::view::build(&app, &mut ui);
        let requested = || app.panel.as_ref().unwrap().chooser.borrow().pending
            .as_ref().map(|(_, action, _)| action.clone());
        assert_eq!(requested(), Some(Action::Panel(super::super::Command::History(1))));
        assert_eq!(app.panel.as_ref().unwrap().history.borrow().selected, None);
        app.event(Event::Leave);
        crate::view::build(&app, &mut ui);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().pending.is_none());
        app.frame.targets = frame.targets;
        let id = app.frame.targets.iter().position(|target| target.key == target_key).unwrap() as u32 + 1;
        app.event(Event::Focus(id));
        crate::view::build(&app, &mut ui);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().pending.is_some());
        for event in [Event::Deactivated, Event::Cancelled, Event::Close] {
            app.event(event);
            crate::view::build(&app, &mut ui);
            assert!(app.panel.as_ref().unwrap().chooser.borrow().pending.is_none());
        }
        app.input.focus = None;
        super::super::history::select(&app, 1);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().previewed.is_some());
        crate::view::build(&app, &mut ui);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().previewed.is_none());
        crate::view::build(&app, &mut ui);
        assert!(app.panel.as_ref().unwrap().chooser.borrow().pending.is_none(),
            "repainting a selected History row must not restart its preview after list leave");
        assert_eq!(app.panel.as_ref().unwrap().history.borrow().selected, Some(1));
        assert_eq!(before, DocId::ALL.map(|id| app.docs.doc(id).raw.clone()));
        assert_eq!(app.docs.undo_count(), undo);
    }

    #[test]
    fn style_hover_preserves_applied_controls_and_their_positions() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(PanelState {
            page: Page::Look,
            ..Default::default()
        });
        app.size = (480, 1000);
        let before = crate::panel::build(&app, &mut ui.text);
        let controls: Vec<_> = before.targets.iter().filter(|target| matches!(&target.action,
            Action::Slider(edit) | Action::Set(edit, _) | Action::Choose(crate::actions::Choices { edit, .. })
                if edit.path.starts_with("/look/material/") || edit.path.as_ref() == "/look/appearance"))
            .map(|target| (target.key, target.rect)).collect();
        assert!(!controls.is_empty());
        let action = Action::SetStyle(Some(Style::Fluent));
        let target = before
            .targets
            .iter()
            .find(|target| target.action == action)
            .unwrap();
        app.input.pointer = Some(Point::new(target.rect.left + 20.0, target.rect.top + 10.0));
        let during = crate::panel::build(&app, &mut ui.text);
        for (key, rect) in controls {
            let target = during
                .targets
                .iter()
                .find(|target| target.key == key)
                .unwrap();
            assert_eq!(target.rect, rect);
        }
        assert!(during.list.ops.iter().any(|op| matches!(op,
            glaze_ui::Op::Text { text, .. } if text.as_ref() == "Liquid settings")));
        assert_eq!(app.docs.undo_count(), 0);
        app.run(action);
        let applied = crate::panel::build(&app, &mut ui.text);
        assert!(applied.list.ops.iter().any(|op| matches!(op,
            glaze_ui::Op::Text { text, .. } if text.as_ref() == "Fluent settings")));
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
        // The hover wash is drawn inside the picture's clip.
        fn flat(ops: &[glaze_ui::Op]) -> Vec<&glaze_ui::Op> {
            ops.iter()
                .flat_map(|op| match op {
                    glaze_ui::Op::Clipped { ops, .. } | glaze_ui::Op::RoundedClipped { ops, .. } => flat(ops),
                    _ => vec![op],
                })
                .collect()
        }
        assert!(flat(&frame.list.ops).into_iter().any(
            |op| matches!(op, glaze_ui::Op::Fill { rect: drawn, color, .. }
            if drawn.contains(app.input.pointer.unwrap())
                && drawn.top >= rect.top && drawn.bottom <= rect.bottom
                && glaze_ui::theme::luminance(*color)
                    > glaze_ui::theme::luminance(crate::panel::palette_for(&app).fill))
        ));
        assert!(frame.semantics.iter().any(
            |(_, semantic)| matches!(semantic, glaze_ui::Semantics::Status(message)
            if message.as_ref() == "Previewing Fluent · Click to apply")
        ));
        assert!(crate::panel::on_key(&mut app, 0x1B, (false, false, false)));
        let frame = crate::panel::build(&app, &mut ui.text);
        assert!(!frame.semantics.iter().any(
            |(_, semantic)| matches!(semantic, glaze_ui::Semantics::Status(message)
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
            |(_, semantic)| matches!(semantic, glaze_ui::Semantics::Status(message)
            if message.as_ref() == "Previewing Solid · Click to apply")
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
        for name in ["As is", "Match Glaze", "Glass", "Sharp"] {
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
            &Event::Move(glaze_ui::Pointer {
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
