//! Durable write-ahead recovery for window parking and the session-only Snap switch.
//! Both shell and core use this file, only after the previous shell has exited.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, HWND},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub hwnd: isize,
    pub pid: u32,
    pub started: u64,
}
impl Identity {
    /// PID plus process creation time prevents restoring an HWND recycled after a crash.
    pub fn read(window: HWND) -> Result<Self> {
        let mut pid = 0;
        // SAFETY: read-only handle queries; the process handle is closed below.
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
            let (mut created, mut exit, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            let result = GetProcessTimes(p, &mut created, &mut exit, &mut kernel, &mut user);
            let _ = CloseHandle(p);
            result?;
            Ok(Self {
                hwnd: window.0 as isize,
                pid,
                started: (u64::from(created.dwHighDateTime) << 32)
                    | u64::from(created.dwLowDateTime),
            })
        }
    }
    pub fn window(self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
    pub fn valid(self) -> bool {
        Self::read(self.window()).is_ok_and(|now| now == self)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    identity: Identity,
    rect: [i32; 4],
    cookie: usize,
    normal: [i32; 4],
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    windows: Vec<Entry>,
    snap: Option<bool>,
}
pub struct Journal {
    path: PathBuf,
    data: Data,
}

pub fn path() -> Result<PathBuf> {
    // Scoped fixtures must never share a recovery file with the user's shell.
    if let Some(path) = std::env::var_os("MYPAPER_TILING_JOURNAL") {
        return Ok(path.into());
    }
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA missing")?)
            .join("mypaper/tiling-recovery.json"),
    )
}
impl Journal {
    pub fn open(path: PathBuf) -> Result<Self> {
        let data = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("reading tiling recovery; refusing to overwrite a corrupt journal")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, data })
    }
    fn save(&self) -> Result<()> {
        fs::create_dir_all(self.path.parent().context("journal has no parent")?)?;
        let temp = self.path.with_extension("json.tmp");
        let mut file = File::create(&temp)?;
        file.write_all(&serde_json::to_vec(&self.data)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, &self.path)?;
        Ok(())
    }
    /// Persist the unmodified outer rectangle before any layout or parking call.
    pub fn remember(&mut self, identity: Identity) -> Result<()> {
        if self.data.windows.iter().any(|e| {
            e.identity == identity
                && unsafe { GetPropW(identity.window(), w!("mypaper.tiling.recovery")) }.0 as usize
                    == e.cookie
        }) {
            return Ok(());
        }
        self.data.windows.retain(|e| e.identity != identity);
        ensure!(
            identity.valid(),
            "window changed identity before journaling"
        );
        let mut r = windows::Win32::Foundation::RECT::default();
        // SAFETY: valid output buffer; no state changes.
        unsafe {
            GetWindowRect(identity.window(), &mut r)?;
        }
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: initialized placement struct; reading state before any mutation.
        unsafe {
            GetWindowPlacement(identity.window(), &mut placement)?;
        }
        let normal = placement.rcNormalPosition;
        let cookie = (identity.started as usize ^ identity.hwnd as usize).max(1);
        self.data.windows.push(Entry {
            identity,
            rect: [r.left, r.top, r.right - r.left, r.bottom - r.top],
            cookie,
            normal: [normal.left, normal.top, normal.right, normal.bottom],
        });
        if let Err(e) = self.save() {
            self.data.windows.pop();
            return Err(e);
        }
        // SAFETY: a per-window recovery token, removed on restore. A recycled HWND has no token.
        unsafe {
            SetPropW(
                identity.window(),
                w!("mypaper.tiling.recovery"),
                Some(HANDLE(cookie as *mut _)),
            )?;
        }
        Ok(())
    }
    /// A now-visible fullscreen or topmost window is handed back to its app without moving it.
    pub fn release(&mut self, identity: Identity) -> Result<()> {
        self.data.windows.retain(|e| e.identity != identity);
        self.save()?;
        // SAFETY: our token is removed only from the same live window identity.
        if identity.valid() {
            let _ = unsafe { RemovePropW(identity.window(), w!("mypaper.tiling.recovery")) };
        }
        Ok(())
    }
    /// Snap is changed for this session, never persisted to the user's profile.
    pub fn disable_snap(&mut self) -> Result<()> {
        let mut on = windows::core::BOOL::default();
        // SAFETY: correctly sized BOOL; no SPIF_UPDATEINIFILE in either call.
        unsafe {
            SystemParametersInfoW(
                SPI_GETWINARRANGING,
                0,
                Some((&mut on as *mut windows::core::BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )?;
            self.data.snap = Some(on.as_bool());
            self.save()?;
            SystemParametersInfoW(
                SPI_SETWINARRANGING,
                0,
                Some(std::ptr::null_mut()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )?;
        }
        Ok(())
    }
    /// Restore live identities only. Failed moves remain journaled for the next recovery.
    pub fn restore(&mut self) -> Result<()> {
        let monitors = crate::monitors();
        let mut failed = Vec::new();
        self.data.windows.retain(|e| {
            // SAFETY: properties are tied to the window lifetime, unlike a numeric HWND.
            if !e.identity.valid()
                || unsafe { GetPropW(e.identity.window(), w!("mypaper.tiling.recovery")) }.0
                    as usize
                    != e.cookie
            {
                return false;
            }
            // An app that became fullscreen or PiP has already rescued itself. Hand it back
            // without changing its game/presentation rectangle, including after a shell crash.
            let h = e.identity.window();
            let mut actual = windows::Win32::Foundation::RECT::default();
            // SAFETY: read-only native state and a valid output rectangle.
            let leave_alone = unsafe {
                GetWindowRect(h, &mut actual).is_ok() && {
                    let on_screen = monitors.iter().any(|m| {
                        actual.right > m.x
                            && actual.bottom > m.y
                            && actual.left < m.x + m.width
                            && actual.top < m.y + m.height
                    });
                    let topmost = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
                    let fullscreen = GetWindowLongPtrW(h, GWL_STYLE) as u32 & WS_CAPTION.0 == 0
                        && monitors.iter().any(|m| {
                            actual.left <= m.x
                                && actual.top <= m.y
                                && actual.right >= m.x + m.width
                                && actual.bottom >= m.y + m.height
                        });
                    on_screen && (topmost || fullscreen)
                }
            };
            if leave_alone {
                let _ = unsafe { RemovePropW(h, w!("mypaper.tiling.recovery")) };
                return false;
            }
            let [mut x, mut y, mut w, mut h] = e.rect;
            if !monitors.iter().any(|m| m.contains_point(x, y)) {
                let Some(m) = monitors.first() else {
                    return true;
                };
                w = w.min(m.width).max(1);
                h = h.min(m.height).max(1);
                x = m.x + (m.width - w) / 2;
                y = m.y + (m.height - h) / 2;
            }
            // SAFETY: identity and property validated above. Minimized windows keep their state
            // while their normal placement is rescued; a hung app retains its journal entry.
            let result = unsafe {
                if IsIconic(e.identity.window()).as_bool() {
                    let mut placement = WINDOWPLACEMENT {
                        length: size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    GetWindowPlacement(e.identity.window(), &mut placement).and_then(|()| {
                        let [left, top, right, bottom] = e.normal;
                        placement.rcNormalPosition = windows::Win32::Foundation::RECT {
                            left,
                            top,
                            right,
                            bottom,
                        };
                        SetWindowPlacement(e.identity.window(), &placement)
                    })
                } else {
                    let mut response = 0;
                    let hung = SendMessageTimeoutW(
                        e.identity.window(),
                        WM_NULL,
                        Default::default(),
                        Default::default(),
                        SMTO_ABORTIFHUNG | SMTO_BLOCK,
                        20,
                        Some(&mut response),
                    )
                    .0 == 0;
                    let result = SetWindowPos(
                        e.identity.window(),
                        None,
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE
                            | SWP_NOZORDER
                            | if hung {
                                SWP_ASYNCWINDOWPOS
                            } else {
                                SET_WINDOW_POS_FLAGS(0)
                            },
                    );
                    if hung {
                        failed.push("unresponsive window recovery queued".into());
                        return true;
                    }
                    result
                }
            };
            if let Err(error) = result {
                failed.push(error.to_string());
                return true;
            }
            // SAFETY: remove only our recovery token after successful restoration.
            let _ = unsafe { RemovePropW(e.identity.window(), w!("mypaper.tiling.recovery")) };
            false
        });
        if let Some(on) = self.data.snap {
            // SAFETY: SPI's pvParam is a boolean value for this action, session-only.
            unsafe {
                SystemParametersInfoW(
                    SPI_SETWINARRANGING,
                    0,
                    Some(usize::from(on) as *mut _),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                )?;
            }
            self.data.snap = None;
        }
        if self.data.windows.is_empty() {
            if self.path.exists() {
                fs::remove_file(&self.path)?;
            }
        } else {
            self.save()?;
        }
        ensure!(
            failed.is_empty() && self.data.windows.is_empty(),
            "some tiling windows could not be restored: {}",
            failed.join(", ")
        );
        Ok(())
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        if !self.data.windows.is_empty() || self.data.snap.is_some() {
            if let Err(e) = self.restore() {
                log::error!("tiling journal cleanup: {e:#}");
            }
        }
    }
}
/// Recovery is a no-op without a file, including ordinary starts with tiling disabled.
pub fn restore_pending() -> Result<()> {
    let path = path()?;
    restore_at(&path)
}
pub fn restore_at(path: &Path) -> Result<()> {
    if path.exists() {
        Journal::open(path.into())?.restore()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_recovery_is_preserved_instead_of_silently_discarded() {
        let path = std::env::temp_dir().join(format!(
            "mypaper-tiling-corrupt-{}.json",
            std::process::id()
        ));
        let bytes = b"{interrupted recovery";
        fs::write(&path, bytes).unwrap();
        assert!(Journal::open(path.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn stale_process_identity_is_retired_without_moving_a_recycled_handle() {
        let path =
            std::env::temp_dir().join(format!("mypaper-tiling-stale-{}.json", std::process::id()));
        let journal = Journal {
            path: path.clone(),
            data: Data {
                windows: vec![Entry {
                    identity: Identity {
                        hwnd: 0,
                        pid: 0,
                        started: 0,
                    },
                    rect: [-100, 0, 100, 100],
                    cookie: 1,
                    normal: [0, 0, 100, 100],
                }],
                snap: None,
            },
        };
        journal.save().unwrap();
        std::mem::forget(journal);
        restore_at(&path).unwrap();
        assert!(!path.exists());
    }
}
