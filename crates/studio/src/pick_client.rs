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
    choice_preview: Option<ChoicePreview>,
    color_open: bool,
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

struct ChoicePreview {
    action: Action,
    version: u64,
    display: usize,
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
    fn finish_choice(&mut self, app: &App) {
        if self.choice_preview.take().is_none() { return; }
        let Some(shell) = self.shell else { return };
        self.sequence += 1;
        let message = if matches!(app.docs.saved, crate::docs::Saved::Failed(_)) {
            Message::PreviewCancel { session: self.session, sequence: self.sequence }
        } else {
            Message::PreviewCommit {
                session: self.session, sequence: self.sequence,
                revisions: snapshot(app).iter().map(|(id, raw)| wire::Revision {
                    document: document(*id), revision: wire::revision(raw),
                }).collect(),
            }
        };
        wire::send(shell, self.endpoint.hwnd(), &message);
    }

    fn stream_preview(&mut self, app: &App) {
        if self.color_open {
            return;
        }
        let Some(shell) = self.shell else { return };
        if let Some(crate::app::Drag::Slider { edit, .. }) = &app.drag {
            self.choice_preview = None;
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
    preview_difference(before, &snapshot(app))
}

fn preview_difference(before: &[(DocId, Value); 3], after: &[(DocId, Value); 3]) -> Vec<wire::PreviewDocument> {
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
                    &format!("{path}/{}", crate::docs::token(key)),
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
            let changed = &after.iter().find(|(changed, _)| changed == id)?.1;
            diff(document, "", raw, changed, &mut ops);
            (!ops.is_empty() && ops.len() <= 32).then(|| wire::PreviewDocument {
                document,
                base_revision: wire::revision(raw),
                ops,
            })
        })
        .collect()
}

struct ColorDrag {
    documents: Vec<wire::PreviewDocument>,
    colors: Vec<(wire::Document, String)>,
    checkpoint: usize,
    label: String,
    last: String,
}

const COLOR_SAMPLE: &str = "#010203";
thread_local! { static COLOR_DRAG: RefCell<Option<ColorDrag>> = const { RefCell::new(None) }; }

fn color_ops(app: &App, edit: &crate::actions::Edit, color: &str) -> Vec<Op> {
    let mut ops = vec![Op {
        doc: edit.doc,
        path: edit.path.to_string(),
        value: Some(serde_json::json!(color)),
    }];
    if edit.doc == DocId::Config {
        // Match the panel's first tint choice when the current glass is completely clear.
        if edit.path.as_ref() == "/look/material/tint"
            && app
                .get(DocId::Config, "/look/material/clarity")
                .and_then(Value::as_f64)
                == Some(1.0)
            && app.get(DocId::Config, "/look/material/tint").is_none()
        {
            ops.push(Op {
                doc: DocId::Config,
                path: "/look/material/clarity".into(),
                value: Some(serde_json::json!(0.88)),
            });
        }
        ops.extend(crate::look::projection_after(&app.docs, &ops));
    }
    ops
}

impl ColorDrag {
    fn new(app: &App, edit: &crate::actions::Edit, initial: u32) -> Option<Self> {
        let ops = color_ops(app, edit, COLOR_SAMPLE);
        let alternative = color_ops(app, edit, "#040506");
        let documents: Vec<_> = snapshot(app)
            .into_iter()
            .filter_map(|(id, raw)| {
                let document = document(id);
                let ops: Vec<_> = ops
                    .iter()
                    .filter(|op| op.doc == id)
                    .map(|op| wire::PreviewOp {
                        path: op.path.clone(),
                        value: op.value.clone().unwrap_or(Value::Null),
                    })
                    .filter(|op| op.valid(document))
                    .collect();
                (!ops.is_empty()).then(|| wire::PreviewDocument {
                    document,
                    base_revision: wire::revision(&raw),
                    ops,
                })
            })
            .collect();
        // Two colors reveal which projected fields follow this picker. A family's own tint
        // remains constant even when it happens to equal the sample color.
        let mut colors = Vec::new();
        for (id, _) in snapshot(app) {
            if let Some(d) = documents.iter().find(|d| d.document == document(id)) {
                for op in &d.ops {
                    let other = alternative
                        .iter()
                        .find(|o| o.doc == id && o.path == op.path)
                        .and_then(|o| o.value.as_ref())
                        .unwrap_or(&Value::Null);
                    if other != &op.value {
                        colors.push((d.document, op.path.clone()));
                    }
                }
            }
        }
        (!documents.is_empty()).then(|| Self {
            documents,
            colors,
            checkpoint: app.docs.undo_count(),
            label: edit.label.to_string(),
            last: color_hex(initial),
        })
    }

