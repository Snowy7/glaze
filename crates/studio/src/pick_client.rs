//! Studio's transient selection client. The panel calls `connect` while building and
//! `on_key` for the semantic IPC wake; no native keyboard input is synthesized.
use mypaper_shell::studio_pick::wire::{self, Kind, Message, Selection};
use std::cell::RefCell;
use windows::Win32::Foundation::HWND;

use super::{Command, Page};
use crate::actions::{Action, Bar, Slot};
use crate::app::App;
use crate::docs::{DocId, Op};
use serde_json::Value;

struct Client {
    endpoint: wire::Endpoint,
    shell: Option<HWND>,
    begun: bool,
    panel: HWND,
    highlighted: Option<String>,
    observed: [(DocId, Value); 3],
    observed_version: u64,
    undo_chain: UndoChain,
    session: u64,
    sequence: u64,
    preview: Option<PreviewDrag>,
    undo_serial: u64,
    undo_step: Option<UndoGuard>,
    page_selection: Option<Selection>,
    desktop_selected: bool,
}

struct PreviewDrag {
    documents: Vec<wire::PreviewDocument>,
    label: String,
    checkpoint: usize,
}

#[derive(Clone, Copy)]
struct UndoGuard {
    step: u64,
    count: usize,
    version: u64,
}

impl UndoGuard {
    fn matches(self, app: &App, step: u64) -> bool {
        self.step == step
            && self.count == app.docs.undo_count()
            && self.version == app.docs.version
            && app.drag.is_none()
    }
}

impl Client {
    fn stream_preview(&mut self, app: &App) {
        let Some(shell) = self.shell else { return };
        if let Some(crate::app::Drag::Slider { edit, .. }) = &app.drag {
            let documents = preview_documents(&self.observed, app);
            if self
                .preview
                .as_ref()
                .is_some_and(|p| p.documents == documents)
            {
                return;
            }
            self.sequence += 1;
            let message = if documents.is_empty() {
                Message::PreviewCancel {
                    session: self.session,
                    sequence: self.sequence,
                }
            } else {
                Message::Preview {
                    session: self.session,
                    sequence: self.sequence,
                    documents: documents.clone(),
                }
            };
            wire::send(shell, self.endpoint.hwnd(), &message);
            let checkpoint = self
                .preview
                .as_ref()
                .map_or(app.docs.undo_count(), |p| p.checkpoint);
            self.preview = Some(PreviewDrag {
                documents,
                label: edit.label.to_string(),
                checkpoint,
            });
        } else if let Some(drag) = self.preview.take() {
            // A release can share a pump turn with the last move, before another panel frame.
            let documents = preview_documents(&self.observed, app);
            if !documents.is_empty() && documents != drag.documents {
                self.sequence += 1;
                wire::send(
                    shell,
                    self.endpoint.hwnd(),
                    &Message::Preview {
                        session: self.session,
                        sequence: self.sequence,
                        documents,
                    },
                );
            }
            self.sequence += 1;
            let saved = !matches!(app.docs.saved, crate::docs::Saved::Failed(_));
            let message = if saved {
                Message::PreviewCommit {
                    session: self.session,
                    sequence: self.sequence,
                    revisions: self
                        .observed
                        .iter()
                        .map(|(id, _)| wire::Revision {
                            document: document(*id),
                            revision: wire::revision(&app.docs.doc(*id).raw),
                        })
                        .collect(),
                }
            } else {
                Message::PreviewCancel {
                    session: self.session,
                    sequence: self.sequence,
                }
            };
            wire::send(shell, self.endpoint.hwnd(), &message);
            if saved && app.docs.undo_count() > drag.checkpoint {
                self.announce(app, format!("{} · Undo", drag.label));
            }
        }
    }

    fn announce(&mut self, app: &App, text: String) {
        let (Some(shell), Some(display)) = (self.shell, app.display()) else {
            return;
        };
        self.undo_serial += 1;
        let step = self.undo_serial;
        self.undo_step = Some(UndoGuard {
            step,
            count: app.docs.undo_count(),
            version: app.docs.version,
        });
        wire::send(
            shell,
            self.endpoint.hwnd(),
            &Message::UndoNotice {
                display: display.device.clone(),
                text,
                step,
            },
        );
    }
}

fn document(id: DocId) -> wire::Document {
    match id {
        DocId::Config => wire::Document::Config,
        DocId::Bars => wire::Document::Bars,
        DocId::Widgets => wire::Document::Widgets,
        _ => unreachable!("only desktop documents have previews"),
    }
}

