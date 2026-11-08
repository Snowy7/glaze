//! File drops are accepted by Studio's own window only while a composition is open.
use glaze_ui::Point;
use std::{cell::RefCell, path::PathBuf};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{
        Shell::{
            DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, DragQueryPoint, HDROP,
            RemoveWindowSubclass, SetWindowSubclass,
        },
        WindowsAndMessaging::WM_DROPFILES,
    },
};

const ID: usize = 0x4d50_4352;
thread_local! { static DROPS:RefCell<Vec<(PathBuf,Point)>>=const{RefCell::new(Vec::new())}; }

#[derive(Debug)]
pub struct Target(HWND);
impl Target {
    pub fn new(window: HWND, thread: u32) -> Option<Self> {
        // SAFETY: the UI thread installs a subclass only on Studio's own HWND; Target removes it.
        unsafe {
            if !SetWindowSubclass(window, Some(callback), ID, thread as usize).as_bool() {
                return None;
            }
            DragAcceptFiles(window, true);
        }
        Some(Self(window))
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        // SAFETY: this target and its window live on the Studio UI thread.
        unsafe {
            DragAcceptFiles(self.0, false);
            let _ = RemoveWindowSubclass(self.0, Some(callback), ID);
        }
        DROPS.with(|drops| drops.borrow_mut().clear());
    }
}
pub fn take() -> Vec<(PathBuf, Point)> {
    DROPS.with(|drops| std::mem::take(&mut *drops.borrow_mut()))
}

unsafe extern "system" fn callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    thread: usize,
) -> LRESULT {
    if message == WM_DROPFILES {
        let drop = HDROP(wparam.0 as *mut _);
        let mut point = POINT::default();
        // SAFETY: WM_DROPFILES gives us ownership of this HDROP until DragFinish below.
        unsafe {
            let _ = DragQueryPoint(drop, &mut point);
            let count = DragQueryFileW(drop, u32::MAX, None);
            for i in 0..count {
                let length = DragQueryFileW(drop, i, None);
                let mut text = vec![0u16; length as usize + 1];
                DragQueryFileW(drop, i, Some(&mut text));
                let path = PathBuf::from(String::from_utf16_lossy(&text[..length as usize]));
                DROPS.with(|drops| {
                    drops
                        .borrow_mut()
                        .push((path, Point::new(point.x as f32, point.y as f32)))
                });
            }
            DragFinish(drop);
        }
        crate::wake(thread as u32);
        return LRESULT(0);
    }
    // SAFETY: unhandled messages retain the existing Studio window procedure.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}