    fn values(&self, color: &str) -> Vec<wire::PreviewDocument> {
        let mut documents = self.documents.clone();
        for d in &mut documents {
            for op in &mut d.ops {
                if self
                    .colors
                    .iter()
                    .any(|(doc, path)| *doc == d.document && *path == op.path)
                {
                    op.value = serde_json::json!(color);
                }
            }
        }
        documents
    }
}

fn color_hex(color: u32) -> String {
    mypaper_config::Color([color as u8, (color >> 8) as u8, (color >> 16) as u8]).hex()
}

fn publish_color(color: u32) {
    COLOR_DRAG.with(|drag| {
        let Ok(mut drag) = drag.try_borrow_mut() else {
            return;
        };
        let Some(drag) = drag.as_mut() else { return };
        let color = color_hex(color);
        if drag.last == color {
            return;
        }
        drag.last = color.clone();
        let documents = drag.values(&color);
        CLIENT.with(|client| {
            let mut client = client.borrow_mut();
            let Some(client) = client.as_mut() else {
                return;
            };
            let Some(shell) = client.shell else { return };
            client.sequence += 1;
            wire::send(
                shell,
                client.endpoint.hwnd(),
                &Message::Preview {
                    session: client.session,
                    sequence: client.sequence,
                    documents: documents.clone(),
                },
            );
            client.preview = Some(PreviewDrag {
                documents,
                checkpoint: drag.checkpoint,
                label: drag.label.clone(),
            });
        });
    });
}

struct ColorSession {
    accepted: bool,
}
impl Drop for ColorSession {
    fn drop(&mut self) {
        COLOR_DRAG.with(|drag| {
            drag.borrow_mut().take();
        });
        CLIENT.with(|client| {
            if let Some(client) = client.borrow_mut().as_mut() {
                client.color_open = false;
                if !self.accepted {
                    client.preview = None;
                    client.sequence += 1;
                    if let Some(shell) = client.shell {
                        wire::send(
                            shell,
                            client.endpoint.hwnd(),
                            &Message::PreviewCancel {
                                session: client.session,
                                sequence: client.sequence,
                            },
                        );
                    }
                }
            }
        });
    }
}

/// Native RGB drags stream only IPC. The caller's existing `set` saves once after OK.
pub fn choose_color(
    app: &App,
    edit: &crate::actions::Edit,
    picker: &mut windows::Win32::UI::Controls::Dialogs::CHOOSECOLORW,
) -> bool {
    use windows::Win32::UI::Controls::Dialogs::{CC_ENABLEHOOK, ChooseColorW};
    let drag = ColorDrag::new(app, edit, picker.rgbResult.0);
    let connected = CLIENT.with(|c| c.borrow().as_ref().is_some_and(|c| c.shell.is_some()));
    if !connected || drag.is_none() {
        // SAFETY: the caller owns the complete dialog structure and custom-color buffer.
        return unsafe { ChooseColorW(picker) }.as_bool();
    }
    COLOR_DRAG.with(|slot| *slot.borrow_mut() = drag);
    CLIENT.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            c.stream_preview(app);
            c.color_open = true;
        }
    });
    let mut session = ColorSession { accepted: false };
    picker.Flags |= CC_ENABLEHOOK;
    picker.lpfnHook = Some(color_hook);
    // SAFETY: synchronous native dialog; the hook stores only owned values, never an App pointer.
    let accepted = unsafe { ChooseColorW(picker) }.as_bool();
    if accepted {
        publish_color(picker.rgbResult.0);
    }
    session.accepted = accepted;
    accepted
}

