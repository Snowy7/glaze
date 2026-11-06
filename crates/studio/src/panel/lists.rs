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
    to: usize,
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
        && drag.list == *list
        && drag.moved
        && drag.to == index
    {
        let y = if drag.to > drag.from {
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
                to: *index,
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
            let to = app.frame.targets.iter().find_map(|t| {
                let index = match &t.action {
                    Action::Panel(
                        Command::SortRow(list, index) | Command::SortHandle(list, index),
                    ) if *list == drag.list => *index,
                    Action::SelectModule(Some(slot))
                        if drag.list == List::Modules(slot.bar, slot.list) =>
                    {
                        slot.index
                    }
                    _ => return None,
                };
                (pointer.at.y >= t.rect.top && pointer.at.y <= t.rect.bottom).then_some(index)
            });
            let active = app.panel.as_mut().unwrap().sort_drag.as_mut().unwrap();
            active.moved |= (pointer.at.y - active.start.y).abs() > app.scale * 4.0;
            if let Some(to) = to {
                active.to = to;
            }
            app.input.pointer = Some(pointer.at);
            app.dirty = true;
        }
        Event::Release(_, Button::Left) => {
            app.panel.as_mut().unwrap().sort_drag = None;
            capture(app, false);
            app.input.pressed = None;
            if drag.moved {
                reorder(app, &drag.list, drag.from, drag.to);
            }
            app.input.focus = Some(row_key(&drag.list, drag.to));
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
            } else if slot.index > index {
                slot.index -= 1;
            }
        }
    }
}
