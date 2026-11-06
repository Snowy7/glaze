//! Small HWND adapter for text selection, extra mouse buttons and shell-owned context menus.
use anyhow::{Result, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};
use windows::{
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, Interface, PCSTR},
};

thread_local! {
    static INPUT: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
    static FIELD: Cell<bool> = const { Cell::new(false) };
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
    static GEOMETRY_HOOK: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

struct Context {
    three: Option<IContextMenu3>,
    two: Option<IContextMenu2>,
}

#[derive(Clone, Copy, Debug)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}
impl Modifiers {
    pub fn read() -> Self {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        Self {
            // SAFETY: this reads the current UI thread key state without retaining pointers.
            control: unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0,
            // SAFETY: this reads the current UI thread key state without retaining pointers.
            shift: unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0,
            // SAFETY: this reads the current UI thread key state without retaining pointers.
            alt: unsafe { GetKeyState(VK_MENU.0 as i32) } < 0,
        }
    }
}
pub enum Input {
    Character(char),
    Back(bool),
    Key(u16, Modifiers),
}
pub fn next() -> Option<Input> {
    INPUT.with_borrow_mut(VecDeque::pop_front)
}
pub fn field(active: bool) {
    FIELD.set(active);
}
pub fn set_geometry_hook(hook: Option<Box<dyn FnMut()>>) {
    GEOMETRY_HOOK.with_borrow_mut(|slot| *slot = hook);
}

pub fn origin(hwnd: HWND) -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: the live HWND is queried into a writable POINT local.
    let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut point) };
    (point.x, point.y)
}

pub fn install(hwnd: HWND) -> Result<()> {
    ensure!(
        // SAFETY: this is our HWND and a static subclass procedure with no borrowed callback state.
        unsafe { SetWindowSubclass(hwnd, Some(subclass), 1, 0) }.as_bool(),
        "Installing Files input adapter"
    );
    Ok(())
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if matches!(msg, WM_WINDOWPOSCHANGED | WM_SHOWWINDOW | WM_SIZE) {
        // Windows' modal move/resize loop does not return to main. A guarded render hook keeps
        // this app's alpha canvas and engine glass in the same place during that loop.
        let hook = GEOMETRY_HOOK.with_borrow_mut(Option::take);
        if let Some(mut hook) = hook {
            hook();
            GEOMETRY_HOOK.with_borrow_mut(|slot| *slot = Some(hook));
        }
    }
    if matches!(
        msg,
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
    ) {
        let result = CONTEXT.with_borrow(|menu| {
            menu.as_ref().map(|menu| {
                let mut result = LRESULT(0);
                // SAFETY: the original menu message is forwarded to its live COM handler with writable result storage.
                let handled = if let Some(menu) = &menu.three {
                    // SAFETY: forwards this menu's original message with valid result storage.
                    unsafe { menu.HandleMenuMsg2(msg, w, l, Some(&mut result)) }.is_ok()
                } else if let Some(menu) = &menu.two {
                    // SAFETY: forwards the original owner-draw/submenu message to its handler.
                    unsafe { menu.HandleMenuMsg(msg, w, l) }.is_ok()
                } else {
                    false
                };
                handled.then_some(result)
            })
        });
        if let Some(Some(result)) = result {
            return result;
        }
    }
    if msg == WM_CHAR && !FIELD.get() {
        if let Some(character) = char::from_u32(w.0 as u32).filter(|c| !c.is_control()) {
            INPUT.with_borrow_mut(|input| input.push_back(Input::Character(character)));
        }
        return LRESULT(0);
    }
    // Snapshot modifiers while dispatching the key, before the queue drains its key-up. The
    // controller runs afterwards; GetKeyState there can already report a released Ctrl/Shift.
    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        let modifiers = Modifiers::read();
        let system = msg == WM_SYSKEYDOWN && matches!(w.0, 0x73 | 0x20);
        let app_shortcut = modifiers.control
            && matches!(w.0, 0x4c | 0x45 | 0x46 | 0x54 | 0x57 | 0x09)
            || w.0 == 0x74
            || w.0 == 0x1b
            || matches!(w.0, 0x09 | 0x21 | 0x22 | 0x26 | 0x28);
        if !system && (!FIELD.get() || app_shortcut || modifiers.alt) {
            INPUT.with_borrow_mut(|input| input.push_back(Input::Key(w.0 as u16, modifiers)));
            return LRESULT(0);
        }
    }
    if msg == WM_XBUTTONUP {
        INPUT.with_borrow_mut(|input| input.push_back(Input::Back((w.0 >> 16) == 2)));
        return LRESULT(1);
    }
    // SAFETY: the original HWND message and parameters are forwarded unchanged to the next procedure.
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

