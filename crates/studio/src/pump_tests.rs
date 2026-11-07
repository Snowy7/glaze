//! Replay the live fast-chord failure without focusing a window or injecting desktop input.
use super::*;
use windows::Win32::Foundation::{HWND, LRESULT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, PostMessageW, RegisterClassExW,
    WM_APP, WM_CLOSE, WNDCLASSEXW,
};

const RELEASE_MODIFIERS: u32 = WM_APP + 91;

/// Only the test thread's queue state changes; the desktop keyboard is never touched.
struct KeyboardState([u8; 256]);

impl Drop for KeyboardState {
    fn drop(&mut self) {
        // SAFETY: restores the calling test thread's saved keyboard state.
        let _ = unsafe { SetKeyboardState(&self.0) };
    }
}

struct ResetWindow(HWND);

impl Drop for ResetWindow {
    fn drop(&mut self) {
        // SAFETY: our message-only window, on the thread that created it.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn release_modifiers(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == RELEASE_MODIFIERS {
        // SAFETY: emulate the queued key-up's effect on this thread, then close only our
        // hidden Studio. Both messages are consumed in the same native message batch.
        unsafe {
            SetKeyboardState(&[0; 256]).unwrap();
            PostMessageW(
                Some(HWND(lparam.0 as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
            .unwrap();
        }
        return LRESULT(0);
    }
    // SAFETY: unhandled messages retain their ordinary message-only-window behavior.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[test]
fn queued_shortcut_keeps_modifiers_until_studio_handles_the_key() {
    let ui = Ui::new().unwrap();
    let mut app = goldens::window_app(true, &ui, (1280, 800));
    let before = app.docs.doc(docs::DocId::Bars).raw.clone();
    app.run(actions::Action::Preset("islands"));
    assert_ne!(app.docs.doc(docs::DocId::Bars).raw, before);
    let surface = Surface::new_deferred_accessibility(
        Kind::App { min: (960, 640) },
        "Studio queued-shortcut test (hidden)",
        Role::Window,
    )
    .unwrap();
    app.window = Some(surface.hwnd());
    let hwnd = surface.hwnd();
    let studio = Rc::new(RefCell::new(Studio {
        app,
        ui,
        surface,
        menu: None,
        picking: None,
        frame: None,
    }));
    // SAFETY: no ShowWindow, focus, SendInput, or desktop keyboard-state changes. The reset
    // endpoint belongs to this test thread and closes only the hidden surface above.
    let reset = unsafe {
        let name = w!("mypaper.studio.queued-shortcut-test");
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(release_modifiers),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&class);
        ResetWindow(
            CreateWindowExW(
                Default::default(),
                name,
                w!(""),
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
            .unwrap(),
        )
    };
    let mut saved = KeyboardState([0; 256]);
    // SAFETY: valid arrays; these APIs change only this thread's message-queue key state.
    unsafe {
        GetKeyboardState(&mut saved.0).unwrap();
        let mut pressed = [0; 256];
        pressed[0x11] = 0x80;
        SetKeyboardState(&pressed).unwrap();
        PostMessageW(Some(hwnd), WM_KEYDOWN, WPARAM(0x5A), LPARAM(0)).unwrap();
        PostMessageW(
            Some(reset.0),
            RELEASE_MODIFIERS,
            WPARAM(0),
            LPARAM(hwnd.0 as isize),
        )
        .unwrap();
    }
    let (_tx, rx) = channel();
    pump(&studio, &rx).unwrap();
    let after = studio.borrow().app.docs.doc(docs::DocId::Bars).raw.clone();
    let studio = match Rc::try_unwrap(studio) {
        Ok(studio) => studio.into_inner(),
        Err(_) => unreachable!(),
    };
    let Studio {
        app,
        ui,
        surface,
        menu,
        ..
    } = studio;
    // Release the surface and fixture caches before Ui tears down this thread's COM apartment.
    drop(menu);
    drop(surface);
    drop(app);
    drop(ui);
    assert_eq!(
        after, before,
        "Ctrl+Z must execute before the next queued message releases Ctrl"
    );
}
