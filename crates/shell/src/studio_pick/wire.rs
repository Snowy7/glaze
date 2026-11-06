//! Versioned, bounded Studio↔shell selection messages. Both executables link this module.
//! COPYDATA is copied in the window procedure and handled later, never re-entrantly.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, w};

pub const SHELL_CLASS: windows::core::PCWSTR = w!("mypaper.shell.studio-pick.v1");
const TAG: usize = 0x4d505331;
const MAX_BYTES: usize = 16 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
/// Semantic wake queued directly into mypaper-ui, never sent to the keyboard queue.
pub const PICK_EVENT: u16 = 0xffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Widget,
    Bar,
    Module,
    Wallpaper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub kind: Kind,
    pub id: String,
    /// Windows display device name (stable across monitor enumeration order).
    pub display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Document {
    Config,
    Bars,
    Widgets,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewOp {
    pub path: String,
    /// `null` removes an override, revealing the layer underneath.
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewDocument {
    pub document: Document,
    pub base_revision: u64,
    pub ops: Vec<PreviewOp>,
}

/// Complete, parser-checked shell state for History. Never persisted by the receiver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotDocument {
    pub document: Document,
    pub base_revision: u64,
    pub raw: serde_json::Value,
}

impl SnapshotDocument {
    pub fn valid(&self) -> bool {
        if !self.raw.is_object() { return false; }
        let text = self.raw.to_string();
        match self.document {
            Document::Config => mypaper_config::Config::parse(&text).is_ok(),
            Document::Bars => mypaper_bars::BarsConfig::parse(&text).is_ok(),
            Document::Widgets => mypaper_widgets::layout::Layout::parse(&text).is_ok(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotTheme {
    pub raw: serde_json::Value,
    pub dir: Option<std::path::PathBuf>,
}

impl SnapshotTheme {
    /// Parse frozen JSON only: later theme-file edits cannot change this preview.
    pub fn loaded(&self) -> anyhow::Result<Option<mypaper_theme::LoadedTheme>> {
        if self.raw.is_null() { return Ok(None); }
        mypaper_theme::LoadedTheme::parse(
            self.dir.clone().unwrap_or_default(), &self.raw.to_string(),
        ).map(Some)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Revision {
    pub document: Document,
    pub revision: u64,
}

impl PreviewOp {
    /// Only shell-owned appearance, design, face and geometry can be previewed.
    /// Actions, pins, sources and Windows styling never cross this boundary.
    pub fn valid(&self, document: Document) -> bool {
        if !self.path.starts_with('/') || self.path.len() > 256 {
            return false;
        }
        let path: Vec<_> = self.path.trim_start_matches('/').split('/').collect();
        let path = match (document, path.as_slice()) {
            (Document::Bars, ["overrides", display, rest @ ..])
                if !display.is_empty() && display.len() <= 128 => rest,
            _ => path.as_slice(),
        };
        let optional = |valid| self.value.is_null() || valid;
        match (document, path) {
            (Document::Config, ["look", "style"] | ["look", "bars" | "widgets" | "menus", "style"]) => {
                return optional(serde_json::from_value::<mypaper_config::look::Style>(self.value.clone()).is_ok());
            }
            (Document::Config, ["look", "shape", "corners"]) => {
                return optional(self.value.as_f64().is_some_and(|v| (0.0..=48.0).contains(&v)));
            }
            (Document::Bars, ["preset"]) => {
                return optional(serde_json::from_value::<mypaper_bars::config::Preset>(self.value.clone()).is_ok());
            }
            (Document::Bars, ["top", "design"]) => {
                return optional(serde_json::from_value::<mypaper_bars::config::TopDesign>(self.value.clone()).is_ok());
            }
            (Document::Bars, ["dock", "design"]) => {
                return optional(serde_json::from_value::<mypaper_bars::config::DockDesign>(self.value.clone()).is_ok());
            }
            (Document::Bars, ["top" | "dock", "enabled"]) => return optional(self.value.is_boolean()),
            (Document::Bars, ["look", "style"] | ["top" | "dock", "look", "style"]) => {
                return optional(serde_json::from_value::<mypaper_bars::style::NativeStyle>(self.value.clone()).is_ok());
            }
            (Document::Bars, ["look", "family"]) => {
                return optional(serde_json::from_value::<mypaper_bars::style::Family>(self.value.clone()).is_ok());
            }
            (Document::Bars, ["appearance", field] | ["top" | "dock", "appearance", field])
            | (Document::Widgets, ["appearance", field] | ["instances", _, "appearance", field]) => {
                return matches!(*field, "surface" | "tint" | "opacity" | "blur" | "radius" | "border" | "text" | "accent" | "font" | "scale" | "shadow" | "style")
                    && optional(serde_json::from_value::<mypaper_theme::Appearance>(
                        serde_json::json!({*field: self.value.clone()})
                    ).is_ok_and(|a| a.validate().is_ok()));
            }
            (Document::Widgets, ["instances", index, "settings", field]) => {
                return index.parse::<usize>().is_ok()
                    && matches!(*field, "variant" | "face" | "layout" | "style" | "look" | "shape" | "display" | "frame")
                    && optional(self.value.as_str().is_some_and(|v| v.len() <= 128 && !v.chars().any(char::is_control)));
            }
            _ => {}
        }
        let field = match (document, path) {
            (Document::Config, ["look", "material", field])
            | (Document::Config, ["look", "bars" | "widgets" | "menus", "material", field]) => {
                *field
            }
            (Document::Bars, ["look", "glass", field])
            | (Document::Bars, ["top" | "dock", "look", "glass", field]) => *field,
            (Document::Bars, ["look", "tint"] | ["top" | "dock", "look", "tint"]) => "opacity",
            (Document::Bars, ["appearance", "tint"] | ["top" | "dock", "appearance", "tint"]) => {
                "color"
            }
            (Document::Bars, ["dock", "size"]) => "dock_size",
            (Document::Bars, ["top", "height"]) => "bar_height",
            (Document::Widgets, ["appearance", field])
            | (Document::Widgets, ["instances", _, "appearance", field]) => *field,
            (Document::Widgets, ["instances", _, "size", "0" | "1"]) => "widget_size",
            (Document::Widgets, ["instances", _, "cell", "0" | "1"]) => "cell",
            _ => return false,
        };
        let max = match field {
            "clarity" | "refraction" | "highlight" | "opacity" => 1.0,
            "blur" if document == Document::Widgets => 64.0,
            "blur" => 32.0,
            "tint" if document == Document::Bars => 1.0,
            "tint" | "color" => {
                return self.value.is_null()
                    || self.value.as_str().is_some_and(|s| {
                        (document == Document::Config && s == "auto")
                            || mypaper_config::Color::parse(s).is_some()
                    });
            }
            "dock_size" => 96.0,
            "bar_height" => 96.0,
            "widget_size" => 256.0,
            "cell" => 16384.0,
            _ => return false,
        };
        let min = match field {
            "dock_size" => 24.0,
            "bar_height" => 16.0,
            "widget_size" => 1.0,
            _ => 0.0,
        };
        self.value.is_null()
            || self.value.as_f64().is_some_and(|n| {
                n.is_finite()
                    && (min..=max).contains(&n)
                    && (!matches!(field, "widget_size" | "cell") || n.fract() == 0.0)
            })
    }
}

/// Applies a validated transient patch to an owned copy. There is no storage boundary here.
pub fn patched(raw: &serde_json::Value, ops: &[PreviewOp]) -> Option<serde_json::Value> {
    fn set(at: &mut serde_json::Value, path: &[&str], value: &serde_json::Value) -> Option<()> {
        let (key, rest) = path.split_first()?;
        if let Some(array) = at.as_array_mut() {
            let slot = array.get_mut(key.parse::<usize>().ok()?)?;
            if rest.is_empty() {
                *slot = value.clone();
            } else {
                set(slot, rest, value)?;
            }
        } else {
            let object = at.as_object_mut()?;
            if rest.is_empty() {
                if value.is_null() {
                    object.remove(*key);
                } else {
                    object.insert((*key).into(), value.clone());
                }
            } else {
                set(
                    object
                        .entry((*key).to_owned())
                        .or_insert_with(|| serde_json::json!({})),
                    rest,
                    value,
                )?;
            }
        }
        Some(())
    }
    let mut result = raw.clone();
    for op in ops {
        // Root replacements come only from validated SnapshotDocument, never PreviewOp::valid.
        if op.path.is_empty() {
            result = op.value.clone();
            continue;
        }
        let tokens: Vec<_> = op.path.strip_prefix('/')?.split('/')
            .map(|part| part.replace("~1", "/").replace("~0", "~"))
            .collect();
        let path: Vec<_> = tokens.iter().map(String::as_str).collect();
        set(&mut result, &path, &op.value)?;
    }
    Some(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
    /// Prime the existing preview machinery at saved values, without an override or save.
    WarmPreview,
    PreviewSnapshot {
        session: u64,
        sequence: u64,
        documents: Vec<SnapshotDocument>,
        theme: SnapshotTheme,
    },
    Preview {
        session: u64,
        sequence: u64,
        documents: Vec<PreviewDocument>,
    },
    PreviewCommit {
        session: u64,
        sequence: u64,
        revisions: Vec<Revision>,
    },
    PreviewDock {
        session: u64,
        sequence: u64,
        base_revision: u64,
        size: f64,
    },
    PreviewSaved {
        session: u64,
        sequence: u64,
        revision: u64,
    },
    PreviewCancel {
        session: u64,
        sequence: u64,
    },
    Begin {
        panel: isize,
    },
    End,
    Highlight {
        id: Option<String>,
    },
    ClearSelection,
    UndoStep {
        display: String,
        text: String,
    },
    UndoNotice {
        display: String,
        text: String,
        step: u64,
    },
    UndoRequest {
        step: u64,
    },
    UndoDismiss,
    ResizeWidget {
        selection: Selection,
        before_cell: [u32; 2],
        before_size: [u32; 2],
        cell: [u32; 2],
        size: [u32; 2],
    },
    ResizeResult {
        selection: Selection,
        cell: [u32; 2],
        size: [u32; 2],
        revision: Option<u64>,
    },
    MoveModule {
        selection: Selection,
        destination: super::module_drag::Slot,
        base_revision: u64,
    },
    MoveModuleResult {
        revision: Option<u64>,
    },
    Selected {
        selection: Selection,
    },
}

#[derive(Default)]
struct Queue {
    messages: RefCell<VecDeque<(HWND, Message)>>,
    wake: Cell<Option<HWND>>,
}

impl Queue {
    fn enqueue(&self, source: HWND, message: Message) -> bool {
        let mut messages = self.messages.borrow_mut();
        let preview = |m: &Message| match m {
            Message::Preview {
                session, sequence, ..
            }
            | Message::PreviewSnapshot { session, sequence, .. }
            | Message::PreviewDock {
                session, sequence, ..
            } => Some((*session, *sequence)),
            _ => None,
        };
        if let Some((session, sequence)) = preview(&message) {
            // A commit/cancel is a barrier: a subsequent drag must remain after it.
            if let Some((from, pending)) = messages.back_mut()
                && *from == source
                && let Some((old_session, old_sequence)) = preview(pending)
                && session == old_session
            {
                if sequence > old_sequence {
                    *pending = message;
                }
                return true;
            }
        }
        if messages.len() >= 64 {
            return false;
        }
        messages.push_back((source, message));
        true
    }
}

/// Formatting-independent revision of a raw document. Used for equality, never authentication.
pub fn revision(value: &impl Serialize) -> u64 {
    serde_json::to_vec(value)
        .unwrap_or_default()
        .into_iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

pub struct Endpoint {
    hwnd: HWND,
    queue: Rc<Queue>,
}

impl Endpoint {
    pub fn new(shell: bool) -> Result<Self> {
        let class_name = if shell {
            SHELL_CLASS
        } else {
            w!("mypaper.studio.pick-client.v1")
        };
        // SAFETY: class and procedure live for the process. The Rc below outlives the window.
        let hwnd = unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassExW(&class);
            CreateWindowExW(
                Default::default(),
                class_name,
                &HSTRING::from("Studio desktop picks"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .context("creating Studio pick endpoint")?
        };
        let queue = Rc::new(Queue::default());
        // SAFETY: the window owns one Rc, released in WM_NCDESTROY.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(queue.clone()) as isize) };
        Ok(Self { hwnd, queue })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub fn next(&self) -> Option<(HWND, Message)> {
        self.queue.messages.borrow_mut().pop_front()
    }
    pub fn wake_ui(&self, window: HWND) {
        self.queue.wake.set(Some(window));
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        // SAFETY: owned window, on its creating thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Bounded send; a hung peer cannot stall the shell indefinitely. Replies are sent later.
pub fn send(to: HWND, from: HWND, message: &Message) -> bool {
    let Ok(bytes) = serde_json::to_vec(message) else {
        return false;
    };
    let limit = if matches!(message, Message::PreviewSnapshot { .. }) { MAX_SNAPSHOT_BYTES } else { MAX_BYTES };
    if bytes.len() > limit {
        return false;
    }
    let data = COPYDATASTRUCT {
        dwData: TAG,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr().cast_mut().cast(),
    };
    let mut answer = 0;
    // SAFETY: synchronous COPYDATA copies these bytes before the bounded call returns.
    unsafe {
        SendMessageTimeoutW(
            to,
            WM_COPYDATA,
            WPARAM(from.0 as usize),
            LPARAM((&raw const data) as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            30,
            Some(&mut answer),
        )
        .0 != 0
            && answer == 1
    }
}

pub fn find_shell() -> Option<HWND> {
    // SAFETY: a read-only lookup of our message-only endpoint.
    unsafe { FindWindowExW(Some(HWND_MESSAGE), None, SHELL_CLASS, None).ok() }
}

pub fn process(hwnd: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: read-only handle queries; a stale window has process id 0.
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
    }
    pid
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: installed Rc pointer, or zero during window construction.
    let queue = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Queue;
    if message == WM_NCDESTROY && !queue.is_null() {
        // SAFETY: takes back exactly the window's strong reference.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Rc::from_raw(queue));
        }
    } else if message == WM_COPYDATA && !queue.is_null() && lp.0 != 0 {
        // SAFETY: Windows supplies COPYDATA and keeps its bytes valid during this call.
        let data = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
        if data.dwData == TAG
            && data.cbData > 0
            && (data.cbData as usize) <= MAX_SNAPSHOT_BYTES
            && !data.lpData.is_null()
        {
            // SAFETY: validated COPYDATA length and pointer; parse to owned data now.
            let bytes = unsafe {
                std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize)
            };
            if let Ok(message) = serde_json::from_slice::<Message>(bytes) {
                if bytes.len() > MAX_BYTES && !matches!(message, Message::PreviewSnapshot { .. }) {
                    return LRESULT(0);
                }
                // SAFETY: queue lives until this window is destroyed on this same thread.
                let queue = unsafe { &*queue };
                if queue.enqueue(HWND(wp.0 as _), message) {
                    if let Some(window) = queue.wake.get() {
                        mypaper_ui::queue_event(window, mypaper_ui::Event::Key(PICK_EVENT));
                        // SAFETY: wakes the peer's own UI loop; no keyboard input or activation.
                        let _ =
                            unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
                    } else {
                        // A posted wake bypasses the shell's sent-message coalescing delay.
                        // SAFETY: the endpoint is owned by this UI thread and is still alive.
                        let _ = unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) };
                    }
                    return LRESULT(1);
                }
            }
        }
        return LRESULT(0);
    }
    // SAFETY: default processing of the original message.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_snapshots_cross_copydata_without_relaxing_normal_message_limits() {
        let receiver = Endpoint::new(false).unwrap();
        let sender = Endpoint::new(false).unwrap();
        let documents = vec![SnapshotDocument {
            document: Document::Widgets,
            base_revision: 1,
            raw: serde_json::json!({"note": "x".repeat(MAX_BYTES * 2)}),
        }];
        let message = Message::PreviewSnapshot {
            session: 7, sequence: 1, documents,
            theme: SnapshotTheme { raw: serde_json::Value::Null, dir: None },
        };
        assert!(send(receiver.hwnd(), sender.hwnd(), &message));
        assert!(matches!(receiver.next(), Some((source, Message::PreviewSnapshot { documents, .. }))
            if source == sender.hwnd() && documents[0].raw["note"].as_str().unwrap().len() == MAX_BYTES * 2));
        assert!(!send(receiver.hwnd(), sender.hwnd(), &Message::Highlight { id: Some("x".repeat(MAX_BYTES * 2)) }));
        let oversized = Message::PreviewSnapshot {
            session: 7, sequence: 2,
            documents: vec![SnapshotDocument { document: Document::Widgets, base_revision: 1,
                raw: serde_json::json!({"note":"x".repeat(MAX_SNAPSHOT_BYTES)}) }],
            theme: SnapshotTheme { raw: serde_json::Value::Null, dir: None },
        };
        assert!(!send(receiver.hwnd(), sender.hwnd(), &oversized));
        assert!(receiver.next().is_none());
    }
    #[test]
    fn preview_queue_is_bounded_and_keeps_only_the_latest_value() {
        let queue = Queue::default();
        let source = HWND(1 as _);
        for sequence in 1..1000 {
            assert!(queue.enqueue(
                source,
                Message::PreviewDock {
                    session: 7,
                    sequence,
                    base_revision: 10,
                    size: 48.0
                }
            ));
        }
        assert_eq!(queue.messages.borrow().len(), 1);
        assert!(matches!(
            queue.messages.borrow().front(),
            Some((_, Message::PreviewDock { sequence: 999, .. }))
        ));
        for _ in 1..64 {
            assert!(queue.enqueue(source, Message::End));
        }
        assert!(!queue.enqueue(source, Message::End));
    }

    #[test]
    fn a_new_drag_never_coalesces_across_the_previous_commit() {
        let queue = Queue::default();
        let source = HWND(1 as _);
        let value = |sequence| Message::Preview {
            session: 7,
            sequence,
            documents: vec![PreviewDocument {
                document: Document::Config,
                base_revision: 10,
                ops: vec![PreviewOp {
                    path: "/look/material/blur".into(),
                    value: serde_json::json!(sequence),
                }],
            }],
        };
        queue.enqueue(source, value(1));
        queue.enqueue(
            source,
            Message::PreviewCommit {
                session: 7,
                sequence: 2,
                revisions: vec![],
            },
        );
        queue.enqueue(source, value(3));
        queue.enqueue(source, value(4));
        let messages = queue.messages.borrow();
        assert_eq!(messages.len(), 3);
        assert!(matches!(
            messages[0].1,
            Message::Preview { sequence: 1, .. }
        ));
        assert!(matches!(
            messages[1].1,
            Message::PreviewCommit { sequence: 2, .. }
        ));
        assert!(matches!(
            messages[2].1,
            Message::Preview { sequence: 4, .. }
        ));
    }
}
