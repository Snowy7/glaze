use super::Command;
use crate::{
    actions::{Action, Bar, Edit, Pins},
    app::App,
    docs::{DocId, Op},
    ui::{B, Col},
};
use mypaper_ui::controls::{Glyph, Key, key};
use mypaper_ui::{Button, Event, Point, Rect};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum List {
    Modules(Bar, &'static str),
    Pins(Pins),
    Menu,
    Photos,
    Widget(String),
}

#[derive(Debug, Clone)]
pub struct Drag {
    list: List,
    from: usize,
    destination: Option<(List, usize)>,
    start: Point,
    moved: bool,
}

pub fn row_key(list: &List, index: usize) -> Key {
    key(("panel-sortable", list, index))
}

pub fn row(
    app: &App,
    b: &mut B,
    c: &mut Col,
    list: &List,
    index: usize,
    label: &str,
    separator: bool,
    icon: Option<Glyph>,
    select: Option<Action>,
) -> Rect {
    let rect = Rect::new(c.x, c.y, c.w, super::controls::height(b, 40.0));
    let picture = mypaper_ui::sortable::row(
        b,
        mypaper_ui::sortable::Row {
            key: row_key(list, index),
            rect,
            label,
            separator,
            icon,
            select: select.unwrap_or_else(|| Action::Panel(Command::SortRow(list.clone(), index))),
            drag: Action::Panel(Command::SortHandle(list.clone(), index)),
            remove: Action::Panel(Command::SortRemove(list.clone(), index)),
        },
    );
    if let Some(drag) = &app.panel.as_ref().unwrap().sort_drag
        && drag.moved
        && drag
            .destination
            .as_ref()
            .is_some_and(|(target, to)| target == list && *to == index)
    {
        let y = if drag.list == *list && index > drag.from {
            rect.bottom
        } else {
            rect.top
        };
        b.hline(rect.left, rect.right, y, b.p.ink1);
    }
    c.y = rect.bottom;
    picture
}

fn capture(app: &App, active: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    if let Some(window) = app.window {
        unsafe {
            if active {
                SetCapture(window);
            } else {
                let _ = ReleaseCapture();
            }
        }
    }
}

pub fn event(app: &mut App, event: &Event) -> bool {
    if let Event::Press(pointer, Button::Left) = event {
        let target = app
            .frame
            .targets
            .iter()
            .rev()
            .find(|t| t.visible.is_some_and(|r| r.contains(pointer.at)));
        if let Some(target) = target
            && let Action::Panel(Command::SortHandle(list, index)) = &target.action
        {
            app.panel.as_mut().unwrap().sort_drag = Some(Drag {
                list: list.clone(),
                from: *index,
                destination: Some((list.clone(), *index)),
                start: pointer.at,
                moved: false,
            });
            app.input.pressed = Some(target.key);
            app.input.focus = Some(target.key);
            app.input.focus_visible = false;
            capture(app, true);
            app.dirty = true;
            return true;
        }
    }
    let Some(drag) = app.panel.as_ref().unwrap().sort_drag.clone() else {
        return false;
    };
    match event {
        Event::Move(pointer) => {
            let destination = app.frame.targets.iter().rev().find_map(|t| {
                let (list, index) = match &t.action {
                    Action::Panel(
                        Command::SortRow(list, index) | Command::SortHandle(list, index),
                    ) if *list == drag.list
                        || matches!((&drag.list, list), (List::Modules(..), List::Modules(..))) =>
                    {
                        (list.clone(), *index)
                    }
                    Action::SelectModule(Some(slot)) if matches!(drag.list, List::Modules(..)) => {
                        (List::Modules(slot.bar, slot.list), slot.index)
                    }
                    _ => return None,
                };
                t.visible
                    .filter(|rect| rect.contains(pointer.at))
                    .map(|_| (list, index))
            });
            let active = app.panel.as_mut().unwrap().sort_drag.as_mut().unwrap();
            active.moved |= (pointer.at.y - active.start.y)
                .abs()
                .max((pointer.at.x - active.start.x).abs())
                > app.scale * 4.0;
            active.destination = destination;
            app.input.pointer = Some(pointer.at);
            app.dirty = true;
        }
        Event::Release(_, Button::Left) => {
            app.panel.as_mut().unwrap().sort_drag = None;
            capture(app, false);
            app.input.pressed = None;
            if drag.moved
                && let Some((list, to)) = drag.destination
            {
                if let (List::Modules(from_bar, from_zone), List::Modules(to_bar, to_zone)) =
                    (&drag.list, &list)
                {
                    move_module(
                        app,
                        crate::actions::Slot {
                            bar: *from_bar,
                            list: from_zone,
                            index: drag.from,
                        },
                        *to_bar,
                        to_zone,
                        to,
                    );
                } else {
                    reorder(app, &list, drag.from, to);
                }
                app.input.focus = Some(row_key(&list, to));
            }
            app.dirty = true;
        }
        Event::CaptureLost | Event::Key(0x1B) => {
            app.panel.as_mut().unwrap().sort_drag = None;
            capture(app, false);
            app.input.pressed = None;
            app.dirty = true;
        }
        _ => return false,
    }
    true
}

/// A captured module drag exposes all destinations without requiring a page change.
pub fn drop_overlay(app: &App, b: &mut B) {
    let Some(drag) = &app.panel.as_ref().unwrap().sort_drag else {
        return;
    };
    if !drag.moved || !matches!(drag.list, List::Modules(..)) {
        return;
    }
    use mypaper_ui::controls::Align;
    let x = b.px(16.0);
    let width = app.size.0 as f32 - x * 2.0;
    let line = super::controls::height(b, 30.0);
    let top = b.px(56.0);
    let plate = Rect::new(x, top, width, line * 5.0 + b.px(24.0));
    b.fill(plate, b.px(10.0), b.p.chrome);
    b.stroke(plate, b.px(10.0), b.px(1.0), b.p.edge);
    super::controls::text(
        b,
        Rect::new(x + b.px(10.0), top, width - b.px(20.0), line),
        "Drop into a zone",
        12.0,
        600,
        b.p.ink1,
        Align::Start,
    );
    let mut y = top + line;
    for bar in [Bar::Top, Bar::Dock] {
        let zones: &[(&str, &str)] = if bar == Bar::Top {
            &[("left", "Start"), ("center", "Middle"), ("right", "End")]
        } else {
            &[("left", "Start"), ("right", "End")]
        };
        let labels: Vec<_> = zones.iter().map(|(_, label)| *label).collect();
        let selected = zones.iter().position(|(zone, _)| {
            drag.destination
                .as_ref()
                .is_some_and(|(list, _)| *list == List::Modules(bar, zone))
        });
        let r = b.labeled_segments(
            key(("panel-drop-bar", bar)),
            Rect::new(x + b.px(10.0), y, width - b.px(20.0), 0.0),
            if bar == Bar::Top { "Top bar" } else { "Dock" },
            &labels,
            selected,
            |i| {
                let list = List::Modules(bar, zones[i].0);
                let index = values(app, &list).len();
                Action::Panel(Command::SortRow(list, index))
            },
        );
        y = r.bottom + b.px(4.0);
    }
}

/// Keyboard Place and pointer drops share one atomic, undoable transfer.
pub fn move_module(
    app: &mut App,
    source: crate::actions::Slot,
    bar: Bar,
    zone: &'static str,
    index: usize,
) {
    let from_list = List::Modules(source.bar, source.list);
    let to_list = List::Modules(bar, zone);
    if from_list == to_list {
        let last = values(app, &from_list).len().saturating_sub(1);
        reorder(app, &from_list, source.index, index.min(last));
        return;
    }
    let mut from = values(app, &from_list);
    if source.index >= from.len()
        || !matches!(zone, "left" | "center" | "right")
        || bar == Bar::Dock && zone == "center"
    {
        return;
    }
    let item = from.remove(source.index);
    if item.as_str() != Some("|")
        && ["left", "center", "right"].iter().any(|zone| {
            !(source.bar == bar && source.list == *zone)
                && values(app, &List::Modules(bar, zone)).contains(&item)
        })
    {
        app.alert("That module is already on this bar");
        return;
    }
    let mut to = values(app, &to_list);
    let index = index.min(to.len());
    to.insert(index, item);
    let version = app.docs.version;
    app.apply(
        "Move module",
        vec![
            Op {
                doc: DocId::Bars,
                path: super::editors::bar_path(
                    app,
                    &format!("/{}/{}", source.bar.key(), source.list),
                ),
                value: Some(json!(from)),
            },
            Op {
                doc: DocId::Bars,
                path: super::editors::bar_path(app, &format!("/{}/{zone}", bar.key())),
                value: Some(json!(to)),
            },
        ],
    );
    if app.docs.version != version {
        app.nav.module = Some(crate::actions::Slot {
            bar,
            list: zone,
            index,
        });
        app.nav.bar = Some(bar);
        if matches!(
            app.panel.as_ref().unwrap().page,
            super::Page::Top | super::Page::Dock
        ) {
            app.panel.as_mut().unwrap().page = if bar == Bar::Top {
                super::Page::Top
            } else {
                super::Page::Dock
            };
        }
    }
}

pub fn zone_target(app: &App, b: &mut B, rect: Rect, bar: Bar, zone: &'static str, index: usize) {
    if !app
        .panel
        .as_ref()
        .unwrap()
        .sort_drag
        .as_ref()
        .is_some_and(|drag| matches!(drag.list, List::Modules(..)))
    {
        return;
    }
    let list = List::Modules(bar, zone);
    b.hit(
        key(("panel-zone", &list)),
        rect,
        mypaper_ui::Role::Group,
        "Module zone",
        Action::Panel(Command::SortRow(list, index)),
        mypaper_ui::controls::Behavior::Click,
    );
}

pub fn keyboard(app: &mut App, delta: i32) -> bool {
    let Some(target) = app
        .input
        .focus
        .and_then(|k| app.frame.targets.iter().find(|t| t.key == k))
    else {
        return false;
    };
    let (list, index) = match &target.action {
        Action::Panel(Command::SortRow(list, index) | Command::SortHandle(list, index)) => {
            (list.clone(), *index)
        }
        Action::SelectModule(Some(slot)) => (List::Modules(slot.bar, slot.list), slot.index),
        _ => return false,
    };
    let len = values(app, &list).len();
    if len > 0 {
        let to = (index as i32 + delta).clamp(0, len as i32 - 1) as usize;
        reorder(app, &list, index, to);
        app.input.focus = Some(row_key(&list, to));
    }
    true
}

fn values(app: &App, list: &List) -> Vec<Value> {
    match list {
        List::Modules(bar, side) => {
            super::editors::bar_value(app, &format!("/{}/{side}", bar.key()))
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        }
        List::Pins(pins) => app
            .get(DocId::Bars, pins.path())
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        List::Menu => app
            .get(DocId::Config, "/desktop_menu/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| {
                serde_json::to_value(mypaper_config::desktop_menu::default_items())
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .clone()
            }),
        List::Photos | List::Widget(_) => {
            let layout = crate::pages::widgets::layout(app);
            let instance = match list {
                List::Widget(path) => path
                    .split('/')
                    .nth(2)
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|i| layout.instances.get(i)),
                _ => app.nav.widget.as_deref().and_then(|id| layout.get(id)),
            };
            let Some(instance) = instance else {
                return vec![];
            };
            let Some(manifest) = app.registry.get(&instance.widget).map(|w| &w.manifest) else {
                return vec![];
            };
            let resolved = manifest.resolve_settings(&instance.settings);
            if let List::Widget(path) = list {
                let name = path
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .replace("~1", "/")
                    .replace("~0", "~");
                resolved
                    .get(&name)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            } else {
                (1..=6)
                    .filter_map(|n| {
                        resolved
                            .get(&format!("image{n}"))
                            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                            .cloned()
                    })
                    .collect()
            }
        }
    }
}

