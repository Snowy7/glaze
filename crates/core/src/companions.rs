//! On-demand apps (Studio and Files) share Quit's lifetime without being restarted by core.
//! Only an exact sibling executable in our Windows logon/session is eligible for shutdown.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM},
    Security::{
        GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenSessionId, TokenStatistics,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GWL_STYLE, GetWindowLongPtrW, GetWindowThreadProcessId, PostMessageW,
        WM_CLOSE, WS_CAPTION,
    },
};
use windows::core::{BOOL, PWSTR};

const APPS: [&str; 2] = ["glaze-studio.exe", "glaze-files.exe"];
const GRACE: Duration = Duration::from_secs(3);

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: each successful open/snapshot transfers this one owned handle to us.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    session: u32,
    logon: (u32, i32),
}

fn identity(process: HANDLE) -> Result<Identity> {
    let mut token = HANDLE::default();
    // SAFETY: query-only token for a live process handle, owned until this function returns.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token)? };
    let token = Handle(token);
    let mut stats = TOKEN_STATISTICS::default();
    let mut session = 0u32;
    let mut returned = 0;
    // SAFETY: fixed-size token information is written into correctly aligned typed locals.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenStatistics,
            Some((&mut stats as *mut TOKEN_STATISTICS).cast()),
            size_of::<TOKEN_STATISTICS>() as u32,
            &mut returned,
        )?;
        GetTokenInformation(
            token.0,
            TokenSessionId,
            Some((&mut session as *mut u32).cast()),
            size_of::<u32>() as u32,
            &mut returned,
        )?;
    }
    Ok(Identity {
        session,
        logon: (
            stats.AuthenticationId.LowPart,
            stats.AuthenticationId.HighPart,
        ),
    })
}

fn is_companion(core: &Path, image: &Path) -> bool {
    let Some(name) = image.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    companion_name(name)
        && core
            .with_file_name(name)
            .as_os_str()
            .eq_ignore_ascii_case(image.as_os_str())
}

fn companion_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    APPS.iter().any(|app| {
        name == *app
            || name
                .strip_prefix(&format!("{app}.old-"))
                .is_some_and(|stamp| {
                    // deploy-local renames an open executable to this timestamped sibling before swapping it.
                    stamp.len() == 14 && stamp.bytes().all(|c| c.is_ascii_digit())
                })
    })
}

/// Request ordinary window close first, then bound total shutdown time across all companions.
pub fn close(core: &Path) {
    if let Err(error) = close_with_grace(core, GRACE) {
        log::warn!("closing companion apps: {error:#}");
    }
}

fn close_with_grace(core: &Path, grace: Duration) -> Result<()> {
    // SAFETY: pseudo-handle is only queried, never owned or closed.
    let ours = identity(unsafe { GetCurrentProcess() })?;
    // SAFETY: read-only process snapshot; owned and closed below.
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut apps = Vec::new();
    // SAFETY: the snapshot and initialized record stay live throughout enumeration.
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while next.is_ok() {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if companion_name(&name) {
            // Open once and retain the handle: PID recycling cannot redirect the later kill.
            // SAFETY: access is limited to query, wait and shutdown of the validated process.
            if let Ok(handle) = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                    false,
                    entry.th32ProcessID,
                )
            } {
                let process = Handle(handle);
                let mut image = [0u16; 32768];
                let mut length = image.len() as u32;
                // SAFETY: the buffer is writable for its stated length; the handle is owned.
                let queried = unsafe {
                    QueryFullProcessImageNameW(
                        process.0,
                        PROCESS_NAME_WIN32,
                        PWSTR(image.as_mut_ptr()),
                        &mut length,
                    )
                };
                if queried.is_ok()
                    && is_companion(
                        core,
                        Path::new(&String::from_utf16_lossy(&image[..length as usize])),
                    )
                    && identity(process.0).is_ok_and(|other| other == ours)
                {
                    apps.push((entry.th32ProcessID, process, name));
                }
            }
        }
        // SAFETY: reuse the same live snapshot and initialized output record.
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    for (pid, _, name) in &apps {
        log::info!("asking {name} ({pid}) to close");
        // SAFETY: callback only reads the PID passed by value and posts WM_CLOSE; no focus/input.
        let _ = unsafe { EnumWindows(Some(close_window), LPARAM(*pid as isize)) };
    }
    let deadline = Instant::now() + grace;
    for (pid, process, name) in apps {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        // SAFETY: retained handle refers to the exact validated process, even if its PID is reused.
        if unsafe { WaitForSingleObject(process.0, remaining) } != WAIT_OBJECT_0 {
            log::warn!("{name} ({pid}) did not close within {grace:?}; terminating it");
            // SAFETY: same validated, retained process handle; bounded fallback after WM_CLOSE.
            unsafe {
                TerminateProcess(process.0, 1)?;
                let _ = WaitForSingleObject(process.0, 1_000);
            }
        } else {
            log::info!("{name} ({pid}) closed gracefully");
        }
    }
    Ok(())
}