fn preview_documents(before: &[(DocId, Value); 3], app: &App) -> Vec<wire::PreviewDocument> {
    fn diff(
        document: wire::Document,
        path: &str,
        before: &Value,
        after: &Value,
        ops: &mut Vec<wire::PreviewOp>,
    ) {
        if before == after {
            return;
        }
        if before.is_object() || after.is_object() {
            let keys: std::collections::BTreeSet<_> = before
                .as_object()
                .into_iter()
                .flat_map(|o| o.keys())
                .chain(after.as_object().into_iter().flat_map(|o| o.keys()))
                .collect();
            for key in keys {
                diff(
                    document,
                    &format!("{path}/{key}"),
                    &before[key],
                    &after[key],
                    ops,
                );
            }
        } else if let (Some(old), Some(new)) = (before.as_array(), after.as_array()) {
            if old.len() == new.len() {
                for (i, (old, new)) in old.iter().zip(new).enumerate() {
                    diff(document, &format!("{path}/{i}"), old, new, ops);
                }
            }
        } else {
            let op = wire::PreviewOp {
                path: path.into(),
                value: after.clone(),
            };
            if op.valid(document) {
                ops.push(op);
            }
        }
    }
    before
        .iter()
        .filter_map(|(id, raw)| {
            let document = document(*id);
            let mut ops = Vec::new();
            diff(document, "", raw, &app.docs.doc(*id).raw, &mut ops);
            (!ops.is_empty() && ops.len() <= 32).then(|| wire::PreviewDocument {
                document,
                base_revision: wire::revision(raw),
                ops,
            })
        })
        .collect()
}

#[derive(Default)]
struct UndoChain {
    values: Vec<String>,
}

impl UndoChain {
    fn record(&mut self, before: String, after: String, merge: bool) -> String {
        if !merge || self.values.is_empty() {
            self.values = vec![before];
        }
        if self.values.last() != Some(&after) {
            self.values.push(after);
        }
        // Preserve the start and two most recent choices when someone explores a long list.
        if self.values.len() > 3 {
            self.values.remove(1);
        }
        format!(
            "{}{} · Undo",
            self.values.join(" → "),
            if merge { " is one step" } else { "" }
        )
    }
}

fn snapshot(app: &App) -> [(DocId, Value); 3] {
    [DocId::Config, DocId::Bars, DocId::Widgets].map(|id| (id, app.docs.doc(id).raw.clone()))
}

thread_local! { static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) }; }

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(shell) = self.shell {
            wire::send(shell, self.endpoint.hwnd(), &Message::End);
        }
    }
}

/// Starts only for a real panel window (fixtures never contact the desktop).
pub fn connect(app: &App) {
    if app.fixture {
        return;
    }
    let Some(panel) = app.window else { return };
    CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        if client.as_ref().is_some_and(|c| c.panel != panel) {
            *client = None;
        }
        if client.is_none() {
            let Ok(endpoint) = wire::Endpoint::new(false) else {
                return;
            };
            endpoint.wake_ui(panel);
            *client = Some(Client {
                endpoint,
                shell: None,
                begun: false,
                panel,
                highlighted: None,
                observed: snapshot(app),
                observed_version: app.docs.version,
                undo_chain: UndoChain::default(),
                session: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
                sequence: 0,
                preview: None,
                undo_serial: 0,
                undo_step: None,
                page_selection: None,
                desktop_selected: false,
            });
        }
        let client = client.as_mut().unwrap();
        let shell = wire::find_shell();
        if client.shell != shell {
            client.shell = shell;
            client.begun = false;
            client.highlighted = None;
            client.preview = None;
            client.page_selection = None;
            client.desktop_selected = false;
            client.undo_step = None;
        }
        if !client.begun
            && let Some(shell) = shell
        {
            // A timeout can still deliver Begin. Keep the peer identity so its valid reply
            // is accepted, and retry the handshake on the next frame if no reply arrives.
            client.begun = wire::send(
                shell,
                client.endpoint.hwnd(),
                &Message::Begin {
                    panel: panel.0 as isize,
                },
            );
        }
        if client.undo_step.is_some_and(|g| !g.matches(app, g.step)) {
            client.undo_step = None;
            if let Some(shell) = client.shell {
                wire::send(shell, client.endpoint.hwnd(), &Message::UndoDismiss);
            }
        }
        client.stream_preview(app);
        let selection = page_selection(app);
        if client.page_selection != selection {
            if let Some(shell) = client.shell {
                let message = selection
                    .clone()
                    .map_or(Message::ClearSelection, |selection| Message::Selected {
                        selection,
                    });
                if wire::send(shell, client.endpoint.hwnd(), &message) {
                    client.desktop_selected = selection.is_some();
                    client.page_selection = selection;
                }
            }
        }
        if app.drag.is_none() && client.observed_version != app.docs.version {
            client.observed = snapshot(app);
            client.observed_version = app.docs.version;
        }
    });
    highlight(hovered_widget(app, app.input.pointer));
}