fn store(app: &mut App, list: &List, items: Vec<Value>, label: &str) {
    let (doc, path) = match list {
        List::Modules(bar, side) => (
            DocId::Bars,
            super::editors::bar_path(app, &format!("/{}/{side}", bar.key())),
        ),
        List::Pins(pins) => (DocId::Bars, pins.path().into()),
        List::Menu => (DocId::Config, "/desktop_menu/items".into()),
        List::Widget(path) => (DocId::Widgets, path.clone()),
        List::Photos => {
            let layout = crate::pages::widgets::layout(app);
            let Some(index) = layout
                .instances
                .iter()
                .position(|i| Some(i.instance.as_str()) == app.nav.widget.as_deref())
            else {
                return;
            };
            app.apply(
                label,
                (0..6)
                    .map(|n| Op {
                        doc: DocId::Widgets,
                        path: format!("/instances/{index}/settings/image{}", n + 1),
                        value: Some(items.get(n).cloned().unwrap_or(json!(""))),
                    })
                    .collect(),
            );
            return;
        }
    };
    app.set(&Edit::new(doc, path, label), Value::Array(items));
}

pub fn reorder(app: &mut App, list: &List, from: usize, to: usize) {
    let mut items = values(app, list);
    if mypaper_ui::sortable::reorder(&mut items, from, to) {
        let version = app.docs.version;
        store(app, list, items, "Reorder items");
        if app.docs.version == version {
            return;
        }
        if let List::Modules(bar, side) = list
            && let Some(slot) = app.nav.module.as_mut()
            && slot.bar == *bar
            && slot.list == *side
        {
            slot.index = if slot.index == from {
                to
            } else if from < slot.index && slot.index <= to {
                slot.index - 1
            } else if to <= slot.index && slot.index < from {
                slot.index + 1
            } else {
                slot.index
            };
        }
    }
}

