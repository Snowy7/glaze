//! Keep a popup and its engine glass together when Win32 raises a layered window. Older
//! overlapping popups must not jump over a menu opened inside them during a repaint.
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use windows::core::w;

thread_local! { static OPEN: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; }
const SUBCLASS: usize = 0x4d50_474c;

pub(super) fn open(client: HWND) {
    OPEN.with_borrow_mut(|open| {
        open.retain(|&h| h != client);
        open.push(client);
    });
    // SAFETY: the Backdrop is installed on the surface's own UI thread.
    let _ = unsafe { SetWindowSubclass(client, Some(follow), SUBCLASS, 0) };
}

pub(super) fn close(client: HWND) {
    OPEN.with_borrow_mut(|open| open.retain(|&h| h != client));
    // SAFETY: removing only our own subclass on this UI thread, including pooled menus.
    let _ = unsafe { RemoveWindowSubclass(client, Some(follow), SUBCLASS) };
}

fn newer_overlaps(client: HWND) -> bool {
    OPEN.with_borrow(|open| {
        let Some(index) = open.iter().position(|&h| h == client) else {
            return false;
        };
        let mut a = RECT::default();
        // SAFETY: read-only queries; vanished windows fail the query.
        unsafe {
            if GetWindowRect(client, &mut a).is_err() {
                return false;
            }
            open[index + 1..].iter().any(|&newer| {
                let mut b = RECT::default();
                IsWindowVisible(newer).as_bool()
                    && GetWindowRect(newer, &mut b).is_ok()
                    && a.left < b.right
                    && b.left < a.right
                    && a.top < b.bottom
                    && b.top < a.bottom
            })
        }
    })
}

unsafe extern "system" fn follow(
    client: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING && lp.0 != 0
        // SAFETY: Win32 supplies a live WINDOWPOS for this message.
        && !unsafe { (*(lp.0 as *const WINDOWPOS)).flags }.contains(SWP_NOZORDER)
        && newer_overlaps(client)
    {
        // SAFETY: Win32 supplies a live WINDOWPOS for this message. Position/size still apply.
        unsafe {
            (*(lp.0 as *mut WINDOWPOS)).flags |= SWP_NOZORDER;
        }
    }
    let restack = msg == WM_WINDOWPOSCHANGED && lp.0 != 0
        // SAFETY: the message supplies a live WINDOWPOS.
        && !unsafe { (*(lp.0 as *const WINDOWPOS)).flags }.contains(SWP_NOZORDER);
    // SAFETY: preserve the surface's original handling before following its final z-order.
    let result = unsafe { DefSubclassProc(client, msg, wp, lp) };
    if restack {
        glaze_ui::platform::windows::glass::system().follow(client);
        move_glass(client);
    }
    if msg == WM_NCDESTROY {
        close(client);
    }
    result
}

fn move_glass(client: HWND) {
    // The engine publishes the client on each of its own glass windows. Lookup also handles
    // device recovery and the multiple glass pieces of a popup crossing monitors.
    let mut pieces = Vec::new();
    let mut previous = None;
    loop {
        // SAFETY: read-only enumeration and a property on the engine's windows.
        let Ok(glass) =
            (unsafe { FindWindowExW(None, previous, w!("glaze.wallpaper.glass"), None) })
        else {
            break;
        };
        previous = Some(glass);
        // SAFETY: the property holds an HWND value, never a pointer we dereference.
        if unsafe { GetPropW(glass, w!("glaze.glass.client")) }.0 != client.0 {
            continue;
        }
        pieces.push(glass);
    }
    // Collect before mutating z-order: enumerating while moving two monitor pieces directly
    // under the client would keep swapping them and could revisit them forever.
    let mut anchor = client;
    for glass in pieces {
        // SAFETY: move only this popup's glass, with no activation, geometry change or redraw.
        unsafe {
            if GetWindow(anchor, GW_HWNDNEXT).ok() != Some(glass) {
                let _ = SetWindowPos(
                    glass,
                    Some(anchor),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
        anchor = glass;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{Foundation::HANDLE, System::LibraryLoader::GetModuleHandleW};

    unsafe extern "system" fn fixture_proc(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        // SAFETY: the fixture forwards the original window message unchanged.
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    #[test]
    fn repaint_cannot_raise_a_parent_over_its_menu_and_glass_follows_a_raise() {
        struct Owned(HWND);
        impl Drop for Owned {
            fn drop(&mut self) {
                close(self.0);
                // SAFETY: only the fixture's own, never activated, off-screen windows.
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        // SAFETY: all windows belong to this thread and stay off screen, with no activation.
        unsafe {
            let class = w!("glaze.wallpaper.glass");
            let instance = GetModuleHandleW(None).unwrap();
            RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(fixture_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            });
            let make = |class| {
                Owned(
                    CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                        class,
                        w!("glass stack fixture"),
                        WS_POPUP,
                        -30000,
                        -30000,
                        100,
                        100,
                        None,
                        None,
                        Some(instance.into()),
                        None,
                    )
                    .unwrap(),
                )
            };
            let lower = make(w!("STATIC"));
            let menu = make(w!("STATIC"));
            let glass = make(class);
            SetPropW(glass.0, w!("glaze.glass.client"), Some(HANDLE(menu.0.0))).unwrap();
            open(lower.0);
            SetWindowPos(
                lower.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .unwrap();
            open(menu.0);
            SetWindowPos(
                menu.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .unwrap();
            assert_eq!(GetWindow(menu.0, GW_HWNDNEXT).unwrap(), glass.0);
            SetWindowPos(
                lower.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .unwrap();
            assert_eq!(
                GetWindow(menu.0, GW_HWNDNEXT).unwrap(),
                glass.0,
                "parent repaint must not split the menu/glass pair"
            );
            assert_eq!(GetWindow(glass.0, GW_HWNDNEXT).unwrap(), lower.0);
            // An explicit later raise repairs the pair even after an intervening window.
            let other = make(w!("STATIC"));
            SetWindowPos(
                other.0,
                Some(menu.0),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .unwrap();
            SetWindowPos(
                menu.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .unwrap();
            assert_eq!(GetWindow(menu.0, GW_HWNDNEXT).unwrap(), glass.0);
            let second = make(class);
            SetPropW(second.0, w!("glaze.glass.client"), Some(HANDLE(menu.0.0))).unwrap();
            SetWindowPos(
                menu.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .unwrap();
            let first = GetWindow(menu.0, GW_HWNDNEXT).unwrap();
            let next = GetWindow(first, GW_HWNDNEXT).unwrap();
            assert!(first == glass.0 || first == second.0);
            assert!(next == glass.0 || next == second.0);
            assert_ne!(
                first, next,
                "both monitor pieces stay directly below the popup"
            );
        }
    }
}