fn page_selection(app: &App) -> Option<Selection> {
    let page = app.panel.as_ref()?.page;
    let display = app.display()?.device.clone();
    let (kind, id) = match page {
        Page::Widget => (Kind::Widget, app.nav.widget.clone()?),
        Page::Top => (Kind::Bar, "top".into()),
        Page::Dock => (Kind::Bar, "dock".into()),
        Page::Module => {
            let slot = app.nav.module.as_ref()?;
            (
                Kind::Module,
                format!(
                    "{}/{}/{}",
                    if slot.bar == Bar::Dock { "dock" } else { "top" },
                    slot.list,
                    slot.index
                ),
            )
        }
        _ => return None,
    };
    Some(Selection { kind, id, display })
}

/// Resolve the actual row rectangle: accessibility and pointer hover need not share a key.
fn hovered_widget(app: &App, point: Option<mypaper_ui::Point>) -> Option<String> {
    let point = point?;
    app.frame
        .targets
        .iter()
        .rev()
        .find(|t| t.visible.is_some_and(|r| r.contains(point)))
        .and_then(|target| match &target.action {
            Action::SelectWidget(id) => id.clone(),
            Action::Panel(Command::Open(Page::Widget)) => app.nav.widget.clone(),
            _ => None,
        })
}

/// Called only after a successful pick has entered Studio's undo history.
pub fn record_pick(app: &App, label: &str, ops: &[Op], checkpoint: usize, merge: bool) {
    if app.fixture || (!merge && app.docs.undo_count() <= checkpoint) {
        return;
    }
    CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        let Some(client) = client.as_mut() else {
            return;
        };
        if client.shell.is_none() {
            return;
        }
        let choice = ops.iter().find_map(|op| {
            let before = client
                .observed
                .iter()
                .find(|(id, _)| *id == op.doc)?
                .1
                .pointer(&op.path);
            (before != op.value.as_ref()).then(|| {
                (
                    choice_name(before, &op.path, label),
                    choice_name(op.value.as_ref(), &op.path, label),
                )
            })
        });
        if let Some((before, after)) = choice {
            let text = client.undo_chain.record(before, after, merge);
            client.announce(app, text);
        }
        client.observed = snapshot(app);
        client.observed_version = app.docs.version;
    });
}

fn choice_name(value: Option<&Value>, path: &str, fallback: &str) -> String {
    let name = match value {
        Some(Value::String(value)) if value == "oneui" => "One UI".into(),
        Some(Value::String(value)) => value
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(value)
            .replace(['_', '-'], " "),
        Some(Value::Bool(value)) => if *value { "On" } else { "Off" }.into(),
        Some(Value::Number(value)) => value.to_string(),
        None | Some(Value::Null) if path.ends_with("/style") => "Liquid".into(),
        None | Some(Value::Null) => "Default".into(),
        _ => fallback.into(),
    };
    let mut chars = name.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    format!("{first}{}", chars.take(38).collect::<String>())
}

/// Also available to a row that has a different navigation action.
pub fn highlight(id: Option<String>) {
    CLIENT.with(|client| {
        if let Some(client) = client.borrow_mut().as_mut() {
            if client.highlighted == id {
                return;
            }
            if let Some(shell) = client.shell {
                wire::send(
                    shell,
                    client.endpoint.hwnd(),
                    &Message::Highlight { id: id.clone() },
                );
            }
            client.highlighted = id;
        }
    });
}

/// Display switches end the desktop selection and place the panel without activation.
pub fn clear(app: &App) {
    deselect();
    reposition(app);
}