unsafe extern "system" fn close_window(window: HWND, pid: LPARAM) -> BOOL {
    let mut owner = 0;
    // SAFETY: read-only PID query; enum guarantees a window handle for the callback.
    unsafe { GetWindowThreadProcessId(window, Some(&mut owner)) };
    // Captioned app/dialog windows handle saves; leave hidden COM/helper windows intact.
    // SAFETY: read-only style query, returning zero for a vanished window.
    let app_window = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32 & WS_CAPTION.0 != 0;
    if owner == pid.0 as u32 && app_window {
        // SAFETY: asynchronous close only for this previously validated process's windows.
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
    BOOL(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, ChildStdout, Command, Stdio};

    struct Fixture(Child, BufReader<ChildStdout>);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn fixture(exe: &Path, stubborn: bool) -> Fixture {
        let mut child = Command::new(exe)
            .args([
                "--ignored",
                "--exact",
                "companions::tests::companion_window_fixture",
                "--nocapture",
            ])
            .env(
                "GLAZE_COMPANION_STUBBORN",
                if stubborn { "1" } else { "0" },
            )
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut fixture = Fixture(child, output);
        assert!(
            (&mut fixture.1)
                .lines()
                .any(|line| line.is_ok_and(|line| line.contains("companion-ready")))
        );
        fixture
    }

    #[test]
    fn closes_matching_apps_gracefully_and_bounds_a_hung_app_without_touching_other_installs() {
        let root = std::env::temp_dir().join(format!("glaze-quit-test-{}", std::process::id()));
        let ours = root.join("ours");
        let other = root.join("other");
        std::fs::create_dir_all(&ours).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let executable = std::env::current_exe().unwrap();
        for path in [ours.join(APPS[0]), ours.join(APPS[1]), other.join(APPS[0])] {
            std::fs::copy(&executable, path).unwrap();
        }
        {
            let mut studio = fixture(&ours.join(APPS[0]), false);
            let mut files = fixture(&ours.join(APPS[1]), true);
            let mut unrelated = fixture(&other.join(APPS[0]), false);
            close_with_grace(&ours.join("glaze-core.exe"), Duration::from_millis(300)).unwrap();
            assert!(
                studio.0.try_wait().unwrap().unwrap().success(),
                "WM_CLOSE should allow an ordinary app to save and exit"
            );
            assert!(
                !files.0.try_wait().unwrap().unwrap().success(),
                "hung app must hit the bounded termination fallback"
            );
            assert!(
                unrelated.0.try_wait().unwrap().is_none(),
                "another install must remain running"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Hidden, nonactivating windows only; never sends input or touches a user's window.
    #[test]
    #[ignore = "child fixture launched by the process-level shutdown test"]
    fn companion_window_fixture() {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::core::w;
        let stubborn = std::env::var("GLAZE_COMPANION_STUBBORN").unwrap() == "1";
        // SAFETY: built-in window class, hidden and owned by this test thread.
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Glaze close fixture"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        println!("companion-ready");
        let mut msg = MSG::default();
        loop {
            // SAFETY: the test thread owns this message queue/window; no messages leave it.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if stubborn && msg.message == WM_CLOSE {
                        continue;
                    }
                    DispatchMessageW(&msg);
                }
                if !IsWindow(Some(window)).as_bool() {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn shutdown_is_scoped_to_known_apps_beside_this_core() {
        let core = Path::new(r"C:\glaze\glaze-core.exe");
        assert!(is_companion(
            core,
            Path::new(r"c:\GLAZE\glaze-studio.exe")
        ));
        assert!(is_companion(
            core,
            Path::new(r"C:\glaze\glaze-files.exe")
        ));
        assert!(!is_companion(
            core,
            Path::new(r"C:\other\glaze-studio.exe")
        ));
        assert!(!is_companion(
            core,
            Path::new(r"C:\glaze\glaze-wallpaper.exe")
        ));
        assert!(!is_companion(
            core,
            Path::new(r"C:\glaze\glaze-studio.exe.backup")
        ));
        assert!(is_companion(
            core,
            Path::new(r"C:\glaze\glaze-studio.exe.old-20261108120000")
        ));
        assert!(!is_companion(
            core,
            Path::new(r"C:\other\glaze-studio.exe.old-20261108120000")
        ));
    }
}
