//! Explorer-compatible OLE clipboard and drag/drop. The shell supplies IDataObject, including
//! CF_HDROP and shell namespace formats; file mutations still go through IFileOperation.
use crate::{backend::WAKE, shell::Operation};
use anyhow::{Result, ensure};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Com::*, DataExchange::RegisterClipboardFormatW, Memory::*, Ole::*, SystemServices::*,
        },
        UI::{Shell::*, WindowsAndMessaging::PostMessageW},
    },
    core::{BOOL, HRESULT, HSTRING, Ref, implement},
};

fn format(id: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: id,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    }
}

struct Medium(STGMEDIUM);
impl Drop for Medium {
    fn drop(&mut self) {
        // SAFETY: this guard owns the STGMEDIUM returned by GetData and releases it once.
        unsafe { ReleaseStgMedium(&mut self.0) };
    }
}

pub fn data_object(paths: &[String]) -> Result<IDataObject> {
    ensure!(!paths.is_empty(), "No items selected");
    let mut ids = Vec::new();
    let result = (|| {
        for path in paths {
            let mut id = std::ptr::null_mut();
            // SAFETY: each terminated parsing name stays live and its allocated PIDL is held in ids until binding ends.
            unsafe {
                SHParseDisplayName(&HSTRING::from(path), None, &mut id, 0, None)?;
            }
            ids.push(id);
        }
        let pointers: Vec<_> = ids.iter().map(|id| *id as *const _).collect();
        // SAFETY: all absolute PIDLs are live through array creation; the shell array retains its own items.
        let items = unsafe { SHCreateShellItemArrayFromIDLists(&pointers) }?;
        // SAFETY: the shell array is live on the initialized OLE UI thread and returns an owned IDataObject.
        Ok(unsafe { items.BindToHandler(None, &BHID_DataObject) }?)
    })();
    for id in ids {
        // SAFETY: each task-allocated absolute PIDL is freed once after shell array creation.
        unsafe { CoTaskMemFree(Some(id.cast())) };
    }
    result
}

fn effect_format() -> u16 {
    // SAFETY: the constant registered-format name is terminated and remains live for the call.
    unsafe { RegisterClipboardFormatW(windows::core::w!("Preferred DropEffect")) as u16 }
}

pub fn copy(paths: &[String], cutting: bool) -> Result<()> {
    let data = data_object(paths)?;
    // SAFETY: the movable HGLOBAL is locked before writing; SetData transfers ownership only on success.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 4)?;
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
            anyhow::bail!("Allocating clipboard effect");
        }
        pointer.cast::<u32>().write(if cutting {
            DROPEFFECT_MOVE.0
        } else {
            DROPEFFECT_COPY.0
        });
        let _ = GlobalUnlock(global);
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: global },
            ..Default::default()
        };
        if let Err(error) = data.SetData(&format(effect_format()), &medium, true) {
            let _ = GlobalFree(Some(global));
            return Err(error.into());
        }
        OleSetClipboard(&data)?;
    }
    Ok(())
}

pub fn files(data: &IDataObject) -> Result<Vec<String>> {
    // SAFETY: IDataObject returns an owned medium, released by the immediately constructed guard.
    let medium = Medium(unsafe { data.GetData(&format(15)) }?);
    ensure!(
        medium.0.tymed == TYMED_HGLOBAL.0 as u32,
        "Clipboard does not contain files"
    );
    // SAFETY: tymed was checked to be TYMED_HGLOBAL before reading this union member.
    let drop = HDROP(unsafe { medium.0.u.hGlobal }.0);
    // SAFETY: the live CF_HDROP medium owns this handle for the query.
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    ensure!(count <= 100_000, "Too many clipboard files");
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: the CF_HDROP handle remains owned by medium while querying each filename length.
        let length = unsafe { DragQueryFileW(drop, i, None) } as usize;
        ensure!(length <= 32_768, "Clipboard filename is too long");
        let mut path = vec![0; length + 1];
        // SAFETY: the filename buffer has the queried length plus its terminator and the medium remains live.
        unsafe {
            DragQueryFileW(drop, i, Some(&mut path));
        }
        paths.push(String::from_utf16_lossy(&path[..length]));
    }
    Ok(paths)
}

pub fn paste(destination: String) -> Result<Operation> {
    // SAFETY: the UI thread has initialized OLE and receives an owned clipboard IDataObject.
    let data = unsafe { OleGetClipboard() }?;
    let paths = files(&data)?;
    // SAFETY: the format descriptor is live and a successful returned medium is immediately guarded.
    let moving = unsafe { data.GetData(&format(effect_format())) }
        .ok()
        .is_some_and(|medium| {
            let medium = Medium(medium);
            if medium.0.tymed != TYMED_HGLOBAL.0 as u32 {
                return false;
            }
            // SAFETY: tymed was checked before accessing its HGLOBAL union member.
            let global = unsafe { medium.0.u.hGlobal };
            // SAFETY: this live medium owns the HGLOBAL; size is checked before reading a DWORD.
            if unsafe { GlobalSize(global) } < size_of::<u32>() {
                return false;
            }
            // SAFETY: the medium owns this HGLOBAL until after the matching unlock.
            let pointer = unsafe { GlobalLock(global) };
            if pointer.is_null() {
                return false;
            }
            // SAFETY: the effect allocation is checked to contain a DWORD and remains locked while reading it.
            let moving = unsafe { pointer.cast::<u32>().read() } & DROPEFFECT_MOVE.0 != 0;
            // SAFETY: this balances the successful GlobalLock above while the medium still owns the handle.
            unsafe {
                let _ = GlobalUnlock(global);
            }
            moving
        });
    Ok(Operation::Copy {
        paths,
        destination,
        moving,
    })
}