unsafe extern "system" fn color_hook(
    window: HWND,
    message: u32,
    _: windows::Win32::Foundation::WPARAM,
    _: windows::Win32::Foundation::LPARAM,
) -> usize {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDlgItemInt, WM_COMMAND, WM_HSCROLL, WM_MOUSEMOVE, WM_PAINT, WM_VSCROLL,
    };
    if matches!(
        message,
        WM_COMMAND | WM_HSCROLL | WM_MOUSEMOVE | WM_PAINT | WM_VSCROLL
    ) {
        // RGB edit IDs are COLOR_RED/GREEN/BLUE in the Windows SDK's ColorDlg.h.
        let values = [706, 707, 708].map(|id| {
            let mut translated = windows::core::BOOL::default();
            // SAFETY: read-only queries of controls in the native dialog invoking this hook.
            let value = unsafe { GetDlgItemInt(window, id, Some(&mut translated), false) };
            (translated.as_bool() && value <= 255).then_some(value)
        });
        if let [Some(r), Some(g), Some(b)] = values {
            publish_color(r | (g << 8) | (b << 16));
        }
    }
    0
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

/// Shows a chooser's action on shell-owned desktop surfaces without editing documents.
/// Call after the chooser's hover/focus delay; clicking still runs the normal action.
/// Returns false for unsupported actions (including Windows looks) or an absent shell.
pub fn preview_action(app: &App, action: &Action) -> bool {
    if app.fixture || app.drag.is_some() {
        return false;
    }
    connect(app);
    if CLIENT.with(|client| client.borrow().as_ref().is_some_and(|client| {
        client.choice_preview.as_ref().is_some_and(|choice| {
            choice.action == *action && choice.version == app.docs.version && choice.display == app.nav.display
        })
    })) {
        return true;
    }
    let Some(documents) = choice_documents(app, action) else {
        end_preview();
        return false;
    };
    CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        let Some(client) = client.as_mut() else { return false };
        let Some(shell) = client.shell.filter(|_| client.begun && !client.color_open) else { return false };
        client.sequence += 1;
        let message = if documents.is_empty() {
            Message::PreviewCancel { session: client.session, sequence: client.sequence }
        } else {
            Message::Preview { session: client.session, sequence: client.sequence, documents: documents.clone() }
        };
        if !wire::send(shell, client.endpoint.hwnd(), &message) {
            return false;
        }
        client.choice_preview = Some(ChoicePreview { action: action.clone(), version: app.docs.version, display: app.nav.display });
        true
    })
}

/// Restores the saved desktop after leaving a chooser, Esc, or navigation.
/// Safe to call repeatedly; ending a preview never discards a committed pick.
pub fn end_preview() {
    CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        let Some(client) = client.as_mut() else { return };
        if client.choice_preview.take().is_some() && let Some(shell) = client.shell {
            client.sequence += 1;
            wire::send(shell, client.endpoint.hwnd(), &Message::PreviewCancel {
                session: client.session, sequence: client.sequence,
            });
        }
    });
}

