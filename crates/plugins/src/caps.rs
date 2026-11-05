//! Capability implementations behind the WIT imports: network origin checks, WinHTTP, secrets in
//! Credential Manager, per-plugin storage, launch and read-only system info. Permission checks
//! happen in `runtime.rs` before these run.
use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Credentials::*;
use windows::core::{HSTRING, PCWSTR, w};

const MAX_RESPONSE: usize = 4 << 20;
pub const MAX_VALUE: usize = 1 << 20;

/// `(scheme, host, port)` of an `http(s)://` or `ws(s)://` URL or origin.
fn origin(url: &str) -> Option<(&str, &str, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return None;
    }
    let default = match scheme {
        "https" | "wss" => 443,
        "http" | "ws" => 80,
        _ => return None,
    };
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((scheme, host, port.parse().ok()?)),
        None => Some((scheme, authority, default)),
    }
}

/// Whether a `network:<origin>` permission covers `url` (exact scheme and port; `*.` subdomains).
pub fn origin_allowed(permissions: &[String], url: &str) -> bool {
    let Some((scheme, host, port)) = origin(url) else { return false };
    let host = host.to_ascii_lowercase();
    permissions.iter().filter_map(|p| p.strip_prefix("network:")).filter_map(origin).any(|(s, h, p)| {
        let h = h.to_ascii_lowercase();
        let host_ok = match h.strip_prefix("*.") {
            Some(base) => host.ends_with(&format!(".{base}")),
            None => host == h,
        };
        s == scheme && p == port && host_ok
    })
}

struct Handle(*mut c_void);
impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, String> {
        if raw.is_null() { Err(windows::core::Error::from_thread().to_string()) } else { Ok(Self(raw)) }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: each successful WinHTTP open is owned here and closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

pub struct Fetched {
    pub status: u16,
    pub body: Vec<u8>,
}

/// One blocking request on a worker thread. Windows owns TLS, proxies and certificates.
pub fn fetch(method: &str, url: &str, headers: &[(String, String)], body: Option<&[u8]>, timeout_ms: u32) -> Result<Fetched, String> {
    let (scheme, host, port) = origin(url).ok_or("expected an http(s) URL")?;
    let secure = match scheme {
        "https" => true,
        "http" => false,
        _ => return Err("expected an http(s) URL".into()),
    };
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let path = rest.find('/').map_or("/".to_owned(), |i| rest[i..].to_owned());
    let timeout = timeout_ms.clamp(1000, 60_000) as i32;
    let header_text: Vec<u16> = headers.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect::<String>().encode_utf16().collect();
    // SAFETY: handles are RAII values, every string and buffer outlives the call using it, and
    // reads stay within `chunk`; the response is bounded by MAX_RESPONSE.
    unsafe {
        let session = Handle::new(WinHttpOpen(w!("mypaper-plugins/0.1"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, timeout, timeout, timeout, timeout).map_err(|e| e.to_string())?;
        let connection = Handle::new(WinHttpConnect(session.0, &HSTRING::from(host), port, 0))?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = Handle::new(WinHttpOpenRequest(connection.0, &HSTRING::from(method), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags))?;
        // Redirects must not carry an authorized request or its secret to an undeclared host.
        WinHttpSetOption(request.0, WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes())).map_err(|e| e.to_string())?;
        let headers = (!header_text.is_empty()).then_some(header_text.as_slice());
        let (data, len) = body.map_or((None, 0), |b| (Some(b.as_ptr().cast::<c_void>()), b.len() as u32));
        WinHttpSendRequest(request.0, headers, data, len, len, 0).map_err(|e| e.to_string())?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
        let (mut status, mut length) = (0u32, 4u32);
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&raw mut status).cast()), &mut length, std::ptr::null_mut())
            .map_err(|e| e.to_string())?;
        let mut output = Vec::new();
        let mut chunk = [0u8; 16384];
        loop {
            let mut count = 0;
            WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut count).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            if output.len() + count as usize > MAX_RESPONSE {
                return Err("response exceeds 4 MB".into());
            }
            output.extend_from_slice(&chunk[..count as usize]);
        }
        Ok(Fetched { status: status as u16, body: output })
    }
}

