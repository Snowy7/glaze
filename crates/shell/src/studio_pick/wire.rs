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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum Message {
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
    ResizeWidget {
        selection: Selection,
        before_cell: [u32; 2],
        before_size: [u32; 2],
        cell: [u32; 2],
        size: [u32; 2],
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
    if bytes.len() > MAX_BYTES {
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
            && (data.cbData as usize) <= MAX_BYTES
            && !data.lpData.is_null()
        {
            // SAFETY: validated COPYDATA length and pointer; parse to owned data now.
            let bytes = unsafe {
                std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize)
            };
            if let Ok(message) = serde_json::from_slice(bytes) {
                // SAFETY: queue lives until this window is destroyed on this same thread.
                let queue = unsafe { &*queue };
                let mut messages = queue.messages.borrow_mut();
                if messages.len() < 64 {
                    messages.push_back((HWND(wp.0 as _), message));
                    if let Some(window) = queue.wake.get() {
                        mypaper_ui::queue_event(window, mypaper_ui::Event::Key(PICK_EVENT));
                        // SAFETY: wakes the peer's own UI loop; no keyboard input or activation.
                        let _ =
                            unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
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
