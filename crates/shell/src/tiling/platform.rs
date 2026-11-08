//! Read-only admission checks and the single batched Win32 placement boundary.
use anyhow::{Context, Result, ensure};
use mypaper_desktop::tiling_recovery::Identity;
use mypaper_tiling::{Rect, WindowId};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, RECT},
        Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Security::{
            GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
            TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
        },
        System::Threading::{
            OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::*,
    },
    core::PWSTR,
};
static ELEVATION_HINTED: AtomicBool = AtomicBool::new(false);
/// Kernel-side eligibility only: safe before queuing an intake event or querying another process.
pub fn intake_candidate(h: HWND) -> bool {
    // SAFETY: these read cached USER window state; they do not message a client window procedure.
    unsafe {
        if !IsWindow(Some(h)).as_bool()
            || !IsWindowVisible(h).as_bool()
            || GetAncestor(h, GA_ROOT) != h
        {
            return false;
        }
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        style & WS_CAPTION.0 == WS_CAPTION.0
            && style & WS_CHILD.0 == 0
            && ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0) == 0
    }
}
pub fn desktop_window(h: HWND) -> bool {
    // SAFETY: read-only desktop HWND queries; neither activates a window.
    unsafe { h == GetDesktopWindow() || h == GetShellWindow() }
}
pub fn hwnd(id: WindowId) -> HWND {
    HWND(id.0 as usize as *mut _)
}
pub fn id(h: HWND) -> WindowId {
    WindowId(h.0 as usize as u64)
}
pub fn foreground() -> HWND {
    // SAFETY: read-only query; no activation is requested.
    unsafe { GetForegroundWindow() }
}
pub fn zoomed(h: HWND) -> bool {
    // SAFETY: stale handles read as false.
    unsafe { IsZoomed(h) }.as_bool()
}
pub fn visible(h: HWND) -> bool {
    // SAFETY: stale handles read as false.
    unsafe { IsWindowVisible(h) }.as_bool()
}
pub fn iconic(h: HWND) -> bool {
    // SAFETY: stale handles read as false.
    unsafe { IsIconic(h) }.as_bool()
}
pub fn topmost(h: HWND) -> bool {
    // SAFETY: read-only style query; no client style is changed.
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}
pub fn cursor() -> Option<windows::Win32::Foundation::POINT> {
    let mut point = windows::Win32::Foundation::POINT::default();
    // SAFETY: read-only pointer query into our local output buffer.
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some(point)
}
pub fn dpi_scale(h: HWND) -> f64 {
    // SAFETY: read-only DPI query; a disappearing HWND falls back to 100%.
    f64::from(unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(h) }).max(96.0) / 96.0
}
pub fn notification_game() -> bool {
    use windows::Win32::UI::Shell::{
        QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    };
    // SAFETY: only reads the notification presentation state.
    unsafe { SHQueryUserNotificationState() }
        .is_ok_and(|s| matches!(s, QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE))
}
pub fn rect(r: RECT) -> Rect {
    Rect {
        x: f64::from(r.left),
        y: f64::from(r.top),
        w: f64::from(r.right - r.left),
        h: f64::from(r.bottom - r.top),
    }
}
pub fn frame(h: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: correctly sized output buffer; fallback is another read-only query.
    unsafe {
        if DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut r as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
        .is_err()
        {
            GetWindowRect(h, &mut r).ok()?;
        }
    }
    Some(rect(r))
}
pub fn outer(h: HWND) -> Option<Rect> {
    let mut r = RECT::default(); /* SAFETY: valid output buffer. */
    unsafe {
        GetWindowRect(h, &mut r).ok()?;
    }
    Some(rect(r))
}
pub fn class(h: HWND) -> String {
    let mut text = [0; 256]; /* SAFETY: bounded writable buffer. */
    let n = unsafe { GetClassNameW(h, &mut text) };
    String::from_utf16_lossy(&text[..n.max(0) as usize])
}
pub fn title(h: HWND) -> String {
    let mut text = [0; 1024]; /* SAFETY: top-level title query does not send WM_GETTEXT to another process. */
    let n = unsafe { GetWindowTextW(h, &mut text) };
    String::from_utf16_lossy(&text[..n.max(0) as usize])
}

/// A present but invalid PID scope is an error, never permission to manage the desktop.
pub fn scope() -> Result<Option<HashSet<u32>>> {
    let Some(value) = std::env::var_os("MYPAPER_TILING_PIDS") else {
        return Ok(None);
    };
    let mut ids = HashSet::new();
    for part in value.to_string_lossy().split(',') {
        let pid = part
            .trim()
            .parse::<u32>()
            .context("invalid MYPAPER_TILING_PIDS")?;
        ensure!(pid != 0, "zero PID in scope");
        ids.insert(pid);
    }
    ensure!(!ids.is_empty(), "empty tiling PID scope");
    Ok(Some(ids))
}
pub fn allowed(h: HWND, scope: &Option<HashSet<u32>>) -> bool {
    let mut pid = 0; /* SAFETY: read-only query. */
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    scope.as_ref().is_none_or(|s| s.contains(&pid))
}