/// CF_UNICODETEXT, with ownership transferred only after SetClipboardData succeeds.
pub fn copy_text(owner: HWND, value: &str) -> Result<()> {
    use windows::Win32::System::{DataExchange::*, Memory::*};
    let text: Vec<u16> = value.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is closed on every path; the movable allocation is unlocked before transfer.
    unsafe {
        OpenClipboard(Some(owner))?;
        let result = (|| -> Result<()> {
            let global = GlobalAlloc(GMEM_MOVEABLE, text.len() * 2)?;
            let pointer = GlobalLock(global);
            if pointer.is_null() {
                let _ = GlobalFree(Some(global));
                anyhow::bail!("Allocating path clipboard");
            }
            std::ptr::copy_nonoverlapping(text.as_ptr(), pointer.cast(), text.len());
            let _ = GlobalUnlock(global);
            if let Err(error) = EmptyClipboard()
                .and_then(|()| SetClipboardData(13, Some(HANDLE(global.0))).map(|_| ()))
            {
                let _ = GlobalFree(Some(global));
                return Err(error.into());
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

struct Ids(Vec<*mut Common::ITEMIDLIST>);
impl Drop for Ids {
    fn drop(&mut self) {
        for id in &self.0 {
            // SAFETY: each PIDL was allocated by SHParseDisplayName and is freed once after shell binding.
            unsafe { CoTaskMemFree(Some((*id).cast())) };
        }
    }
}
struct Popup(HMENU);
impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: this popup menu is owned by this guard and destroyed once.
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

/// The actual shell IContextMenu, including owner-drawn extensions and dynamic submenus.
/// Cross-folder search selections have no shared parent; show the first item's menu there.
pub fn context_menu(
    owner: HWND,
    paths: &[String],
    background: Option<&str>,
    at: (i32, i32),
) -> Result<()> {
    // SAFETY: this UI thread owns an initialized STA; PIDLs, menus and invoke strings outlive each shell call.
    unsafe {
        let menu: IContextMenu = if paths.is_empty() {
            let folder = crate::shell::item(background.unwrap_or("shell:Desktop"))?;
            let folder: IShellFolder = folder.BindToHandler(None, &BHID_SFObject)?;
            folder.CreateViewObject(owner)?
        } else {
            let mut ids = Ids(Vec::new());
            let same_parent = paths.iter().all(|path| {
                std::path::Path::new(path).parent() == std::path::Path::new(&paths[0]).parent()
            });
            for path in paths.iter().take(if same_parent { paths.len() } else { 1 }) {
                let mut id = std::ptr::null_mut();
                SHParseDisplayName(&HSTRING::from(path), None, &mut id, 0, None)?;
                ids.0.push(id);
            }
            let parent: IShellFolder = SHBindToParent(ids.0[0], None)?;
            let children = ids
                .0
                .iter()
                .map(|id| ILFindLastID(*id) as *const _)
                .collect::<Vec<_>>();
            parent.GetUIObjectOf(owner, &children, None)?
        };
        let popup = Popup(CreatePopupMenu()?);
        menu.QueryContextMenu(popup.0, 0, 1, 0x7fff, CMF_NORMAL)
            .ok()?;
        CONTEXT.with_borrow_mut(|slot| {
            *slot = Some(Context {
                three: menu.cast().ok(),
                two: menu.cast().ok(),
            })
        });
        let command = TrackPopupMenuEx(
            popup.0,
            TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0,
            at.0,
            at.1,
            owner,
            None,
        )
        .0;
        CONTEXT.with_borrow_mut(|slot| *slot = None);
        if command > 0 {
            let info = CMINVOKECOMMANDINFO {
                cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd: owner,
                lpVerb: PCSTR((command - 1) as usize as _),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            menu.InvokeCommand(&info)?;
        }
    }
    Ok(())
}