fn deselect() {
    CLIENT.with(|client| {
        if let Some(client) = client.borrow_mut().as_mut() {
            if let Some(shell) = client.shell {
                wire::send(shell, client.endpoint.hwnd(), &Message::ClearSelection);
            }
            client.highlighted = None;
            client.desktop_selected = false;
        }
    });
}

pub fn on_key(app: &mut App, key: u16) -> bool {
    if key == 0x1B && app.field.is_none() && app.overlay.is_none() && app.drag.is_none() {
        let selected = CLIENT.with(|c| c.borrow().as_ref().is_some_and(|c| c.desktop_selected));
        if selected {
            deselect();
            return true;
        }
    }
    if key != wire::PICK_EVENT {
        return false;
    }
    let messages = CLIENT.with(|client| {
        let client = client.borrow();
        let Some(client) = client.as_ref() else {
            return Vec::new();
        };
        let mut messages = Vec::new();
        while let Some((source, message)) = client.endpoint.next() {
            if Some(source) == client.shell {
                messages.push(message);
            }
        }
        messages
    });
    for message in messages {
        match message {
            Message::Selected { selection } => {
                CLIENT.with(|c| {
                    if let Some(c) = c.borrow_mut().as_mut() {
                        c.desktop_selected = true;
                    }
                });
                selected(app, selection);
            }
            Message::UndoRequest { step } => {
                let guard =
                    CLIENT.with(|c| c.borrow_mut().as_mut().and_then(|c| c.undo_step.take()));
                if guard.is_some_and(|g| g.matches(app, step)) {
                    app.run(Action::Undo);
                }
                CLIENT.with(|c| {
                    if let Some(c) = c.borrow().as_ref()
                        && let Some(shell) = c.shell
                    {
                        wire::send(shell, c.endpoint.hwnd(), &Message::UndoDismiss);
                    }
                });
            }
            Message::ResizeWidget {
                selection,
                before_cell,
                before_size,
                cell,
                size,
            } => {
                resize_widget(app, selection, before_cell, before_size, cell, size);
            }
            _ => {}
        }
    }
    true
}

/// The shell never saves widget geometry. Release returns here for one validated undo step.
fn resize_widget(
    app: &mut App,
    selection: Selection,
    before_cell: [u32; 2],
    before_size: [u32; 2],
    cell: [u32; 2],
    size: [u32; 2],
) {
    if selection.kind != Kind::Widget || (cell, size) == (before_cell, before_size) {
        return;
    }
    let layout = crate::pages::widgets::layout(app);
    let Some((index, instance)) = layout
        .instances
        .iter()
        .enumerate()
        .find(|(_, i)| i.instance == selection.id)
    else {
        return;
    };
    if instance.cell != before_cell || instance.size != before_size {
        app.alert("This widget changed while resizing. Try again.");
        return;
    }
    let Some(widget) = app.registry.get(&instance.widget) else {
        return;
    };
    if !widget.manifest.resizable
        || widget.manifest.fit_size(size.map(f64::from), [u32::MAX; 2]) != Some(size)
    {
        return;
    }
    let label = format!("Resize {}", widget.manifest.name);
    let mut ops = Vec::new();
    if app.get(DocId::Widgets, "/version").and_then(Value::as_u64) == Some(1) {
        // Upgrade all grid coordinates together, preserving unknown fields in the raw file.
        ops.push(Op {
            doc: DocId::Widgets,
            path: "/version".into(),
            value: Some(serde_json::json!(2)),
        });
        for (index, instance) in layout.instances.iter().enumerate() {
            for (field, value) in [("cell", instance.cell), ("size", instance.size)] {
                ops.push(Op {
                    doc: DocId::Widgets,
                    path: format!("/instances/{index}/{field}"),
                    value: Some(serde_json::json!(value)),
                });
            }
        }
    }
    ops.extend([
        Op {
            doc: DocId::Widgets,
            path: format!("/instances/{index}/cell"),
            value: Some(serde_json::json!(cell)),
        },
        Op {
            doc: DocId::Widgets,
            path: format!("/instances/{index}/size"),
            value: Some(serde_json::json!(size)),
        },
    ]);
    app.apply(&label, ops);
}