/// Builds exactly the shell portion of a pick on owned JSON copies, never through Docs::edit.
fn choice_documents(app: &App, action: &Action) -> Option<Vec<wire::PreviewDocument>> {
    use serde_json::json;
    let op = |doc, path: String, value| Op { doc, path, value };
    let mut ops = match action {
        Action::SetStyle(style) => {
            let mut ops = vec![op(DocId::Config, "/look/style".into(), style.map(|s| json!(s)))];
            for field in ["clarity", "blur", "refraction", "highlight"] {
                ops.push(op(DocId::Config, format!("/look/material/{field}"), None));
            }
            // A style's signature includes its shape, even over a customized saved look.
            let radius = style.map(|style| {
                let style = match style {
                    mypaper_config::look::Style::Liquid => mypaper_ui::Style::Liquid,
                    mypaper_config::look::Style::Oneui => mypaper_ui::Style::OneUi,
                    mypaper_config::look::Style::Solid => mypaper_ui::Style::Solid,
                    mypaper_config::look::Style::Fluent => mypaper_ui::Style::Fluent,
                    mypaper_config::look::Style::Mono => mypaper_ui::Style::Mono,
                };
                json!(mypaper_ui::Theme::new(style, mypaper_ui::Scheme::Dark, mypaper_ui::theme::SYSTEM_BLUE).radius.surface)
            });
            ops.push(op(DocId::Config, "/look/shape/corners".into(), radius));
            ops
        }
        Action::Preset(id) => {
            let mut ops = vec![op(DocId::Bars, "/preset".into(), Some(json!(id)))];
            for bar in ["top", "dock"] {
                for field in ["design", "enabled"] {
                    ops.push(op(DocId::Bars, format!("/{bar}/{field}"), None));
                }
            }
            ops
        }
        Action::FirstRunPick(index) => {
            let (_, preset, _) = crate::pages::first_run::panel_starter(*index)?;
            let mut ops = vec![op(DocId::Bars, "/preset".into(), Some(json!(preset)))];
            for bar in ["top", "dock"] {
                for field in ["design", "enabled"] {
                    ops.push(op(DocId::Bars, format!("/{bar}/{field}"), None));
                }
            }
            if matches!(preset, "macos" | "islands") {
                ops.push(op(DocId::Bars, "/dock/enabled".into(), Some(json!(true))));
            }
            if matches!(app.overlay, Some(crate::app::Overlay::FirstRun { taskbar: 1 | 2, .. })) {
                for bar in ["top", "dock"] {
                    ops.push(op(DocId::Bars, format!("/{bar}/enabled"), Some(json!(false))));
                }
            }
            ops
        }
        Action::Set(edit, value) if edit.doc == DocId::Studio && edit.path.as_ref() == "/panel/bar-preset" => {
            let base = mypaper_bars::BarsConfig::parse(&json!({"preset":value}).to_string()).ok()?;
            let base = serde_json::to_value(base).ok()?;
            let mut ops = vec![op(DocId::Bars, crate::panel::editors::bar_path(app, "/preset"), Some(value.clone()))];
            for bar in ["top", "dock"] {
                for field in ["design", "enabled"] {
                    let path = format!("/{bar}/{field}");
                    ops.push(op(DocId::Bars, crate::panel::editors::bar_path(app, &path), base.pointer(&path).cloned()));
                }
            }
            ops
        }
        Action::Set(edit, value) if matches!(edit.doc, DocId::Bars | DocId::Widgets | DocId::Config) => {
            let candidate = wire::PreviewOp { path: edit.path.to_string(), value: value.clone() };
            let appearance = edit.doc == DocId::Widgets && edit.path.ends_with("/appearance")
                && (value.is_object() || value.is_null());
            if !appearance && !candidate.valid(document(edit.doc)) { return None; }
            vec![op(edit.doc, edit.path.to_string(), (!value.is_null()).then(|| value.clone()))]
        }
        _ => return None,
    };
    ops.extend(crate::look::projection_after(&app.docs, &ops));
    let before = snapshot(app);
    let mut after = before.clone();
    for operation in &ops {
        let (_, raw) = after.iter_mut().find(|(id, _)| *id == operation.doc)?;
        crate::docs::apply_op(raw, operation);
    }
    // Use the owning parsers before sending; an incomplete/invalid face never reaches the host.
    for ((id, old), (_, new)) in before.iter().zip(&after) {
        if old != new && (app.docs.error(*id).is_some() || id.parse(&new.to_string()).is_err()) {
            return None;
        }
    }
    Some(preview_difference(&before, &after))
}

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
                choice_preview: None,
                color_open: false,
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
            client.choice_preview = None;
            client.color_open = false;
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
        if client.choice_preview.is_some() && client.observed_version != app.docs.version && app.drag.is_none() {
            client.finish_choice(app);
        }
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
    // A caption's WM_NCMOUSELEAVE can follow the client's move and clear Input::pointer.
    // Sample our HWND directly so that transition cannot lose the row's desktop highlight.
    highlight(hovered_widget(app, panel_pointer(app)));
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

