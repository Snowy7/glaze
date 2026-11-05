//! A nonblocking inbox for core's tray menu. No commands or GPU work run in core's UI path.
use anyhow::Result;
use mypaper_bars::tray_menu::{self as wire, Item, Request};
use mypaper_ui::{Entry, Event, Menu, Outcome, SurfaceId, Ui};
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::*,
    },
    core::PCWSTR,
};

thread_local! { static REQUESTS: RefCell<Vec<(isize, Request)>> = const { RefCell::new(Vec::new()) }; }

pub struct Host {
    hwnd: HWND,
    menu: Menu<u32>,
    caller: Option<(isize, usize)>,
}
impl Host {
    pub fn new() -> Result<Self> {
        let menu = Menu::new("mypaper commands")?;
        // SAFETY: a static callback; this thread owns and destroys the hidden inbox.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None)?.into();
            RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(receive),
                hInstance: instance,
                lpszClassName: wire::HOST,
                ..Default::default()
            });
            CreateWindowExW(
                Default::default(),
                wire::HOST,
                PCWSTR::null(),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            menu,
            caller: None,
        })
    }
    pub fn process(&mut self, ui: &mut Ui) {
        for (caller, request) in REQUESTS.with_borrow_mut(std::mem::take) {
            self.finish(0);
            self.menu.close();
            self.caller = Some((caller, request.nonce));
            let tokens = mypaper_config::Config::load()
                .map(|c| crate::look::tokens(&c.theme, &ui.text))
                .unwrap_or_default();
            if let Err(e) = self.menu.open(
                ui,
                entries(request.items),
                request.point,
                mypaper_ui::screen_at(request.point.0, request.point.1),
                &tokens,
            ) {
                log::warn!("tray menu: {e:#}");
                self.finish(0);
            }
        }
    }
    pub fn event(&mut self, ui: &mut Ui, id: SurfaceId, event: &Event) -> bool {
        if !self.menu.owns(id) {
            return false;
        }
        if let Some(outcome) = self.menu.handle(ui, id, event) {
            self.finish(match outcome {
                Outcome::Chosen(id) => id,
                Outcome::Closed => 0,
            });
        }
        true
    }
    fn finish(&mut self, id: u32) {
        if let Some((caller, nonce)) = self.caller.take() {
            // SAFETY: posting never waits on core, even if it has already exited.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(caller as _)),
                    wire::CHOSEN,
                    WPARAM(id as usize),
                    LPARAM(nonce as isize),
                )
            };
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.finish(0);
        // SAFETY: the inbox is owned by this thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

fn entries(items: Vec<Item>) -> Vec<Entry<u32>> {
    items
        .into_iter()
        .map(|item| match item {
            Item::Separator => Entry::Separator,
            Item::Action {
                label,
                id,
                checked,
                enabled,
            } => Entry::Action {
                label,
                command: id,
                checked: checked.then_some(true),
                enabled,
            },
            Item::Submenu { label, items } => Entry::Submenu {
                label,
                items: entries(items),
            },
        })
        .collect()
}

unsafe extern "system" fn receive(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA && lp.0 != 0 {
        // SAFETY: WM_COPYDATA keeps this packet and its bytes alive until we return; copy now.
        let packet = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
        if packet.dwData == wire::MAGIC && packet.cbData <= 256 * 1024 && !packet.lpData.is_null() {
            // SAFETY: the validated WM_COPYDATA buffer remains readable for this callback.
            let bytes = unsafe {
                std::slice::from_raw_parts(packet.lpData as *const u8, packet.cbData as usize)
            };
            if let Ok(request) = serde_json::from_slice::<Request>(bytes) {
                REQUESTS.with_borrow_mut(|q| q.push((wp.0 as isize, request)));
                return LRESULT(1);
            }
        }
        return LRESULT(0);
    }
    // SAFETY: messages the inbox doesn't handle retain their original parameters.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}
