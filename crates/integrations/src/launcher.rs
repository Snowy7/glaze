//! On-demand public Shell Recent/Frequent categories and explicit Grok routine POSTs.
use anyhow::{Result, ensure};
use windows::{
    Win32::{
        Networking::WinHttp::*,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            self, ADLT_FREQUENT, ADLT_RECENT, ApplicationDocumentLists, Common::IObjectArray,
            IApplicationDocumentLists, IShellItem, IShellLinkW, SIGDN_DESKTOPABSOLUTEPARSING,
            SIGDN_NORMALDISPLAY,
        },
    },
    core::{Interface, PCWSTR, w},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recent {
    pub title: String,
    pub target: String,
    pub arguments: String,
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
fn string(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}

/// Runs on a COM worker only when the user's app context panel opens. Pinned/custom/tasks are excluded.
pub fn recent(app_id: &str, frequent: bool) -> Result<Vec<Recent>> {
    let _com = crate::media::Com::new()?;
    let id = wide(app_id);
    // SAFETY: COM is initialized and Shell outputs are owned interfaces/buffers.
    unsafe {
        let lists: IApplicationDocumentLists =
            CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER)?;
        lists.SetAppID(PCWSTR(id.as_ptr()))?;
        let list: IObjectArray =
            lists.GetList(if frequent { ADLT_FREQUENT } else { ADLT_RECENT }, 12)?;
        let mut result = Vec::new();
        for index in 0..list.GetCount()?.min(12) {
            let object: windows::core::IUnknown = list.GetAt(index)?;
            if let Ok(item) = object.cast::<IShellItem>() {
                let title = item.GetDisplayName(SIGDN_NORMALDISPLAY)?;
                let name = title.to_string();
                CoTaskMemFree(Some(title.0.cast()));
                let target = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
                let path = target.to_string();
                CoTaskMemFree(Some(target.0.cast()));
                result.push(Recent {
                    title: name?,
                    target: path?,
                    arguments: String::new(),
                });
            } else if let Ok(link) = object.cast::<IShellLinkW>() {
                let (mut path, mut args, mut description) = ([0; 32768], [0; 4096], [0; 1024]);
                link.GetPath(&mut path, std::ptr::null_mut(), 0)?;
                link.GetArguments(&mut args)?;
                link.GetDescription(&mut description)?;
                let path = string(&path);
                let title = string(&description);
                result.push(Recent {
                    title: if title.is_empty() {
                        path.clone()
                    } else {
                        title
                    },
                    target: path,
                    arguments: string(&args),
                });
            }
        }
        Ok(result)
    }
}

/// ShellExecute preserves Shell link arguments; no command shell is involved.
pub fn open(item: &Recent) -> Result<()> {
    let (target, args) = (wide(&item.target), wide(&item.arguments));
    // SAFETY: both null-terminated strings live through ShellExecuteW.
    let result = unsafe {
        Shell::ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR(args.as_ptr()),
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    ensure!(result.0 as isize > 32, "Windows could not open the item");
    Ok(())
}

pub fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn routine_url(url: &str) -> Result<hyper::Uri> {
    let uri: hyper::Uri = url.parse()?;
    ensure!(
        uri.scheme_str() == Some("https") && uri.host().is_some(),
        "routine webhook must use HTTPS"
    );
    ensure!(
        uri.authority().is_some_and(|a| !a.as_str().contains('@'))
            && uri.query().is_none()
            && !url.contains('#'),
        "webhook credentials belong in Credential Manager, not the URL"
    );
    Ok(uri)
}
struct Internet(*mut std::ffi::c_void);
impl Internet {
    fn new(handle: *mut std::ffi::c_void) -> Result<Self> {
        ensure!(!handle.is_null(), "WinHTTP could not create a request");
        Ok(Self(handle))
    }
}
impl Drop for Internet {
    fn drop(&mut self) {
        // SAFETY: this is our own handle, closed once.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutineRun {
    pub id: String,
    pub title: String,
    pub state: RoutineState,
    pub at_ms: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutineState {
    Sending,
    Accepted,
    Failed,
}
static RUNS: std::sync::Mutex<Vec<RoutineRun>> = std::sync::Mutex::new(Vec::new());
/// Session-local, bounded request receipts. Acceptance does not establish Bot completion.
pub fn routine_runs() -> Vec<RoutineRun> {
    RUNS.lock().unwrap().clone()
}

/// Explicit user click only. Both launcher and widget submissions share these receipts.
pub fn start_routine(routine: &crate::config::Routine) -> Result<()> {
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    {
        let mut runs = RUNS.lock().unwrap();
        ensure!(
            !runs
                .iter()
                .any(|run| run.id == routine.id && run.state == RoutineState::Sending),
            "routine request is already sending"
        );
        runs.insert(
            0,
            RoutineRun {
                id: routine.id.clone(),
                title: routine.title.chars().take(128).collect(),
                state: RoutineState::Sending,
                at_ms,
            },
        );
        runs.truncate(16);
    }
    let result = send_routine(routine);
    if let Some(run) = RUNS
        .lock()
        .unwrap()
        .iter_mut()
        .find(|run| run.id == routine.id && run.at_ms == at_ms)
    {
        run.state = if result.is_ok() {
            RoutineState::Accepted
        } else {
            RoutineState::Failed
        };
    }
    result
}

fn send_routine(routine: &crate::config::Routine) -> Result<()> {
    let uri = routine_url(&routine.url)?;
    let secret = crate::credentials::read(&format!("grok/{}", routine.id))?;
    let host = wide(uri.host().unwrap());
    let path = wide(uri.path());
    let mut headers = wide(&format!(
        "Authorization: Bearer {}\r\nContent-Type: application/json\r\n",
        secret.text()?
    ));
    ensure!(
        !secret.text()?.contains(['\r', '\n']),
        "invalid webhook key"
    );
    let bytes = serde_json::to_vec(&routine.input)?;
    ensure!(bytes.len() <= 64 * 1024, "routine input exceeds 64 KiB");
    // SAFETY: synchronous WinHTTP calls; all strings, request bytes and result buffers remain alive.
    let result = unsafe {
        (|| -> Result<()> {
            let session = Internet::new(WinHttpOpen(
                w!("mypaper/1"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                None,
                None,
                0,
            ))?;
            WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 10000)?;
            let connection = Internet::new(WinHttpConnect(
                session.0,
                PCWSTR(host.as_ptr()),
                uri.port_u16().unwrap_or(443),
                0,
            ))?;
            let request = Internet::new(WinHttpOpenRequest(
                connection.0,
                w!("POST"),
                PCWSTR(path.as_ptr()),
                None,
                None,
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            WinHttpSetOption(
                Some(request.0),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes()),
            )?;
            WinHttpSendRequest(
                request.0,
                Some(&headers[..headers.len() - 1]),
                Some(bytes.as_ptr().cast()),
                bytes.len() as u32,
                bytes.len() as u32,
                0,
            )?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
            let (mut status, mut size) = (0_u32, 4_u32);
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                None,
                Some((&mut status as *mut u32).cast()),
                &mut size,
                std::ptr::null_mut(),
            )?;
            ensure!(
                status == 200,
                "Grok routine was not accepted (HTTP {status})"
            );
            Ok(())
        })()
    };
    headers.fill(0);
    result
}