/// The one navigation seam. Selection changes only navigation, never document values.
fn selected(app: &mut App, selection: Selection) {
    let mut display = display_index(&app.monitors, &selection.display);
    if display.is_none() && !app.fixture {
        app.monitors = mypaper_desktop::monitor_infos();
        display = display_index(&app.monitors, &selection.display);
    }
    if let Some(display) = display {
        app.nav.display = display;
    }
    let page = match selection.kind {
        Kind::Widget => {
            app.nav.widget = Some(selection.id);
            Page::Widget
        }
        Kind::Wallpaper => Page::Wallpapers,
        Kind::Bar | Kind::Module => {
            let mut id = selection.id.split('/');
            let bar = if id.next() == Some("dock") {
                Bar::Dock
            } else {
                Bar::Top
            };
            app.nav.bar = Some(bar);
            app.nav.module = if selection.kind == Kind::Module {
                match (id.next(), id.next().and_then(|v| v.parse::<usize>().ok())) {
                    (Some(list @ ("left" | "center" | "right")), Some(index)) => Some(Slot {
                        bar,
                        list: match list {
                            "left" => "left",
                            "center" => "center",
                            _ => "right",
                        },
                        index,
                    }),
                    _ => None,
                }
            } else {
                None
            };
            if selection.kind == Kind::Module && app.nav.module.is_some() {
                Page::Module
            } else if bar == Bar::Dock {
                Page::Dock
            } else {
                Page::Top
            }
        }
    };
    if let Some(panel) = &mut app.panel {
        panel.page = page;
    }
    // Navigation can already name this display after a switcher change or a window drag.
    // Every matched desktop pick establishes the panel's actual placement too.
    if display.is_some() {
        reposition(app);
    }
    app.input.focus = None;
    app.input.hover = None;
    app.dirty = true;
    log::info!("Studio desktop selection opened {page:?}");
}

fn display_index(monitors: &[mypaper_desktop::Monitor], device: &str) -> Option<usize> {
    monitors
        .iter()
        .position(|m| m.device.eq_ignore_ascii_case(device))
}