/// `mypaper/integrations/plugin/<id>/<key>`, the same target the native plugin host uses.
fn credential(plugin: &str, key: &str) -> Result<Vec<u16>, String> {
    let ok = |s: &str| !s.is_empty() && s.len() <= 96 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
    if !ok(plugin) || key.len() > 128 || !key.split('/').all(ok) {
        return Err("keys contain 1-96 character [A-Za-z0-9_-] segments".into());
    }
    // Scoped first-party keys preserve credentials entered in Studio before plugin migration.
    let target = if key.starts_with(&format!("{plugin}/")) {
        format!("mypaper/integrations/{key}\0")
    } else {
        format!("mypaper/integrations/plugin/{plugin}/{key}\0")
    };
    Ok(target.encode_utf16().collect())
}

pub fn secret_get(plugin: &str, key: &str) -> Result<Option<String>, String> {
    let name = credential(plugin, key)?;
    let mut found = std::ptr::null_mut();
    // SAFETY: Win32 allocates the result; it is copied, then freed exactly once.
    unsafe {
        if CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut found).is_err() {
            return Ok(None);
        }
        let cred = &*found;
        let bytes = if cred.CredentialBlobSize == 0 { Vec::new() } else { std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize).to_vec() };
        CredFree(found.cast());
        String::from_utf8(bytes).map(Some).map_err(|_| "not UTF-8".into())
    }
}

pub fn secret_set(plugin: &str, key: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 2560 {
        return Err("secrets are 1-2560 bytes".into());
    }
    let mut name = credential(plugin, key)?;
    let mut bytes = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: windows::core::PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: all buffers are owned and alive for this synchronous call.
    let result = unsafe { CredWriteW(&cred, 0) };
    bytes.fill(0);
    result.map_err(|e| e.to_string())
}

pub fn secret_remove(plugin: &str, key: &str) -> Result<(), String> {
    let name = credential(plugin, key)?;
    // SAFETY: the null-terminated target is alive during the call.
    let _ = unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) };
    Ok(())
}

/// Storage keys become hex file names, so any key is safe on disk.
fn kv_path(data_dir: &Path, key: &str) -> Result<std::path::PathBuf, String> {
    if key.is_empty() || key.len() > 128 {
        return Err("keys are 1-128 bytes".into());
    }
    let name: String = key.bytes().map(|b| format!("{b:02x}")).collect();
    Ok(data_dir.join("kv").join(name))
}

pub fn storage_get(data_dir: &Path, key: &str) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(kv_path(data_dir, key)?) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn storage_set(data_dir: &Path, key: &str, value: &[u8]) -> Result<(), String> {
    if value.len() > MAX_VALUE {
        return Err("values are at most 1 MiB".into());
    }
    let path = kv_path(data_dir, key)?;
    std::fs::create_dir_all(path.parent().expect("kv dir")).map_err(|e| e.to_string())?;
    std::fs::write(path, value).map_err(|e| e.to_string())
}

pub fn storage_remove(data_dir: &Path, key: &str) -> Result<(), String> {
    match std::fs::remove_file(kv_path(data_dir, key)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

pub fn launch(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // SAFETY: the strings outlive the call; ShellExecuteW returns > 32 on success.
    let code = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if code.0 as usize > 32 { Ok(()) } else { Err(format!("ShellExecute failed ({})", code.0 as usize)) }
}

/// Public app status only. A stale marker cannot make a stopped app appear signed in.
pub fn desktop_status(app: &str) -> Result<crate::runtime::mypaper::plugin::launch::DesktopStatus, String> {
    use std::io::Read;
    use std::path::PathBuf;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    if app.is_empty() || app.len() > 96 || !app.bytes().all(|b| b.is_ascii_alphanumeric() || b" -_".contains(&b)) {
        return Err("app must be one application folder name".into());
    }
    let root = |name| std::env::var_os(name).map(PathBuf::from).unwrap_or_default();
    let mut installed = root("LOCALAPPDATA").join("Programs").join(app).join(format!("{app}.exe")).is_file();
    let marker = std::fs::File::open(root("APPDATA").join(app).join("desktop-status.json")).ok().and_then(|file| {
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= 4096).then(|| String::from_utf8(bytes).ok()).flatten()
    });
    let pid = marker.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()).and_then(|v| v["pid"].as_u64()).and_then(|v| u32::try_from(v).ok());
    let mut running = false;
    if let Some(pid) = pid {
        // SAFETY: read-only process query; the handle and output buffer are owned here.
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut name = [0u16; 32768];
                let mut size = name.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(name.as_mut_ptr()), &mut size).is_ok() {
                    let image = String::from_utf16_lossy(&name[..size as usize]);
                    running = image.rsplit('\\').next().is_some_and(|name| name.eq_ignore_ascii_case(&format!("{app}.exe")));
                }
                let _ = windows::Win32::Foundation::CloseHandle(process);
            }
        }
    }
    installed |= running;
    Ok(crate::runtime::mypaper::plugin::launch::DesktopStatus { installed, running, marker })
}

