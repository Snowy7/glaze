//! Retains the tray/Studio edit messages while the widget executable goes away. Everything
//! becomes an inbox notification; a COM wait loop cannot discard a thread-only hotkey.
use anyhow::{Context, Result};
use std::sync::OnceLock;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

static MESSAGES: OnceLock<(u32, u32)> = OnceLock::new();
pub struct Control {
    hwnd: HWND,
    _power: glaze_desktop::ControlWindow,
}
impl Control {
    pub fn new() -> Result<Self> {
        // SAFETY: static class/procedure and registered protocol names. Own windows are
        // destroyed on their creating thread, and hotkeys are unregistered before destruction.
        unsafe {
            MESSAGES.get_or_init(|| {
                (
                    RegisterWindowMessageW(w!("glaze.widgets.edit")),
                    RegisterWindowMessageW(w!("glaze.widgets.overlay")),
                )
            });
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: w!("glaze.widgets.host"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("glaze.widgets.host"),
                w!("Native widgets"),
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
            .context("widget control")?;
            for (id, key) in [(1, b'W'), (2, b'O')] {
                if let Err(e) = RegisterHotKey(
                    Some(hwnd),
                    id,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    key.into(),
                ) {
                    log::warn!("widget hotkey: {e}");
                }
            }
            Ok(Self {
                hwnd,
                _power: glaze_desktop::ControlWindow::new(w!("glaze.widgets.power"))?,
            })
        }
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        // SAFETY: own hotkeys/window, on the creating UI thread.
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), 1);
            let _ = UnregisterHotKey(Some(self.hwnd), 2);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
unsafe extern "system" fn proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_COPYDATA && lparam.0 != 0 {
        // SAFETY: WM_COPYDATA's buffer is valid only during this call; receive copies it.
        let data = unsafe { &*(lparam.0 as *const windows::Win32::System::DataExchange::COPYDATASTRUCT) };
        if data.dwData == glaze_widgets::plugin_manager::COPY_TAG && data.cbData <= 64 << 10 && !data.lpData.is_null() {
            // SAFETY: bounded synchronous WM_COPYDATA payload, copied before returning.
            let bytes = unsafe { std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize) };
            return LRESULT(isize::from(glaze_widgets::plugin_manager::receive(bytes)));
        }
    }
    let (edit, overlay) = *MESSAGES.get().expect("registered before window");
    if message == edit || (message == WM_HOTKEY && wparam.0 == 1) {
        crate::inbox::post(
            super::WM_EDIT,
            if message == WM_HOTKEY {
                2
            } else {
                wparam.0 as isize
            },
        );
        return LRESULT(0);
    }
    if message == overlay || (message == WM_HOTKEY && wparam.0 == 2) {
        crate::inbox::post(
            super::WM_OVERLAY,
            if message == WM_HOTKEY {
                2
            } else {
                wparam.0 as isize
            },
        );
        return LRESULT(0);
    }
    // SAFETY: default handling of the original message.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