fn reposition(app: &App) {
    if let Some(window) = app.window {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let bounds = super::placement(app);
        let mut outer = RECT::default();
        let mut client = POINT::default();
        // SAFETY: only the panel's own window moves, preserving focus and the other windows.
        unsafe {
            if GetWindowRect(window, &mut outer).is_ok()
                && ClientToScreen(window, &mut client).as_bool()
            {
                let _ = SetWindowPos(
                    window,
                    None,
                    bounds.0 - (client.x - outer.left),
                    bounds.1 - (client.y - outer.top),
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_ui::Ui;

    #[test]
    fn consecutive_picks_update_one_undo_chip_and_new_lists_reset_it() {
        assert_eq!(
            choice_name(Some(&serde_json::json!("oneui")), "/look/style", "Style"),
            "One UI"
        );
        let mut chain = UndoChain::default();
        assert_eq!(
            chain.record("Liquid".into(), "Fluent".into(), false),
            "Liquid → Fluent · Undo"
        );
        assert_eq!(
            chain.record("Fluent".into(), "One UI".into(), true),
            "Liquid → Fluent → One UI is one step · Undo"
        );
        assert_eq!(
            chain.record("Digital".into(), "World".into(), false),
            "Digital → World · Undo"
        );
        for i in 0..20 {
            chain.record("World".into(), i.to_string(), true);
        }
        assert_eq!(chain.values.len(), 3);
        assert_eq!(chain.values[0], "Digital");
    }

    #[test]
    fn resize_release_is_one_document_step_and_refuses_a_stale_gesture() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let before = app.docs.doc(DocId::Widgets).raw.clone();
        let layout = crate::pages::widgets::layout(&app);
        let instance = layout.get("clock-1").unwrap();
        let selection = Selection {
            kind: Kind::Widget,
            id: instance.instance.clone(),
            display: instance.monitor.clone(),
        };
        let (cell, size) = (instance.cell, instance.size);
        let next = [size[0] + 1, size[1] + 1];
        resize_widget(&mut app, selection.clone(), cell, size, cell, next);
        assert_eq!(
            crate::pages::widgets::layout(&app)
                .get("clock-1")
                .unwrap()
                .size,
            next
        );
        assert_eq!(app.docs.undo_count(), 1);
        let revision = app.docs.version;
        resize_widget(
            &mut app,
            selection,
            cell,
            size,
            cell,
            [next[0] + 1, next[1]],
        );
        assert_eq!(
            app.docs.version, revision,
            "a concurrent edit invalidates the original size"
        );
        assert_eq!(app.docs.undo_count(), 1);
        app.docs.undo().unwrap();
        assert_eq!(
            app.docs.doc(DocId::Widgets).raw,
            before,
            "the complete version-one layout is restored too"
        );
        assert!(DocId::ALL.iter().all(|id| app.docs.doc(*id).path.is_none()));
    }

    #[test]
    fn a_pick_places_the_panel_even_when_navigation_already_names_that_display() {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        app.nav.display = 1;
        let surface = mypaper_ui::Surface::new(
            mypaper_ui::Kind::App { min: (360, 160) },
            "Studio pick placement test",
            mypaper_ui::Role::Window,
        )
        .unwrap();
        // Only an owned, hidden test HWND is moved. No real panel or other window is used.
        surface.place((100, 100, 480, 600));
        app.window = Some(surface.hwnd());
        let display = app.monitors[1].device.clone();
        let revision = app.docs.version;
        let foreground = unsafe { GetForegroundWindow() };
        selected(
            &mut app,
            Selection {
                kind: Kind::Widget,
                id: "clock-1".into(),
                display,
            },
        );
        let mut point = POINT::default();
        assert!(unsafe { ClientToScreen(surface.hwnd(), &mut point) }.as_bool());
        assert!(
            point.x < 0,
            "the same display's pick must bring the panel back: {point:?}"
        );
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        assert_eq!(app.docs.version, revision);
    }

    #[test]
    fn display_matching_uses_windows_device_identity_not_enumeration_order() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.monitors.reverse();
        let left = app.monitors[0].device.to_ascii_lowercase();
        assert_eq!(display_index(&app.monitors, &left), Some(0));
        app.panel = Some(crate::panel::State::default());
        app.nav.display = 1;
        selected(
            &mut app,
            Selection {
                kind: Kind::Widget,
                id: "calendar-1".into(),
                display: left,
            },
        );
        assert_eq!(app.nav.display, 0);
        assert!(app.display().unwrap().rect.x < 0);
        assert_eq!(display_index(&app.monitors, r"\\.\DISPLAY99"), None);
    }

    #[test]
    fn dock_slider_streams_memory_values_then_the_saved_revision() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        let receiver = wire::Endpoint::new(true).unwrap();
        let base_revision = wire::revision(&app.docs.doc(DocId::Bars).raw);
        let mut client = Client {
            endpoint: wire::Endpoint::new(false).unwrap(),
            shell: Some(receiver.hwnd()),
            begun: true,
            panel: HWND::default(),
            highlighted: None,
            observed: snapshot(&app),
            observed_version: app.docs.version,
            undo_chain: UndoChain::default(),
            session: 123,
            sequence: 0,
            preview: None,
            undo_serial: 0,
            undo_step: None,
            page_selection: None,
            desktop_selected: false,
        };
        let edit = crate::actions::Edit::new(DocId::Bars, "/dock/size", "Icon size");
        app.drag = Some(crate::app::Drag::Slider {
            edit,
            track: mypaper_ui::Rect::new(0.0, 0.0, 100.0, 20.0),
            min: 24.0,
            max: 96.0,
            step: 1.0,
        });
        app.docs
            .live(&[Op {
                doc: DocId::Bars,
                path: "/dock/size".into(),
                value: Some(serde_json::json!(64.0)),
            }])
            .unwrap();
        client.stream_preview(&app);
        assert!(
            matches!(receiver.next(), Some((_, Message::Preview { session: 123, sequence: 1, documents }))
                if documents.len() == 1 && documents[0].base_revision == base_revision
                    && documents[0].ops[0].value.as_f64() == Some(64.0))
        );
        assert!(
            !app.docs.can_undo(),
            "there is no saved step during the drag"
        );
        app.docs.commit_live("Icon size").unwrap();
        app.drag = None;
        client.stream_preview(&app);
        assert!(
            matches!(receiver.next(), Some((_, Message::PreviewCommit { session: 123, sequence: 2, revisions }))
                if revisions.iter().any(|r| r.document == wire::Document::Bars && r.revision == wire::revision(&app.docs.doc(DocId::Bars).raw)))
        );
        assert_eq!(app.docs.undo_count(), 1);
        assert!(DocId::ALL.iter().all(|id| app.docs.doc(*id).path.is_none()));
    }

    #[test]
    fn desktop_picks_navigate_without_editing_documents() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let revision = app.docs.version;
        selected(
            &mut app,
            Selection {
                kind: Kind::Widget,
                id: "clock-2".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Widget);
        assert_eq!(app.nav.widget.as_deref(), Some("clock-2"));
        selected(
            &mut app,
            Selection {
                kind: Kind::Wallpaper,
                id: "wallpaper".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Wallpapers);
        selected(
            &mut app,
            Selection {
                kind: Kind::Module,
                id: "top/right/2".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Module);
        assert_eq!(
            app.nav.module,
            Some(Slot {
                bar: Bar::Top,
                list: "right",
                index: 2
            })
        );
        selected(
            &mut app,
            Selection {
                kind: Kind::Bar,
                id: "dock".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Dock);
        assert!(app.nav.module.is_none());
        assert_eq!(app.docs.version, revision);
    }

    #[test]
    fn material_preview_includes_projection_and_retains_the_durable_base() {
        let ui = Ui::new().unwrap();
        for (field, value) in [
            ("clarity", serde_json::json!(0.4)),
            ("blur", serde_json::json!(24)),
            ("refraction", serde_json::json!(0.8)),
            ("highlight", serde_json::json!(0.7)),
            ("tint", serde_json::json!("#987654")),
        ] {
            let mut app = crate::goldens::app(true, &ui);
            let before = snapshot(&app);
            let mut ops = vec![Op {
                doc: DocId::Config,
                path: format!("/look/material/{field}"),
                value: Some(value.clone()),
            }];
            ops.extend(crate::look::projection_after(&app.docs, &ops));
            app.docs.live(&ops).unwrap();
            let first = preview_documents(&before, &app);
            assert!(
                first.iter().any(|d| d.document == wire::Document::Config
                    && d.ops.iter().any(|o| o.value == value)),
                "{field}"
            );
            assert!(
                first.iter().any(|d| d.document == wire::Document::Bars),
                "{field}"
            );
            for d in &first {
                let raw = &before
                    .iter()
                    .find(|(id, _)| document(*id) == d.document)
                    .unwrap()
                    .1;
                assert_eq!(d.base_revision, wire::revision(raw));
                assert!(d.ops.iter().all(|o| o.valid(d.document)));
            }
            assert_eq!(app.docs.undo_count(), 0);
            app.docs.commit_live("Material").unwrap();
            assert_eq!(app.docs.undo_count(), 1);
            app.docs.undo().unwrap();
            assert_eq!(snapshot(&app), before);
        }
    }

    #[test]
    fn chip_undo_uses_the_announced_step_and_rejects_stale_history() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.docs
            .edit(
                "Blur",
                &[Op {
                    doc: DocId::Config,
                    path: "/look/material/blur".into(),
                    value: Some(serde_json::json!(24)),
                }],
            )
            .unwrap();
        let guard = UndoGuard {
            step: 12,
            count: app.docs.undo_count(),
            version: app.docs.version,
        };
        assert!(guard.matches(&app, 12));
        assert!(!guard.matches(&app, 11));
        app.docs.undo().unwrap();
        assert!(!guard.matches(&app, 12));
        app.docs
            .edit(
                "Other step",
                &[Op {
                    doc: DocId::Config,
                    path: "/look/material/blur".into(),
                    value: Some(serde_json::json!(12)),
                }],
            )
            .unwrap();
        assert_eq!(guard.count, app.docs.undo_count());
        assert!(
            !guard.matches(&app, 12),
            "a new branch with the same count is still a different step"
        );
    }

    #[test]
    fn rows_and_widget_pages_resolve_lifts_from_geometry_and_instance_identity() {
        let mut ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State {
            page: Page::Widgets,
            ..Default::default()
        });
        let built = crate::panel::build(&app, &mut ui.text);
        app.frame.targets = built.targets;
        let row = app
            .frame
            .targets
            .iter()
            .find(|t| matches!(&t.action, Action::SelectWidget(Some(id)) if id == "clock-1"))
            .unwrap();
        let rect = row.visible.unwrap();
        let pointer = mypaper_ui::Point::new(rect.left + 4.0, rect.top + 4.0);
        assert!(app.input.hover.is_none());
        assert_eq!(
            hovered_widget(&app, Some(pointer)).as_deref(),
            Some("clock-1")
        );
        assert_eq!(hovered_widget(&app, None), None);
        app.nav.widget = Some("clock-1".into());
        app.panel.as_mut().unwrap().page = Page::Widget;
        assert_eq!(page_selection(&app).unwrap().id, "clock-1");
        app.panel.as_mut().unwrap().page = Page::Root;
        assert_eq!(page_selection(&app), None);
    }
}