pub fn flush() {
    // SAFETY: the OLE UI thread flushes the shell clipboard object before its process exits.
    let _ = unsafe { OleFlushClipboard() };
}

#[implement(IDropSource)]
struct Source;
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if !keys.contains(MK_LBUTTON) {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }
    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

pub fn drag(paths: &[String]) -> Result<()> {
    let data = data_object(paths)?;
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT_NONE;
    // SAFETY: the OLE UI thread retains both COM objects through the synchronous drag loop and effect output.
    unsafe {
        DoDragDrop(
            &data,
            &source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            &mut effect,
        )
        .ok()?;
    }
    Ok(())
}

#[implement(IDropTarget)]
struct Target {
    enabled: Arc<AtomicBool>,
    destination: Arc<Mutex<Option<String>>>,
    regions: Arc<Mutex<Vec<DropRegion>>>,
    sender: Sender<Operation>,
    hwnd: usize,
    valid: Cell<bool>,
    allowed: Cell<DROPEFFECT>,
}
impl Target_Impl {
    fn destination_at(&self, point: &POINTL) -> Option<String> {
        let root = self.destination.lock().ok().and_then(|p| p.clone());
        self.regions
            .lock()
            .ok()
            .and_then(|regions| {
                regions
                    .iter()
                    .find(|region| {
                        point.x >= region.left
                            && point.x < region.right
                            && point.y >= region.top
                            && point.y < region.bottom
                    })
                    .map(|r| r.path.clone())
            })
            .or(root)
    }
    fn effect(&self, keys: MODIFIERKEYS_FLAGS, point: &POINTL) -> DROPEFFECT {
        if !self.enabled.load(Ordering::Relaxed)
            || !self.valid.get()
            || self.destination_at(point).is_none()
        {
            return DROPEFFECT_NONE;
        }
        let wanted = if keys.contains(MK_SHIFT) {
            DROPEFFECT_MOVE
        } else {
            DROPEFFECT_COPY
        };
        if self.allowed.get().contains(wanted) {
            wanted
        } else {
            DROPEFFECT_NONE
        }
    }
}
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.valid.set(
            data.as_ref()
                // SAFETY: QueryGetData only checks a live format descriptor; it transfers no medium or borrowed data.
                .is_some_and(|data| unsafe { data.QueryGetData(&format(15)) }.is_ok()),
        );
        // SAFETY: OLE supplies a non-null writable effect pointer for the lifetime of this callback.
        self.allowed.set(unsafe { *effect });
        // SAFETY: OLE supplies a non-null writable effect pointer for the lifetime of this callback.
        unsafe {
            *effect = self.effect(keys, point);
        }
        Ok(())
    }
    fn DragOver(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: OLE supplies a non-null writable effect pointer for the lifetime of this callback.
        unsafe {
            *effect = self.effect(keys, point);
        }
        Ok(())
    }
    fn DragLeave(&self) -> windows::core::Result<()> {
        self.valid.set(false);
        Ok(())
    }
    fn Drop(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let chosen = self.effect(keys, point);
        // SAFETY: OLE supplies a non-null writable effect pointer for the lifetime of this callback.
        unsafe {
            *effect = DROPEFFECT_NONE;
        }
        if chosen == DROPEFFECT_NONE {
            return Ok(());
        }
        if let Some(destination) = self.destination_at(point)
            && let Some(data) = data.as_ref()
            && let Ok(paths) = files(data)
            && self
                .sender
                .send(Operation::Copy {
                    paths,
                    destination,
                    moving: chosen == DROPEFFECT_MOVE,
                })
                .is_ok()
        {
            // SAFETY: OLE supplies a non-null writable effect pointer for the lifetime of this callback.
            unsafe {
                *effect = chosen;
            }
            // SAFETY: this pointer-free wake targets our HWND and safely fails if it has closed.
            let _ = unsafe { PostMessageW(Some(HWND(self.hwnd as _)), WAKE, WPARAM(0), LPARAM(0)) };
        }
        Ok(())
    }
}

pub struct DropRegion {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub path: String,
}

pub struct DropTarget {
    hwnd: HWND,
    _target: IDropTarget,
    pub destination: Arc<Mutex<Option<String>>>,
    pub enabled: Arc<AtomicBool>,
    pub regions: Arc<Mutex<Vec<DropRegion>>>,
    pub requests: Receiver<Operation>,
}
impl DropTarget {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let destination = Arc::new(Mutex::new(None));
        let enabled = Arc::new(AtomicBool::new(false));
        let regions = Arc::new(Mutex::new(Vec::new()));
        let (sender, requests) = mpsc::channel();
        let target: IDropTarget = Target {
            enabled: enabled.clone(),
            destination: destination.clone(),
            regions: regions.clone(),
            sender,
            hwnd: hwnd.0 as usize,
            valid: Cell::new(false),
            allowed: Cell::new(DROPEFFECT_NONE),
        }
        .into();
        // SAFETY: the OLE UI thread registers its own live HWND with a retained IDropTarget.
        unsafe {
            RegisterDragDrop(hwnd, &target)?;
        }
        Ok(Self {
            hwnd,
            _target: target,
            destination,
            enabled,
            regions,
            requests,
        })
    }
}
impl Drop for DropTarget {
    fn drop(&mut self) {
        // SAFETY: this balances RegisterDragDrop for this window before its target is released.
        let _ = unsafe { RevokeDragDrop(self.hwnd) };
    }
}