/// Whether `process:<name>` names a plain executable name (no path, no extension tricks).
pub fn process_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && !name.starts_with('.')
}

const RUNNABLE: [&str; 4] = ["exe", "cmd", "bat", "com"];

/// The executable `exe` names, if a declared `process:<name>` covers it: a bare name searched on
/// `PATH` with the runnable extensions, or an absolute path whose stem is the declared name.
pub fn resolve_exe(permissions: &[String], exe: &str) -> Result<std::path::PathBuf, String> {
    let declared = |stem: &str| permissions.iter().filter_map(|p| p.strip_prefix("process:")).any(|n| n.eq_ignore_ascii_case(stem));
    let path = Path::new(exe);
    if path.is_absolute() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !declared(stem) || !RUNNABLE.contains(&ext.as_str()) {
            return Err(format!("the manifest doesn't declare process:{stem}"));
        }
        return if path.is_file() { Ok(path.to_owned()) } else { Err(format!("{exe} doesn't exist")) };
    }
    if !process_name_ok(exe) || !declared(exe) {
        return Err(format!("the manifest doesn't declare process:{exe}"));
    }
    let dirs = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&dirs)
        .flat_map(|dir| RUNNABLE.map(|ext| dir.join(format!("{exe}.{ext}"))))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{exe} isn't on PATH"))
}

pub struct Ran {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs `exe` hidden with empty stdin, reading at most 1 MiB of each stream; killed at `timeout`.
pub fn run_process(exe: &Path, args: &[String], timeout: std::time::Duration) -> Result<Ran, String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = std::process::Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting {}: {e}", exe.display()))?;
    let drain = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(stream) = stream {
                let _ = stream.take(MAX_VALUE as u64).read_to_end(&mut out);
            }
            out
        })
    };
    let stdout = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} timed out", exe.display()));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };
    Ok(Ran { code: status.code().unwrap_or(-1), stdout: stdout.join().unwrap_or_default(), stderr: stderr.join().unwrap_or_default() })
}

/// Whether `file:<path>` is a profile path (`~/...`) with no `..`, drive or stream parts.
pub fn file_path_ok(path: &str) -> bool {
    path.strip_prefix("~/").is_some_and(|rest| !rest.is_empty() && !rest.contains(':') && !rest.contains('\\') && rest.split('/').all(|part| !part.is_empty() && part != "." && part != ".."))
}

