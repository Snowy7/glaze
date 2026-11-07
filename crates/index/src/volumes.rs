//! Drive-letter arrival/removal broadcasts. A hidden top-level window receives volume
//! WM_DEVICECHANGE messages (message-only windows do not), with no polling timer.
use anyhow::Result;
use std::sync::mpsc::SyncSender;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use crate::watch::Change;

unsafe extern "system" fn window(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DEVICECHANGE && matches!(wp.0, 0x0007 | 0x0018 | 0x8000 | 0x8004) {
        // SAFETY: the window's thread owns this sender until after its message loop ends.
        let sender = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const SyncSender<Change>;
        if !sender.is_null() {
            // SAFETY: installed from the live stack immediately after window creation.
            let _ = unsafe { &*sender }.send(Change::VolumesChanged);
        }
    }
    // SAFETY: forward unchanged message arguments to Windows.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub(crate) fn start(tx: SyncSender<Change>) -> Result<usize> {
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("index-volumes".into())
        .spawn(move || {
            let create = || -> Result<HWND> {
                // SAFETY: owns the class and invisible window on this message-pumping thread.
                unsafe {
                    let instance = GetModuleHandleW(None)?;
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(window),
                        hInstance: instance.into(),
                        lpszClassName: w!("mypaper.index.volumes"),
                        ..Default::default()
                    };
                    RegisterClassW(&class);
                    let hwnd = CreateWindowExW(
                        Default::default(),
                        class.lpszClassName,
                        w!(""),
                        WS_OVERLAPPED,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(instance.into()),
                        None,
                    )?;
                    SetWindowLongPtrW(
                        hwnd,
                        GWLP_USERDATA,
                        (&tx as *const SyncSender<Change>) as isize,
                    );
                    Ok(hwnd)
                }
            };
            match create() {
                Ok(hwnd) => {
                    let _ = ready.send(Ok(hwnd.0 as usize));
                    // SAFETY: only dispatches this thread's messages; the sender stays live.
                    unsafe {
                        let mut msg = MSG::default();
                        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })?;
    started
        .recv()
        .map_err(|_| anyhow::anyhow!("volume notifier stopped before startup"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_arrival_and_removal_broadcasts_wake_the_service() {
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let raw = start(tx).unwrap();
        let volume: [u32; 5] = [20, 2, 0, 1 << 25, 0];
        for event in [0x0007, 0x8000, 0x8004] {
            // SAFETY: addresses only the hidden window created for this test.
            unsafe {
                SendMessageW(
                    HWND(raw as *mut _),
                    WM_DEVICECHANGE,
                    Some(WPARAM(event)),
                    Some(LPARAM(volume.as_ptr() as isize)),
                );
            }
            assert!(matches!(
                rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap(),
                Change::VolumesChanged
            ));
        }
        // SAFETY: closes only this test's hidden window on its owning thread.
        unsafe {
            let _ = PostMessageW(Some(HWND(raw as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}
