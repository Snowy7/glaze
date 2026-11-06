//! Attaching the TAP to a process with `InitializeXamlDiagnosticsEx`, the documented entry point
//! Visual Studio's Live Visual Tree uses. System XAML exports it from `Windows.UI.Xaml.dll`;
//! WinUI 3 from the Windows App SDK's `Microsoft.Internal.FrameworkUdk.dll`, loaded from the same
//! folder the target process uses.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mypaper_app_styler::{Attach, CLSID_TAP, Framework};
use windows::Win32::Foundation::{CloseHandle, ERROR_NOT_FOUND, HMODULE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};
use windows_core::{GUID, HRESULT, PCWSTR, s};

type InitializeXamlDiagnosticsEx = unsafe extern "system" fn(
    endpoint: PCWSTR,
    pid: u32,
    dll_xaml_diagnostics: PCWSTR,
    tap_dll: PCWSTR,
    tap_clsid: GUID,
    initialization_data: PCWSTR,
) -> HRESULT;

/// Loads the TAP into `pid` for `framework`. `tap` is our DLL's full path.
pub fn attach(pid: u32, framework: Framework, tap: &Path, data: &Attach) -> Result<()> {
    let (library, endpoint) = match framework {
        Framework::System => (system_library()?, "VisualDiagConnection"),
        Framework::WinUi => (winui_library(pid)?, "WinUIVisualDiagConnection"),
    };
    // SAFETY: a loaded module and a NUL-terminated export name.
    let export = unsafe { GetProcAddress(library, s!("InitializeXamlDiagnosticsEx")) }
        .context("InitializeXamlDiagnosticsEx is not exported")?;
    // SAFETY: the export has this signature (xamlOM.h).
    let initialize: InitializeXamlDiagnosticsEx = unsafe { std::mem::transmute(export) };
    let tap = wide(tap.as_os_str());
    let data = wide(data.encode().as_ref());
    // An endpoint name another tool (or an earlier run) holds fails with "not found": take the
    // next one, as Visual Studio does.
    for n in 1..=16 {
        let name = wide(format!("{endpoint}{n}").as_ref());
        // SAFETY: NUL-terminated strings that outlive the call.
        let hr = unsafe {
            initialize(
                PCWSTR(name.as_ptr()),
                pid,
                PCWSTR::null(),
                PCWSTR(tap.as_ptr()),
                CLSID_TAP,
                PCWSTR(data.as_ptr()),
            )
        };
        if hr.is_ok() {
            log::info!("attached to {pid} ({framework}, {endpoint}{n})");
            return Ok(());
        }
        if hr != ERROR_NOT_FOUND.to_hresult() {
            bail!("InitializeXamlDiagnosticsEx for {pid} ({framework}): {hr}");
        }
    }
    bail!("no free XAML Diagnostics endpoint for {pid}")
}

fn system_library() -> Result<HMODULE> {
    // SAFETY: a System32 DLL by name.
    unsafe {
        LoadLibraryExW(
            windows_core::w!("Windows.UI.Xaml.dll"),
            None,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .context("loading Windows.UI.Xaml.dll")
}

/// The Windows App SDK copy the target runs on: there can be several installed.
fn winui_library(pid: u32) -> Result<HMODULE> {
    let path = module_path(pid, "Microsoft.Internal.FrameworkUdk.dll")
        .context("the process has not loaded WinUI 3 (no Explorer window yet?)")?;
    let path_w = wide(path.as_os_str());
    // SAFETY: a full path; its dependencies resolve from its own folder.
    unsafe { LoadLibraryExW(PCWSTR(path_w.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .with_context(|| format!("loading {}", path.display()))
}

/// The full path of `name` as loaded in `pid`, if it is.
pub fn module_path(pid: u32, name: &str) -> Option<PathBuf> {
    // SAFETY: a snapshot handle we close below.
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) }.ok()?;
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    // SAFETY: a valid snapshot and a sized entry.
    let mut more = unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        let module = from_wide(&entry.szModule);
        if module.eq_ignore_ascii_case(name) {
            found = Some(PathBuf::from(from_wide_os(&entry.szExePath)));
            break;
        }
        // SAFETY: as above.
        more = unsafe { Module32NextW(snapshot, &mut entry) }.is_ok();
    }
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(snapshot) };
    found
}

fn from_wide(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

fn from_wide_os(buffer: &[u16]) -> OsString {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..len])
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}