/// Reads a declared profile file (`~/...`), at most 1 MiB; `None` when it doesn't exist.
pub fn read_file(path: &str) -> Result<Option<Vec<u8>>, String> {
    if !file_path_ok(path) {
        return Err("expected ~/<path>".into());
    }
    let home = std::env::var_os("USERPROFILE").ok_or("no USERPROFILE")?;
    let full = path[2..].split('/').fold(std::path::PathBuf::from(home), |p, part| p.join(part));
    match std::fs::metadata(&full) {
        Ok(meta) if meta.len() > MAX_VALUE as u64 => Err("files are at most 1 MiB".into()),
        Ok(_) => std::fs::read(&full).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub struct Monitor {
    pub id: String,
    pub rect: (i32, i32, u32, u32),
    pub primary: bool,
}

pub fn monitors() -> Vec<Monitor> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
    use windows::core::BOOL;
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the Vec passed below, alive for the enumeration.
        let out = unsafe { &mut *(out.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` is a correctly sized MONITORINFOEXW.
        if unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
            let r = info.monitorInfo.rcMonitor;
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
            out.push(Monitor {
                id: String::from_utf16_lossy(&info.szDevice[..len]),
                rect: (r.left, r.top, (r.right - r.left) as u32, (r.bottom - r.top) as u32),
                primary: info.monitorInfo.dwFlags & 1 != 0,
            });
        }
        true.into()
    }
    let mut out = Vec::new();
    // SAFETY: the callback only touches `out`, which outlives the call.
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM((&raw mut out) as isize)) };
    out
}

/// `(on_battery, percent, saver)`.
pub fn power() -> (bool, Option<u8>, bool) {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid out pointer.
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return (false, None, false);
    }
    (status.ACLineStatus == 0, (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent), status.SystemStatusFlag == 1)
}

/// Whether apps use the dark theme (the personalization setting).
pub fn dark() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut value = 1u32;
    let mut size = 4u32;
    // SAFETY: `value` and `size` are valid out pointers of the declared size.
    let read = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some((&raw mut value).cast()), Some(&mut size))
    };
    read.is_ok() && value == 0
}

pub struct Window {
    pub id: u64,
    pub title: String,
    pub process: String,
    pub rect: (i32, i32, u32, u32),
}

/// The foreground window: its handle as the id, title, process image name and rect.
pub fn foreground() -> Option<Window> {
    use windows::Win32::Foundation::{CloseHandle, RECT};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId};
    // SAFETY: plain queries on the foreground window; buffers are sized and owned here, and the
    // process handle is closed.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut process = String::new();
        if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path = [0u16; 1024];
            let mut size = path.len() as u32;
            if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(path.as_mut_ptr()), &mut size).is_ok() {
                let full = String::from_utf16_lossy(&path[..size as usize]);
                process = full.rsplit('\\').next().unwrap_or_default().to_owned();
            }
            let _ = CloseHandle(handle);
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        Some(Window { id: hwnd.0 as u64, title: String::from_utf16_lossy(&title[..len]), process, rect: (r.left, r.top, (r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32) })
    }
}

/// mypaper's toast identity for plugins (an unpackaged AppUserModelID, registered on first use).
pub const TOAST_APP: &str = "mypaper.Plugins";

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The toast XML: the plugin's title and body, attributed "via <plugin name>".
pub fn toast_xml(plugin_name: &str, title: &str, body: &str) -> String {
    format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text><text placement="attribution">via {}</text></binding></visual></toast>"#,
        xml_escape(title),
        xml_escape(body),
        xml_escape(plugin_name)
    )
}

/// Shows a Windows toast from mypaper on a worker thread (WinRT activation can take a while).
pub fn notify(plugin_name: &str, title: &str, body: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegSetValueExW};
    // SAFETY: registers the AppUserModelID's display name under HKCU (idempotent); strings
    // outlive the calls and the key is closed.
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyW(HKEY_CURRENT_USER, &HSTRING::from(format!(r"Software\Classes\AppUserModelId\{TOAST_APP}")), &mut key).is_ok() {
            let name: Vec<u8> = "mypaper\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
            let _ = RegSetValueExW(key, w!("DisplayName"), None, REG_SZ, Some(&name));
            let _ = RegCloseKey(key);
        }
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let show = || -> windows::core::Result<()> {
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(toast_xml(plugin_name, title, body)))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(TOAST_APP))?.Show(&toast)
    };
    show().map_err(|e| e.message())
}

#[cfg(test)]
mod tests {
    #[test]
    fn scoped_credentials_keep_legacy_keys_without_cross_plugin_access() {
        let target = |plugin, key| super::credential(plugin,key).map(|v| String::from_utf16_lossy(&v));
        assert_eq!(target("grok","grok/morning").unwrap(), "mypaper/integrations/grok/morning\0");
        assert_eq!(target("other","grok/morning").unwrap(), "mypaper/integrations/plugin/other/grok/morning\0");
        for invalid in ["../other","grok//key","grok/../key","/key","key/"] { assert!(target("grok",invalid).is_err()); }
        for invalid in ["../Grok Bot", "Grok Bot/secret", "Grok Bot\\secret", ""] { assert!(super::desktop_status(invalid).is_err()); }
    }
    #[test]
    fn toasts_escape_text_and_attribute_the_plugin() {
        let xml = super::toast_xml("T3 <Code>", "Build & ship", "done \"now\"");
        assert!(xml.contains("<text>Build &amp; ship</text>") && xml.contains("done &quot;now&quot;"), "{xml}");
        assert!(xml.contains(r#"<text placement="attribution">via T3 &lt;Code&gt;</text>"#), "{xml}");
    }

    #[test]
    fn the_foreground_window_has_a_process() {
        if let Some(window) = super::foreground() {
            assert!(window.id != 0);
        }
    }

    use super::*;

    #[test]
    fn origins_match_scheme_host_and_port_exactly() {
        let perms = ["network:https://api.open-meteo.com".to_owned(), "network:wss://*.example.com".to_owned()];
        assert!(origin_allowed(&perms, "https://api.open-meteo.com/v1/forecast?x=1"));
        assert!(!origin_allowed(&perms, "http://api.open-meteo.com/v1"));
        assert!(!origin_allowed(&perms, "https://api.open-meteo.com:8443/"));
        assert!(!origin_allowed(&perms, "https://api.open-meteo.com.evil.net/"));
        assert!(!origin_allowed(&perms, "https://user@api.open-meteo.com/"));
        assert!(origin_allowed(&perms, "wss://live.example.com/socket"));
        assert!(!origin_allowed(&perms, "wss://example.com/"));
        assert!(!origin_allowed(&[], "https://api.open-meteo.com/"));
    }

    #[test]
    fn processes_need_their_declared_name_and_report_code_and_streams() {
        let perms = ["process:cmd".to_owned()];
        assert!(resolve_exe(&perms, "powershell").is_err(), "undeclared");
        assert!(resolve_exe(&perms, r"C:\Windows\System32\cmd.exe").is_ok());
        assert!(resolve_exe(&perms, r"C:\Windows\System32\cmd.txt").is_err(), "not runnable");
        assert!(resolve_exe(&perms, r"..\cmd").is_err() && resolve_exe(&perms, "cmd.exe").is_err(), "bare names only");
        let exe = resolve_exe(&perms, "cmd").unwrap();
        let ran = run_process(&exe, &["/c".into(), "echo out& echo err 1>&2& exit 3".into()], std::time::Duration::from_secs(10)).unwrap();
        assert_eq!((ran.code, String::from_utf8_lossy(&ran.stdout).trim()), (3, "out"));
        assert_eq!(String::from_utf8_lossy(&ran.stderr).trim(), "err");
        assert!(run_process(&exe, &["/c".into(), "ping -n 5 127.0.0.1 >nul".into()], std::time::Duration::from_millis(300)).is_err(), "timed out");
    }

    #[test]
    fn declared_files_are_profile_paths_only() {
        assert!(file_path_ok("~/.t3/userdata/server-runtime.json"));
        for bad in ["/etc/x", "~/", "~/../x", "~/a/./b", r"~/a\b", "~/C:/x", "~/a//b", "C:/x"] {
            assert!(!file_path_ok(bad), "{bad}");
        }
        assert_eq!(read_file("~/no-such-mypaper-file").unwrap(), None);
    }

    #[test]
    fn storage_keys_are_safe_file_names() {
        let dir = std::env::temp_dir().join(format!("mypaper-kv-{}", std::process::id()));
        storage_set(&dir, "../../evil", b"v").unwrap();
        assert_eq!(storage_get(&dir, "../../evil").unwrap(), Some(b"v".to_vec()));
        assert!(dir.join("kv").read_dir().unwrap().count() == 1);
        storage_remove(&dir, "../../evil").unwrap();
        assert_eq!(storage_get(&dir, "../../evil").unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
