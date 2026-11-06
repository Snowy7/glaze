//! The `scroll` view kind (docs/plugins.md, "Scroll"): a plugin's list in a clipped viewport
//! that scrolls in the shell with the kit's [`Scroll`] (touchpad deltas at once, notches glide,
//! an overlay scrollbar that fades). Plugins never see the wheel: [`wheel`] moves the pane
//! locally, and only `on_reach_end` goes back to them, once per growth of the content.
//!
//! A pane's state lives here by (widget instance, scroll id), not in the face, so its position
//! survives every view update. [`scope`] names the instance while its face draws.
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use mypaper_plugin_sdk::View;
use mypaper_ui::kit::Frame;
use mypaper_ui::{HitId, Point, Rect, Scroll};

/// One scroll view as last drawn.
struct Pane {
    scroll: Scroll,
    viewport: Rect,
    /// Device px per design px when drawn.
    scale: f32,
    on_end: Option<String>,
    /// The scroll range when `on_end` was last sent: sent again only once the content grows.
    sent_at: Option<f32>,
    /// Drawn in the current pass of its instance; the rest are dropped after it.
    drawn: bool,
}

type Key = (String, String);

thread_local! {
    static PANES: RefCell<HashMap<Key, Pane>> = RefCell::new(HashMap::new());
    static INSTANCE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Draws one widget's face (`draw`) with its scroll views keyed under `instance`; panes it no
/// longer shows are forgotten.
pub fn scope<R>(instance: &str, draw: impl FnOnce() -> R) -> R {
    INSTANCE.with_borrow_mut(|i| instance.clone_into(i));
    PANES.with_borrow_mut(|panes| panes.iter_mut().filter(|(k, _)| k.0 == instance).for_each(|(_, p)| p.drawn = false));
    let out = draw();
    PANES.with_borrow_mut(|panes| panes.retain(|k, p| k.0 != instance || p.drawn));
    INSTANCE.with_borrow_mut(String::clear);
    out
}

/// The content's natural height: a list or stack's rows all laid out, not cut to the viewport.
fn natural(f: &mut Frame<'_>, child: &View, width: f32) -> f32 {
    match child {
        View::List { children, gap } | View::Stack { children, gap } => {
            let gap = f.px(gap.unwrap_or(8.0));
            let sum: f32 = children.iter().map(|c| super::height(f, c, width)).sum();
            sum + gap * children.len().saturating_sub(1) as f32
        }
        other => super::height(f, other, width),
    }
}

/// Draws `child` scrolled inside `rect`, its hits cut to the viewport.
pub(super) fn draw(f: &mut Frame<'_>, rect: Rect, id: &str, child: &View, on_end: Option<&String>, next: &mut HitId) {
    let key = (INSTANCE.with_borrow(Clone::clone), id.to_owned());
    // Out of the map while drawing, so a nested scroll can take its own.
    let mut pane = PANES.with_borrow_mut(|panes| panes.remove(&key)).unwrap_or_else(|| Pane {
        scroll: Scroll::new(&f.theme.motion),
        viewport: rect,
        scale: f.s,
        on_end: None,
        sent_at: None,
        drawn: false,
    });
    pane.viewport = rect;
    pane.scale = f.s;
    pane.on_end = on_end.cloned();
    pane.drawn = true;
    let width = rect.width();
    f.scroll(rect, &pane.scroll, Instant::now(), |f, origin| {
        let height = natural(f, child, width);
        // Every row gets its room (the list draws whole rows that fit); the clip does the rest.
        super::node(f, Rect::new(origin.x, origin.y, width, height.max(rect.height())), child, next);
        height
    });
    PANES.with_borrow_mut(|panes| panes.insert(key, pane));
}

/// What a wheel over a plugin face did.
#[derive(Debug, Default, PartialEq)]
pub struct Wheeled {
    /// A pane moved: repaint the widget (its viewport is the only damage).
    pub moved: bool,
    /// The pane's `on_reach_end` action, when the scroll heads to the end.
    pub reach_end: Option<String>,
}

/// Routes a wheel at `at` (surface px) on `instance` to the pane under it.
pub fn wheel(instance: &str, at: Point, delta: i16, now: Instant) -> Wheeled {
    PANES.with_borrow_mut(|panes| {
        let Some(pane) = panes
            .iter_mut()
            .filter(|(k, p)| k.0 == instance && p.viewport.contains(at))
            .map(|(_, p)| p)
            // The innermost (smallest) pane under the pointer.
            .min_by(|a, b| a.viewport.height().total_cmp(&b.viewport.height()))
        else {
            return Wheeled::default();
        };
        let moved = pane.scroll.wheel(delta, pane.scale, now);
        let max = pane.scroll.max();
        // Where the glide comes to rest.
        let rest = pane.scroll.offset(now + Duration::from_secs(5));
        let at_end = moved && max > 0.0 && rest >= max - pane.scale;
        let reach_end = match &pane.on_end {
            Some(action) if at_end && pane.sent_at.is_none_or(|sent| max > sent + 0.5) => {
                pane.sent_at = Some(max);
                Some(action.clone())
            }
            _ => None,
        };
        Wheeled { moved, reach_end }
    })
}

/// Advances `instance`'s glides and fades: when to draw it next (`None` at rest).
pub fn next(instance: &str, now: Instant) -> Option<Instant> {
    PANES.with_borrow_mut(|panes| {
        panes
            .iter_mut()
            .filter(|(k, _)| k.0 == instance)
            .filter_map(|(_, p)| {
                let (moving, next) = p.scroll.tick(now);
                next.or(moving.then(|| now + mypaper_ui::display_frame()))
            })
            .min()
    })
}

/// A pane's resting offset in device px, for tests.
#[cfg(test)]
pub fn offset(instance: &str, id: &str) -> Option<f32> {
    PANES.with_borrow(|panes| panes.get(&(instance.to_owned(), id.to_owned())).map(|p| p.scroll.offset(Instant::now() + Duration::from_secs(5))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::views::plugin::{ACTION, Hit, action_at, draw as face};
    use mypaper_plugin_sdk::Face;
    use mypaper_ui::{Interaction, Scheme, Style, Theme};
    use mypaper_ui::{Rgba, Ui};

    /// A face with a fixed header and 30 clickable rows in a scroll.
    fn chats() -> Face {
        let rows = (0..30).map(|i| View::Item { title: format!("Thread {i}"), detail: None, tag: None, tone: None, action: Some(format!("open-{i}")), nested: false });
        let list = View::List { children: rows.collect(), gap: Some(2.0) };
        let scroll = View::Scroll { id: "chats".into(), child: Box::new(list), on_reach_end: Some("more".into()) };
        View::stack([View::text("Header", mypaper_plugin_sdk::Role::Headline), scroll]).face()
    }

    /// Draws `face` as widget `instance` in a 300×240 tile; the hits' actions, top to bottom.
    fn draw(ui: &mut Ui, instance: &str, face_: &Face) -> Vec<(Rect, String)> {
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        let mut f = Frame::new(&mut ui.text, &theme, 1.0, &input);
        scope(instance, || face(&mut f, Rect::new(0.0, 0.0, 300.0, 240.0), "T3", mypaper_ui::icons::Icon::Terminal, Some(face_), super::super::Status::Live));
        f.list
            .hits
            .iter()
            .filter_map(|h| match action_at(face_, h.id) {
                Some(Hit::Action(a)) if h.id >= ACTION => Some((h.rect, a)),
                _ => None,
            })
            .collect()
    }

    fn viewport(instance: &str) -> Rect {
        PANES.with_borrow(|p| p[&(instance.to_owned(), "chats".to_owned())].viewport)
    }

    #[test]
    fn rows_are_clipped_to_the_viewport_and_stay_clickable_when_scrolled() {
        let mut ui = Ui::new().unwrap();
        let hits = draw(&mut ui, "clip", &chats());
        let view = viewport("clip");
        assert!(!hits.is_empty() && hits.len() < 30, "only the rows in view are hits: {}", hits.len());
        assert!(hits.iter().all(|(r, _)| r.top >= view.top && r.bottom <= view.bottom), "hits are cut to the viewport");
        assert_eq!(hits[0].1, "open-0");
        let inside = Point::new(view.left + 10.0, view.top + 10.0);
        let now = Instant::now();
        // A touchpad delta (not a multiple of 120) moves at once; a wheel outside the viewport (the header) does nothing.
        assert_eq!(wheel("clip", Point::new(10.0, 2.0), -120, now), Wheeled::default());
        assert!(wheel("clip", inside, -250, now).moved);
        assert_eq!(offset("clip", "chats"), Some(100.0), "250/120 of a notch");
        let hits = draw(&mut ui, "clip", &chats());
        assert_ne!(hits[0].1, "open-0", "the first row scrolled out");
        assert!(hits.iter().all(|(r, _)| r.top >= view.top - 0.5 && r.bottom <= view.bottom + 0.5));
        // The action numbering still maps every visible row to its own action.
        let first: usize = hits[0].1.trim_start_matches("open-").parse().unwrap();
        assert!(hits.iter().enumerate().all(|(i, (_, a))| *a == format!("open-{}", first + i)));
    }

    #[test]
    fn the_position_survives_view_updates_and_the_end_asks_for_more_once() {
        let mut ui = Ui::new().unwrap();
        draw(&mut ui, "keep", &chats());
        let inside = Point::new(20.0, viewport("keep").top + 20.0);
        let now = Instant::now();
        wheel("keep", inside, -60, now);
        let before = offset("keep", "chats");
        // A new view (a row's text changed) keeps the pane where it was.
        let mut update = chats();
        if let View::Stack { children, .. } = &mut update.body {
            children[0] = View::text("Header · 3 running", mypaper_plugin_sdk::Role::Headline);
        }
        draw(&mut ui, "keep", &update);
        assert_eq!(offset("keep", "chats"), before);
        assert!(before.unwrap() > 0.0);
        // Scrolling to the end sends `on_reach_end` once, not on every wheel after.
        let mut sent = Vec::new();
        for _ in 0..40 {
            sent.extend(wheel("keep", inside, -120, now).reach_end);
        }
        assert_eq!(sent, ["more"]);
        // A face without the scroll forgets the pane.
        draw(&mut ui, "keep", &View::text("gone", mypaper_plugin_sdk::Role::Body).face());
        assert_eq!(offset("keep", "chats"), None);
    }
}