pub fn remove(app: &mut App, list: &List, index: usize) {
    let mut items = values(app, list);
    if index < items.len() {
        items.remove(index);
        let version = app.docs.version;
        store(app, list, items, "Remove item");
        if app.docs.version == version {
            return;
        }
        if let List::Modules(bar, side) = list
            && let Some(slot) = app.nav.module.as_mut()
            && slot.bar == *bar
            && slot.list == *side
        {
            if slot.index == index {
                app.nav.module = None;
                if let Some(panel) = &mut app.panel
                    && panel.page == super::Page::Module
                {
                    panel.page = if *bar == Bar::Dock {
                        super::Page::Dock
                    } else {
                        super::Page::Top
                    };
                }
            } else if slot.index > index {
                slot.index -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(ui: &mypaper_ui::Ui) -> App {
        let mut app = crate::goldens::app(true, ui);
        app.panel = Some(super::super::State::default());
        app.size = (480, 760);
        app.docs = crate::docs::Docs::fixture([
            "{}",
            r#"{"version":1,"top":{"left":["start","network"],"center":[],"right":["clock"]},"dock":{"left":[],"right":["volume"]}}"#,
            "{}",
            "{}",
        ]);
        app
    }

    #[test]
    fn module_transfers_cross_zones_and_bars_atomically_on_the_selected_display() {
        let ui = mypaper_ui::Ui::new().unwrap();
        for (bar, zone) in [(Bar::Top, "center"), (Bar::Dock, "left")] {
            let mut app = fixture(&ui);
            app.nav.display = 1;
            let before = app.docs.doc(DocId::Bars).raw.clone();
            move_module(
                &mut app,
                crate::actions::Slot {
                    bar: Bar::Top,
                    list: "left",
                    index: 1,
                },
                bar,
                zone,
                0,
            );
            assert_eq!(
                values(&app, &List::Modules(Bar::Top, "left")),
                [json!("start")]
            );
            assert_eq!(values(&app, &List::Modules(bar, zone)), [json!("network")]);
            assert_eq!(
                app.docs.doc(DocId::Bars).raw.pointer("/top/left"),
                before.pointer("/top/left")
            );
            app.run(Action::Undo);
            assert_eq!(app.docs.doc(DocId::Bars).raw, before);
        }
    }

    #[test]
    fn duplicate_modules_and_invalid_zones_leave_both_lists_intact() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        let source = crate::actions::Slot {
            bar: Bar::Top,
            list: "left",
            index: 0,
        };
        app.set(
            &Edit::new(DocId::Bars, "/dock/left", "Fixture modules"),
            json!(["start"]),
        );
        let version = app.docs.version;
        let before = app.docs.doc(DocId::Bars).raw.clone();
        move_module(&mut app, source, Bar::Dock, "left", 0);
        assert_eq!(app.docs.version, version);
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
        move_module(
            &mut app,
            crate::actions::Slot {
                bar: Bar::Top,
                list: "left",
                index: 0,
            },
            Bar::Dock,
            "center",
            0,
        );
        assert_eq!(app.docs.version, version);
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
    }

    #[test]
    fn captured_drag_exposes_other_bar_zones_and_writes_only_on_release() {
        use mypaper_ui::Pointer;
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = fixture(&ui);
        app.panel.as_mut().unwrap().page = super::super::Page::Top;
        app.nav.bar = Some(Bar::Top);
        let before = app.docs.doc(DocId::Bars).raw.clone();
        let built = super::super::build(&app, &mut ui.text);
        let handle = built
            .targets
            .iter()
            .find(|target| {
                target.action
                    == Action::Panel(Command::SortHandle(List::Modules(Bar::Top, "left"), 1))
            })
            .unwrap()
            .rect;
        let pointer = |rect: Rect| Pointer {
            at: Point::new(
                (rect.left + rect.right) / 2.0,
                (rect.top + rect.bottom) / 2.0,
            ),
            screen: (0, 0),
        };
        app.frame.targets = built.targets;
        assert!(event(
            &mut app,
            &Event::Press(pointer(handle), Button::Left)
        ));
        assert!(event(&mut app, &Event::Move(pointer(handle.inflate(10.0)))));
        // Move horizontally to leave the threshold, then rebuild the drag overlay.
        let mut moved = pointer(handle);
        moved.at.x += 10.0;
        assert!(event(&mut app, &Event::Move(moved)));
        let built = super::super::build(&app, &mut ui.text);
        let drop = built
            .targets
            .iter()
            .find(|target| {
                target.action
                    == Action::Panel(Command::SortRow(List::Modules(Bar::Dock, "left"), 0))
            })
            .unwrap()
            .rect;
        app.frame.targets = built.targets;
        assert!(event(&mut app, &Event::Move(pointer(drop))));
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
        assert!(event(
            &mut app,
            &Event::Release(pointer(drop), Button::Left)
        ));
        assert_eq!(
            values(&app, &List::Modules(Bar::Dock, "left")),
            [json!("network")]
        );
        assert_eq!(app.docs.undo_count(), 1);
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Bars).raw, before);
    }
}
