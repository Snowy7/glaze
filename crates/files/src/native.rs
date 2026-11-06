//! Small HWND adapter for text selection, extra mouse buttons and shell-owned context menus.
use std::{cell::{Cell, RefCell}, collections::VecDeque};
use anyhow::{Result, ensure};
use windows::{core::{HSTRING, Interface, PCSTR}, Win32::{Foundation::*, System::Com::*, UI::{Shell::*, WindowsAndMessaging::*}}};

thread_local! {
    static INPUT: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
    static FIELD: Cell<bool> = const { Cell::new(false) };
    static CONTEXT: RefCell<Option<IContextMenu3>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug)]
pub struct Modifiers { pub control: bool, pub shift: bool, pub alt: bool }
impl Modifiers {
    pub fn read() -> Self {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        Self { control: unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0,
            shift: unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0,
            alt: unsafe { GetKeyState(VK_MENU.0 as i32) } < 0 }
    }
}
pub enum Input { Character(char), Back(bool), Key(u16, Modifiers) }
pub fn next() -> Option<Input> { INPUT.with_borrow_mut(VecDeque::pop_front) }
pub fn field(active: bool) { FIELD.set(active); }

pub fn origin(hwnd: HWND) -> (i32, i32) {
    let mut point = POINT::default();
    let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut point) };
    (point.x, point.y)
}

pub fn install(hwnd: HWND) -> Result<()> {
    ensure!(unsafe { SetWindowSubclass(hwnd, Some(subclass), 1, 0) }.as_bool(), "Installing Files input adapter");
    Ok(())
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM, _: usize, _: usize) -> LRESULT {
    if matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        let result = CONTEXT.with_borrow(|menu| menu.as_ref().map(|menu| {
            let mut result = LRESULT(0);
            let _ = unsafe { menu.HandleMenuMsg2(msg, w, l, Some(&mut result)) };
            result
        }));
        if let Some(result) = result { return result; }
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
        let app_shortcut = modifiers.control && matches!(w.0, 0x4c | 0x45 | 0x46)
            || w.0 == 0x74 || w.0 == 0x1b || matches!(w.0, 0x09 | 0x21 | 0x22 | 0x26 | 0x28);
        if !system && (!FIELD.get() || app_shortcut || modifiers.alt) {
            INPUT.with_borrow_mut(|input| input.push_back(Input::Key(w.0 as u16, modifiers)));
            return LRESULT(0);
        }
    }
    if msg == WM_XBUTTONUP {
        INPUT.with_borrow_mut(|input| input.push_back(Input::Back((w.0 >> 16) == 2)));
        return LRESULT(1);
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

struct Ids(Vec<*mut Common::ITEMIDLIST>);
impl Drop for Ids { fn drop(&mut self) { for id in &self.0 { unsafe { CoTaskMemFree(Some((*id).cast())) }; } } }
struct Popup(HMENU);
impl Drop for Popup { fn drop(&mut self) { let _ = unsafe { DestroyMenu(self.0) }; } }

/// The actual shell IContextMenu, including owner-drawn extensions and dynamic submenus.
/// Cross-folder search selections have no shared parent; show the first item's menu there.
pub fn context_menu(owner: HWND, paths: &[String], background: Option<&str>, at: (i32, i32)) -> Result<()> {
    unsafe {
        let menu: IContextMenu = if paths.is_empty() {
            let folder = crate::shell::item(background.unwrap_or("shell:Desktop"))?;
            let folder: IShellFolder = folder.BindToHandler(None, &BHID_SFObject)?;
            folder.CreateViewObject(owner)?
        } else {
            let mut ids = Ids(Vec::new());
            let same_parent = paths.iter().all(|path| std::path::Path::new(path).parent() == std::path::Path::new(&paths[0]).parent());
            for path in paths.iter().take(if same_parent { paths.len() } else { 1 }) {
                let mut id = std::ptr::null_mut();
                SHParseDisplayName(&HSTRING::from(path), None, &mut id, 0, None)?;
                ids.0.push(id);
            }
            let parent: IShellFolder = SHBindToParent(ids.0[0], None)?;
            let children = ids.0.iter().map(|id| ILFindLastID(*id) as *const _).collect::<Vec<_>>();
            parent.GetUIObjectOf(owner, &children, None)?
        };
        let popup = Popup(CreatePopupMenu()?);
        menu.QueryContextMenu(popup.0, 0, 1, 0x7fff, CMF_NORMAL).ok()?;
        CONTEXT.with_borrow_mut(|slot| *slot = menu.cast().ok());
        let command = TrackPopupMenuEx(popup.0, TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0, at.0, at.1, owner, None).0;
        CONTEXT.with_borrow_mut(|slot| *slot = None);
        if command > 0 {
            let info = CMINVOKECOMMANDINFO { cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32, hwnd: owner,
                lpVerb: PCSTR((command - 1) as usize as _), nShow: SW_SHOWNORMAL.0, ..Default::default() };
            menu.InvokeCommand(&info)?;
        }
    }
    Ok(())
}
