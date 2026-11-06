//! Explorer-compatible OLE clipboard and drag/drop. The shell supplies IDataObject, including
//! CF_HDROP and shell namespace formats; file mutations still go through IFileOperation.
use std::{cell::Cell, sync::{Arc, Mutex, mpsc::{self, Receiver, Sender}}};
use anyhow::{Result, ensure};
use windows::{core::{implement, Ref, HRESULT, BOOL, HSTRING}, Win32::{
    Foundation::*, System::{Com::*, DataExchange::RegisterClipboardFormatW, Memory::*, Ole::*, SystemServices::*},
    UI::{Shell::*, WindowsAndMessaging::PostMessageW},
}};
use crate::{backend::WAKE, shell::Operation};

fn format(id: u16) -> FORMATETC {
    FORMATETC { cfFormat: id, dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32, ..Default::default() }
}

struct Medium(STGMEDIUM);
impl Drop for Medium { fn drop(&mut self) { unsafe { ReleaseStgMedium(&mut self.0) }; } }

pub fn data_object(paths: &[String]) -> Result<IDataObject> {
    ensure!(!paths.is_empty(), "No items selected");
    let mut ids = Vec::new();
    let result = (|| {
        for path in paths {
            let mut id = std::ptr::null_mut();
            unsafe { SHParseDisplayName(&HSTRING::from(path), None, &mut id, 0, None)?; }
            ids.push(id);
        }
        let pointers: Vec<_> = ids.iter().map(|id| *id as *const _).collect();
        let items = unsafe { SHCreateShellItemArrayFromIDLists(&pointers) }?;
        Ok(unsafe { items.BindToHandler(None, &BHID_DataObject) }?)
    })();
    for id in ids { unsafe { CoTaskMemFree(Some(id.cast())) }; }
    result
}

fn effect_format() -> u16 { unsafe { RegisterClipboardFormatW(windows::core::w!("Preferred DropEffect")) as u16 } }

pub fn copy(paths: &[String], cutting: bool) -> Result<()> {
    let data = data_object(paths)?;
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 4)?;
        let pointer = GlobalLock(global);
        if pointer.is_null() { let _ = GlobalFree(Some(global)); anyhow::bail!("Allocating clipboard effect"); }
        pointer.cast::<u32>().write(if cutting { DROPEFFECT_MOVE.0 } else { DROPEFFECT_COPY.0 });
        let _ = GlobalUnlock(global);
        let medium = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: global }, ..Default::default() };
        if let Err(error) = data.SetData(&format(effect_format()), &medium, true) {
            let _ = GlobalFree(Some(global)); return Err(error.into());
        }
        OleSetClipboard(&data)?;
    }
    Ok(())
}

pub fn files(data: &IDataObject) -> Result<Vec<String>> {
    let medium = Medium(unsafe { data.GetData(&format(15)) }?);
    ensure!(medium.0.tymed == TYMED_HGLOBAL.0 as u32, "Clipboard does not contain files");
    let drop = HDROP(unsafe { medium.0.u.hGlobal }.0);
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    ensure!(count <= 100_000, "Too many clipboard files");
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        let length = unsafe { DragQueryFileW(drop, i, None) } as usize;
        let mut path = vec![0; length + 1];
        unsafe { DragQueryFileW(drop, i, Some(&mut path)); }
        paths.push(String::from_utf16_lossy(&path[..length]));
    }
    Ok(paths)
}

pub fn paste(destination: String) -> Result<Operation> {
    let data = unsafe { OleGetClipboard() }?;
    let paths = files(&data)?;
    let moving = unsafe { data.GetData(&format(effect_format())) }.ok().is_some_and(|medium| {
        let medium = Medium(medium);
        if medium.0.tymed != TYMED_HGLOBAL.0 as u32 { return false; }
        let global = unsafe { medium.0.u.hGlobal };
        let pointer = unsafe { GlobalLock(global) };
        if pointer.is_null() { return false; }
        let moving = unsafe { pointer.cast::<u32>().read() } & DROPEFFECT_MOVE.0 != 0;
        unsafe { let _ = GlobalUnlock(global); }
        moving
    });
    Ok(Operation::Copy { paths, destination, moving })
}

pub fn flush() { let _ = unsafe { OleFlushClipboard() }; }

#[implement(IDropSource)]
struct Source;
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() { DRAGDROP_S_CANCEL }
        else if !keys.contains(MK_LBUTTON) { DRAGDROP_S_DROP }
        else { HRESULT(0) }
    }
    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT { DRAGDROP_S_USEDEFAULTCURSORS }
}

pub fn drag(paths: &[String]) -> Result<()> {
    let data = data_object(paths)?;
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT_NONE;
    unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect).ok()?; }
    Ok(())
}

#[implement(IDropTarget)]
struct Target {
    destination: Arc<Mutex<Option<String>>>,
    sender: Sender<Operation>,
    hwnd: usize,
    valid: Cell<bool>,
    allowed: Cell<DROPEFFECT>,
}
impl Target_Impl {
    fn effect(&self, keys: MODIFIERKEYS_FLAGS) -> DROPEFFECT {
        if !self.valid.get() || self.destination.lock().ok().and_then(|p| p.clone()).is_none() { return DROPEFFECT_NONE; }
        let wanted = if keys.contains(MK_SHIFT) { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
        if self.allowed.get().contains(wanted) { wanted } else { DROPEFFECT_NONE }
    }
}
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        self.valid.set(data.as_ref().is_some_and(|data| unsafe { data.QueryGetData(&format(15)) }.is_ok()));
        self.allowed.set(unsafe { *effect });
        unsafe { *effect = self.effect(keys); }
        Ok(())
    }
    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = self.effect(keys); }
        Ok(())
    }
    fn DragLeave(&self) -> windows::core::Result<()> { self.valid.set(false); Ok(()) }
    fn Drop(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let chosen = self.effect(keys);
        unsafe { *effect = DROPEFFECT_NONE; }
        if chosen == DROPEFFECT_NONE { return Ok(()); }
        if let Some(destination) = self.destination.lock().ok().and_then(|d| d.clone())
            && let Some(data) = data.as_ref()
            && let Ok(paths) = files(data)
            && self.sender.send(Operation::Copy { paths, destination, moving: chosen == DROPEFFECT_MOVE }).is_ok()
        {
            unsafe { *effect = chosen; }
            let _ = unsafe { PostMessageW(Some(HWND(self.hwnd as _)), WAKE, WPARAM(0), LPARAM(0)) };
        }
        Ok(())
    }
}

pub struct DropTarget {
    hwnd: HWND,
    _target: IDropTarget,
    pub destination: Arc<Mutex<Option<String>>>,
    pub requests: Receiver<Operation>,
}
impl DropTarget {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let destination = Arc::new(Mutex::new(None));
        let (sender, requests) = mpsc::channel();
        let target: IDropTarget = Target { destination: destination.clone(), sender, hwnd: hwnd.0 as usize, valid: Cell::new(false), allowed: Cell::new(DROPEFFECT_NONE) }.into();
        unsafe { RegisterDragDrop(hwnd, &target)?; }
        Ok(Self { hwnd, _target: target, destination, requests })
    }
}
impl Drop for DropTarget { fn drop(&mut self) { let _ = unsafe { RevokeDragDrop(self.hwnd) }; } }