fn process_info(pid: u32) -> Result<(String, u32)> {
    // SAFETY: all handles are owned and closed; token buffers are sized before use.
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
        let result = (|| -> Result<_> {
            let mut path = vec![0; 32768];
            let mut len = path.len() as u32;
            QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len)?;
            let exe = String::from_utf16_lossy(&path[..len as usize])
                .rsplit('\\')
                .next()
                .unwrap_or("")
                .to_owned();
            let mut token = HANDLE::default();
            OpenProcessToken(p, TOKEN_QUERY, &mut token)?;
            let token_result = (|| -> Result<u32> {
                let mut size = 0;
                let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut size);
                ensure!(
                    size as usize >= size_of::<TOKEN_MANDATORY_LABEL>(),
                    "invalid integrity token"
                );
                let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
                GetTokenInformation(
                    token,
                    TokenIntegrityLevel,
                    Some(buffer.as_mut_ptr().cast()),
                    size,
                    &mut size,
                )?;
                let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
                let count = *GetSidSubAuthorityCount(label.Label.Sid);
                ensure!(count > 0, "empty integrity SID");
                Ok(*GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)))
            })();
            let _ = CloseHandle(token);
            Ok((exe, token_result?))
        })();
        let _ = CloseHandle(p);
        result
    }
}
pub fn integrity(pid: u32) -> Result<u32> {
    process_info(pid).map(|(_, integrity)| integrity)
}
#[derive(Debug)]
pub struct Admission {
    pub identity: Identity,
    pub exe: String,
    pub owner: Option<WindowId>,
    pub floating: bool,
    pub frame: Rect,
    pub border: [f64; 4],
}

/// Cheap window/style checks precede token, executable, title and DWM queries.
pub fn admit(h: HWND, scope: &Option<HashSet<u32>>) -> Option<Admission> {
    // SAFETY: window/style queries tolerate a disappearing HWND.
    unsafe {
        if !IsWindow(Some(h)).as_bool()
            || !IsWindowVisible(h).as_bool()
            || IsIconic(h).as_bool()
            || GetAncestor(h, GA_ROOT) != h
            || !allowed(h, scope)
        {
            return None;
        }
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if style & WS_CHILD.0 != 0
            || style & WS_CAPTION.0 != WS_CAPTION.0
            || ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0) != 0
        {
            return None;
        }
        let class = class(h);
        if matches!(
            class.as_str(),
            "Progman"
                | "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "#32768"
                | "Chrome_RenderWidgetHostHWND"
                | "Windows.UI.Core.CoreWindow"
        ) || class.starts_with("mypaper.")
        {
            return None;
        }
        let mut cloak = 0u32;
        if DwmGetWindowAttribute(h, DWMWA_CLOAKED, (&mut cloak as *mut u32).cast(), 4).is_err()
            || cloak != 0
        {
            return None;
        }
        let identity = Identity::read(h).ok()?;
        if identity.pid == std::process::id() {
            return None;
        }
        let (mut exe, mut integrity) = process_info(identity.pid).ok()?;
        if class == "ApplicationFrameWindow" {
            let mut child_pid = 0;
            unsafe extern "system" fn child(h: HWND, p: LPARAM) -> windows::core::BOOL {
                if self::class(h) == "Windows.UI.Core.CoreWindow" {
                    /* SAFETY: synchronous callback and stack PID buffer. */
                    unsafe {
                        GetWindowThreadProcessId(h, Some(&mut *(p.0 as *mut u32)));
                    }
                    return false.into();
                }
                true.into()
            }
            let _ = EnumChildWindows(
                Some(h),
                Some(child),
                LPARAM(&mut child_pid as *mut u32 as isize),
            );
            if child_pid != 0 {
                let info = process_info(child_pid).ok()?;
                exe = info.0;
                integrity = integrity.max(info.1);
            }
        }
        // Even a mistakenly elevated shell must never begin managing elevated apps.
        if integrity > 0x2000 {
            if !ELEVATION_HINTED.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "Scrolling tiling leaves elevated windows alone; launch an app without elevation to tile it"
                );
            }
            return None;
        }
        if ex & WS_EX_LAYERED.0 != 0 {
            return None;
        }
        let frame = frame(h)?;
        let outer = outer(h)?;
        if frame.w < 32.0 || frame.h < 32.0 {
            return None;
        }
        let owner = GetWindow(h, GW_OWNER)
            .ok()
            .filter(|h| !h.is_invalid())
            .map(id);
        Some(Admission {
            identity,
            exe,
            owner,
            floating: owner.is_some() || style & WS_THICKFRAME.0 == 0 || class == "#32770",
            frame,
            border: [
                outer.x - frame.x,
                outer.y - frame.y,
                outer.w - frame.w,
                outer.h - frame.h,
            ],
        })
    }
}
pub fn fullscreen(h: HWND) -> bool {
    // SAFETY: cheap style read plus a cached frame query.
    unsafe {
        if GetWindowLongPtrW(h, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0
            || IsZoomed(h).as_bool()
        {
            return false;
        }
    }
    frame(h).is_some_and(|r| {
        mypaper_desktop::monitors().iter().any(|m| {
            r.x <= f64::from(m.x) + 1.0
                && r.y <= f64::from(m.y) + 1.0
                && r.x + r.w >= f64::from(m.x + m.width) - 1.0
                && r.y + r.h >= f64::from(m.y + m.height) - 1.0
        })
    })
}
pub fn hands_off(h: HWND) -> bool {
    // SAFETY: a read-only style query; HWND_TOPMOST is never applied to managed clients.
    (unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }) || fullscreen(h)
}
pub fn responsive(h: HWND) -> bool {
    let mut result = 0;
    // SAFETY: bounded 20 ms WM_NULL probe, no pointer parameters or process injection.
    unsafe {
        SendMessageTimeoutW(
            h,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            20,
            Some(&mut result),
        )
        .0 != 0
    }
}

use windows::Win32::Foundation::{LPARAM, WPARAM};