pub(crate) fn panel_pointer(app: &App) -> Option<mypaper_ui::Point> {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::ScreenToClient,
        UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint},
    };
    let window = app.window?;
    let mut point = POINT::default();
    // SAFETY: read-only pointer queries. Never treat a covered panel or another HWND as hover.
    unsafe {
        if GetCursorPos(&mut point).is_err()
            || WindowFromPoint(point) != window
            || !ScreenToClient(window, &mut point).as_bool()
        {
            return None;
        }
    }
    Some(mypaper_ui::Point::new(point.x as f32, point.y as f32))
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
        client.finish_choice(app);
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
                let sent = wire::send(
                    shell,
                    client.endpoint.hwnd(),
                    &Message::Highlight { id: id.clone() },
                );
                if !sent {
                    return;
                }
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
    if key == 0x1b {
        end_preview();
    }
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
                let guard = CLIENT.with(|c| {
                    c.borrow_mut().as_mut().and_then(|c| {
                        c.undo_step.filter(|g| g.step == step)?;
                        c.undo_step.take()
                    })
                });
                let Some(guard) = guard else { continue };
                if guard.matches(app, step) {
                    app.run(Action::Undo);
                }
                CLIENT.with(|c| {
                    if let Some(c) = c.borrow_mut().as_mut()
                        && let Some(shell) = c.shell
                    {
                        c.sequence += 1;
                        wire::send(
                            shell,
                            c.endpoint.hwnd(),
                            &Message::PreviewCancel {
                                session: c.session,
                                sequence: c.sequence,
                            },
                        );
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
                let revision =
                    resize_widget(app, selection.clone(), before_cell, before_size, cell, size);
                CLIENT.with(|c| {
                    if let Some(c) = c.borrow().as_ref()
                        && let Some(shell) = c.shell
                    {
                        wire::send(
                            shell,
                            c.endpoint.hwnd(),
                            &Message::ResizeResult {
                                selection,
                                cell,
                                size,
                                revision,
                            },
                        );
                    }
                });
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
) -> Option<u64> {
    if selection.kind != Kind::Widget || (cell, size) == (before_cell, before_size) {
        return None;
    }
    let layout = crate::pages::widgets::layout(app);
    let Some((index, instance)) = layout
        .instances
        .iter()
        .enumerate()
        .find(|(_, i)| i.instance == selection.id)
    else {
        return None;
    };
    if instance.cell != before_cell || instance.size != before_size {
        app.alert("This widget changed while resizing. Try again.");
        return None;
    }
    let Some(widget) = app.registry.get(&instance.widget) else {
        return None;
    };
    if !widget.manifest.resizable
        || widget.manifest.fit_size(size.map(f64::from), [u32::MAX; 2]) != Some(size)
    {
        return None;
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
    let checkpoint = app.docs.undo_count();
    app.apply(&label, ops);
    (app.docs.undo_count() > checkpoint).then(|| wire::revision(&app.docs.doc(DocId::Widgets).raw))
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

    #[test]
    fn chooser_previews_include_whole_styles_without_touching_documents_or_undo() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let app = crate::goldens::app(true, &ui);
        let before = snapshot(&app);
        let undo = app.docs.undo_count();
        let version = app.docs.version;
        for style in mypaper_config::look::Style::ALL {
            let documents = choice_documents(&app, &Action::SetStyle(Some(style))).unwrap();
            assert!(documents.iter().all(|d| d.ops.iter().all(|op| op.valid(d.document))));
            for document in documents {
                let (_, raw) = before.iter().find(|(id, _)| self::document(*id) == document.document).unwrap();
                let effective = wire::patched(raw, &document.ops).unwrap();
                if document.document == wire::Document::Config {
                    assert_eq!(effective["look"]["style"], serde_json::json!(style));
                    assert!(effective["look"]["shape"]["corners"].is_number());
                }
            }
        }
        assert_eq!(snapshot(&app), before);
        assert_eq!(app.docs.undo_count(), undo);
        assert_eq!(app.docs.version, version);
        assert!(choice_documents(&app, &Action::WindowsLook(0)).is_none());
    }

    #[test]
    fn bar_choices_preview_each_display_without_changing_its_neighbors() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        if let Some(first) = app.monitors.first().cloned() {
            let mut second = first;
            second.device = r"\\.\DISPLAY2".into();
            second.primary = false;
            app.monitors.push(second);
        }
        app.nav.display = app.monitors.len().saturating_sub(1);
        let action = Action::Set(crate::actions::Edit::new(DocId::Studio, "/panel/bar-preset", "Design"), serde_json::json!("windows11"));
        let preview = choice_documents(&app, &action).unwrap();
        let bars = preview.iter().find(|d| d.document == wire::Document::Bars).unwrap();
        assert!(bars.ops.iter().all(|op| op.path.starts_with("/overrides/")));
        let raw = &app.docs.doc(DocId::Bars).raw;
        let changed = wire::patched(raw, &bars.ops).unwrap();
        assert_eq!(changed.get("dock"), raw.get("dock"));
        let parsed = mypaper_bars::BarsConfig::parse(&changed.to_string()).unwrap();
        let second = parsed.for_monitor(r"\\.\DISPLAY2", false).unwrap();
        assert!(!second.top.enabled);
        assert_eq!(second.dock.design, mypaper_bars::config::DockDesign::Windows11);
    }

    #[test]
    fn starter_preview_uses_the_declared_bar_pair_and_taskbar_step_without_a_save() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        let before = snapshot(&app);
        for index in 0..crate::pages::first_run::panel_starters().len() {
            let (_, preset, _) = crate::pages::first_run::panel_starter(index).unwrap();
            let docs = choice_documents(&app, &Action::FirstRunPick(index)).unwrap();
            assert!(docs.iter().all(|doc| doc.document == wire::Document::Bars));
            let ops = docs.iter().find(|doc| doc.document == wire::Document::Bars).map_or(&[][..], |doc| doc.ops.as_slice());
            let raw = wire::patched(&app.docs.doc(DocId::Bars).raw, ops).unwrap();
            assert_eq!(raw["preset"], preset);
        }
        app.overlay = Some(crate::app::Overlay::FirstRun { step: 0, starter: 0, taskbar: 1 });
        let docs = choice_documents(&app, &Action::FirstRunPick(2)).unwrap();
        let bars = docs.iter().find(|doc| doc.document == wire::Document::Bars).unwrap();
        let raw = wire::patched(&app.docs.doc(DocId::Bars).raw, &bars.ops).unwrap();
        let bars = mypaper_bars::BarsConfig::parse(&raw.to_string()).unwrap();
        assert!(!bars.top.enabled && !bars.dock.enabled);
        assert_eq!(snapshot(&app), before);
        assert!(choice_documents(&app, &Action::FirstRunPick(usize::MAX)).is_none());
    }
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
            choice_preview: None,
            color_open: false,
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
    fn color_dialog_builds_only_transient_material_values_without_editing_documents() {
        let ui = Ui::new().unwrap();
        let app = crate::goldens::app(true, &ui);
        let before = snapshot(&app);
        for edit in [
            crate::actions::Edit::new(DocId::Config, "/look/material/tint", "Tint"),
            crate::actions::Edit::new(DocId::Widgets, "/instances/0/appearance/tint", "Tint"),
            crate::actions::Edit::new(DocId::Bars, "/dock/appearance/tint", "Tint"),
        ] {
            let drag = ColorDrag::new(&app, &edit, 0).unwrap();
            let red = drag.values("#ff0000");
            let blue = drag.values("#0000ff");
            assert!(
                red.iter()
                    .flat_map(|d| &d.ops)
                    .any(|o| o.value.as_str() == Some("#ff0000"))
            );
            assert!(
                blue.iter()
                    .flat_map(|d| &d.ops)
                    .any(|o| o.value.as_str() == Some("#0000ff"))
            );
            assert!(
                blue.iter()
                    .all(|d| d.ops.iter().all(|op| op.valid(d.document)))
            );
        }
        assert_eq!(snapshot(&app), before);
        assert_eq!(app.docs.undo_count(), 0);
        assert!(
            ColorDrag::new(
                &app,
                &crate::actions::Edit::new(DocId::Config, "/look/windows/border", "Border"),
                0
            )
            .is_none()
        );
    }

    #[test]
    fn color_dialog_preserves_a_family_tint_that_matches_the_projection_sample() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.docs
            .edit(
                "Own bar tint",
                &[Op {
                    doc: DocId::Config,
                    path: "/look/bars/material/tint".into(),
                    value: Some(serde_json::json!(COLOR_SAMPLE)),
                }],
            )
            .unwrap();
        let before = snapshot(&app);
        let drag = ColorDrag::new(
            &app,
            &crate::actions::Edit::new(DocId::Config, "/look/material/tint", "Tint"),
            0,
        )
        .unwrap();
        let documents = drag.values("#ff0000");
        let tint = |document| {
            documents
                .iter()
                .find(|d| d.document == document)
                .unwrap()
                .ops
                .iter()
                .find(|o| o.path == "/appearance/tint")
                .unwrap()
                .value
                .as_str()
                .unwrap()
        };
        assert_eq!(tint(wire::Document::Bars), COLOR_SAMPLE);
        assert_eq!(tint(wire::Document::Widgets), "#ff0000");
        assert_eq!(snapshot(&app), before);
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
    fn chip_requests_route_without_keyboard_input_and_old_requests_leave_the_new_step_alone() {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        let before = snapshot(&app);
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
        let shell = wire::Endpoint::new(true).unwrap();
        let endpoint = wire::Endpoint::new(false).unwrap();
        let to = endpoint.hwnd();
        let guard = UndoGuard {
            step: 12,
            count: app.docs.undo_count(),
            version: app.docs.version,
        };
        CLIENT.with(|c| {
            *c.borrow_mut() = Some(Client {
                endpoint,
                shell: Some(shell.hwnd()),
                begun: true,
                panel: HWND::default(),
                highlighted: None,
                observed: snapshot(&app),
                observed_version: app.docs.version,
                undo_chain: UndoChain::default(),
                session: 123,
                sequence: 0,
                preview: None,
                choice_preview: None,
                color_open: false,
                undo_serial: 12,
                undo_step: Some(guard),
                page_selection: None,
                desktop_selected: false,
            })
        });
        // SAFETY: read-only foreground query; neither hidden endpoint is activated.
        let foreground = unsafe { GetForegroundWindow() };
        wire::send(to, shell.hwnd(), &Message::UndoRequest { step: 11 });
        on_key(&mut app, wire::PICK_EVENT);
        assert_eq!(app.docs.undo_count(), 1);
        assert!(CLIENT.with(|c| c.borrow().as_ref().unwrap().undo_step.is_some()));
        wire::send(to, shell.hwnd(), &Message::UndoRequest { step: 12 });
        on_key(&mut app, wire::PICK_EVENT);
        assert_eq!(snapshot(&app), before);
        assert!(matches!(
            shell.next(),
            Some((
                _,
                Message::PreviewCancel {
                    session: 123,
                    sequence: 1
                }
            ))
        ));
        assert!(matches!(shell.next(), Some((_, Message::UndoDismiss))));
        wire::send(to, shell.hwnd(), &Message::UndoRequest { step: 12 });
        on_key(&mut app, wire::PICK_EVENT);
        assert_eq!(app.docs.undo_count(), 0);
        // SAFETY: the assertion verifies that routing never changed activation.
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        CLIENT.with(|c| *c.borrow_mut() = None);
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
