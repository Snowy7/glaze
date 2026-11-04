//! The dock's Recycle Bin item: full or empty (`SHQueryRecycleBinW`), kept current by shell
//! change notifications on the bin (`SHChangeNotifyRegister`, delivered to the broadcast window
//! as [`WM_TRASH_NOTIFY`]); registered only while a dock shows it.

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CSIDL_BITBUCKET, SHCNE_CREATE, SHCNE_DELETE, SHCNE_MKDIR, SHCNE_RENAMEFOLDER, SHCNE_RENAMEITEM,
    SHCNE_RMDIR, SHCNE_UPDATEDIR, SHCNE_UPDATEITEM, SHCNRF_InterruptLevel, SHCNRF_ShellLevel,
    SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister, SHEmptyRecycleBinW,
    SHGetSpecialFolderLocation, SHQUERYRBINFO, SHQueryRecycleBinW,
};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::core::PCWSTR;

/// Sent by the shell to the broadcast window when the bin changed.
pub const WM_TRASH_NOTIFY: u32 = WM_APP + 31;

/// Whether the Recycle Bin (all drives) holds anything.
pub fn full() -> bool {
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` has its size set and outlives the call.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.is_ok() && info.i64NumItems > 0
}

/// Asks to empty the bin (the shell confirms), off the main thread: the dialog blocks.
pub fn empty() {
    let _ = std::thread::Builder::new()
        .name("empty trash".into())
        .spawn(|| {
            // SAFETY: no window, all drives, the shell's own confirmation and sound.
            if let Err(e) = unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), 0) } {
                log::info!("emptying the Recycle Bin: {e}");
            }
        });
}

/// A registration for the bin's change notifications; deregistered on drop.
pub struct TrashWatch(u32);

impl TrashWatch {
    pub fn new(window: HWND) -> Option<Self> {
        // SAFETY: the pidl is ours and freed after registering (the shell copies it).
        unsafe {
            let pidl: *mut ITEMIDLIST =
                SHGetSpecialFolderLocation(None, CSIDL_BITBUCKET as i32).ok()?;
            let entry = SHChangeNotifyEntry {
                pidl,
                fRecursive: true.into(),
            };
            let events = SHCNE_CREATE
                | SHCNE_DELETE
                | SHCNE_MKDIR
                | SHCNE_RMDIR
                | SHCNE_UPDATEDIR
                | SHCNE_UPDATEITEM
                | SHCNE_RENAMEITEM
                | SHCNE_RENAMEFOLDER;
            let id = SHChangeNotifyRegister(
                window,
                SHCNRF_ShellLevel | SHCNRF_InterruptLevel,
                events.0 as i32,
                WM_TRASH_NOTIFY,
                1,
                &entry,
            );
            CoTaskMemFree(Some(pidl as *const c_void));
            if id == 0 {
                log::warn!("no Recycle Bin notifications");
                return None;
            }
            Some(Self(id))
        }
    }
}

impl Drop for TrashWatch {
    fn drop(&mut self) {
        // SAFETY: a registration we made, removed once.
        let _ = unsafe { SHChangeNotifyDeregister(self.0) };
    }
}
