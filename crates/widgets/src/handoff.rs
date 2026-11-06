//! Hands a URL launch straight to an app that is already running, the way its own second
//! instance would, for Chromium-based apps (Electron: T3 Code, VS Code, Discord...). Starting
//! the second process only for it to pass the URL on and exit costs ~0.4 s; this costs ~1 ms.
//!
//! Chromium's ProcessSingleton on Windows (Chromium 152, Electron 44):
//! - The running browser process owns a message-only window of class `Chrome_MessageWindow`
//!   titled with its user-data dir (`base/win/message_window.cc`, `CreateNamed` in
//!   `chrome/browser/process_singleton_win.cc`).
//! - A second instance sends it `WM_COPYDATA` holding `START\0<cwd>\0<command line>\0`, with
//!   `cbData` counting the string's terminator too, through `SendMessageTimeout(SMTO_ABORTIFHUNG)`
//!   after `AllowSetForegroundWindow(owner)` (`chrome/browser/win/chrome_process_finder.cc`).
//! - Electron appends `<base64 data>\0` only for `requestSingleInstanceLock(data)` with data
//!   (`patches/chromium/feat_add_data_parameter_to_processsingleton.patch`) and emits
//!   `second-instance` with the parsed argv; a nonzero reply means the instance took it (zero:
//!   it is quitting).

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE, InternalGetWindowText, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA,
};
use windows::core::{PCWSTR, PWSTR, w};

/// How long a running instance gets to take the URL before the launch falls back to starting the
/// app (Chromium's own second instance waits 20 s, then offers to kill a hung one).
const TIMEOUT_MS: u32 = 200;

/// Delivers `target` to the one running instance of `exe` (the scheme's registered handler, whose
/// `open` command is `command`, like `"C:\…\T3 Code.exe" "%1"`). Returns the instance's process id,
/// or why it couldn't, and then the caller starts the app as before.
pub(crate) fn hand_off(exe: &str, command: &str, target: &str) -> Result<u32, &'static str> {
    let line = command_line(command, target).ok_or("the target doesn't fit the handler's command")?;
    let (window, pid) = singleton_window(exe)?;
    // The cwd the started app would have inherited from us (ShellExecute without a directory).
    let cwd = std::env::current_dir().map(|dir| dir.display().to_string()).unwrap_or_default();
    let message = start_message(&cwd, &line);
    let data = COPYDATASTRUCT { dwData: 0, cbData: (message.len() * size_of::<u16>()) as u32, lpData: message.as_ptr() as *mut _ };
    let mut reply = 0usize;
    // SAFETY: `data` and `message` outlive the synchronous send; the system copies them to the
    // receiver. A stale window just fails the send.
    let sent = unsafe {
        // Lets the instance bring itself forward for the link, like Chromium's second instance.
        let _ = AllowSetForegroundWindow(pid);
        SendMessageTimeoutW(window, WM_COPYDATA, WPARAM(0), LPARAM(&raw const data as isize), SMTO_ABORTIFHUNG, TIMEOUT_MS, Some(&mut reply))
    };
    match (sent.0, reply) {
        (0, _) => Err("the running instance didn't answer in time"),
        (_, 0) => Err("the running instance refused it (quitting)"),
        _ => Ok(pid),
    }
}

/// The `WM_COPYDATA` payload of a Chromium second instance: `START\0<cwd>\0<command line>\0` and
/// the string's terminator, which Chromium counts in `cbData` (no Electron additional data).
pub(crate) fn start_message(cwd: &str, command_line: &str) -> Vec<u16> {
    let mut message: Vec<u16> = "START\0".encode_utf16().collect();
    for field in [cwd, command_line] {
        message.extend(field.encode_utf16());
        message.push(0);
    }
    message.push(0);
    message
}

/// The command line Windows would start the handler with: the registered `open` command with
/// the target in place of its one `%1`/`%L`. None for any other placeholder (`%*`, `%2`...) or a
/// target a quote or control character could split.
pub(crate) fn command_line(command: &str, target: &str) -> Option<String> {
    if target.contains('"') || target.chars().any(char::is_control) || command.matches('%').count() != 1 {
        return None;
    }
    let placeholder = ["%1", "%L", "%l"].into_iter().find(|p| command.contains(p))?;
    Some(command.replacen(placeholder, target, 1))
}

/// Chromium's singleton window owned by a running process whose image is `exe`, and that
/// process. Several (instances on other user-data dirs) are refused: a second instance would
/// reach only the one on its default dir, which we can't tell apart.
fn singleton_window(exe: &str) -> Result<(HWND, u32), &'static str> {
    let mut found = None;
    let mut after = None;
    // SAFETY: read-only window queries; handles may go stale, then the calls just fail.
    while let Ok(window) = unsafe { FindWindowExW(Some(HWND_MESSAGE), after, w!("Chrome_MessageWindow"), PCWSTR::null()) } {
        after = Some(window);
        let mut title = [0u16; 8];
        // Only ProcessSingleton names its window (with the user-data dir); the others are untitled.
        // SAFETY: as above, into a buffer of its length.
        if unsafe { InternalGetWindowText(window, &mut title) } == 0 {
            continue;
        }
        let mut pid = 0;
        // SAFETY: as above.
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid != 0 && image_path(pid).is_some_and(|image| image.eq_ignore_ascii_case(exe)) {
            if found.is_some() {
                return Err("several running instances");
            }
            found = Some((window, pid));
        }
    }
    found.ok_or("no running instance")
}

/// The full path of the image process `pid` runs.
pub(crate) fn image_path(pid: u32) -> Option<String> {
    // SAFETY: a query-only process handle, closed before returning; a buffer of the length passed.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&path[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits a payload the way Electron's patched `ParseCommandLine` does: the command, the cwd,
    /// the command line, and the base64 additional data between the third and fourth NUL.
    fn parse(message: &[u16]) -> (String, String, String, String) {
        let text = String::from_utf16(message).unwrap();
        let fields: Vec<_> = text.split('\0').collect();
        (fields[0].into(), fields[1].into(), fields[2].into(), fields[3].into())
    }

    #[test]
    fn the_start_message_is_what_a_chromium_second_instance_sends() {
        let line = r#""C:\Programs\t3code\T3 Code (Nightly).exe" "t3code://threads/env/thread""#;
        let message = start_message(r"C:\Windows\system32", line);
        assert_eq!(parse(&message), ("START".into(), r"C:\Windows\system32".into(), line.into(), String::new()), "no additional data");
        assert!(message.ends_with(&[0, 0]), "the field's NUL, then the terminator cbData counts");
        assert_eq!(message.len(), "START".len() + 1 + r"C:\Windows\system32".len() + 1 + line.encode_utf16().count() + 2);
        assert!(start_message("", "").len() >= 7, "Chromium's shortest valid message is START\\0\\0");
    }

    #[test]
    fn the_command_line_is_the_registered_command_with_the_target() {
        let command = r#""C:\Programs\t3code\T3 Code (Nightly).exe" "%1""#;
        let url = "t3code://threads/6363b367/a%20b";
        assert_eq!(command_line(command, url).as_deref(), Some(r#""C:\Programs\t3code\T3 Code (Nightly).exe" "t3code://threads/6363b367/a%20b""#));
        assert_eq!(command_line(r#""C:\Code.exe" --open-url -- "%L""#, "vscode://file/x").as_deref(), Some(r#""C:\Code.exe" --open-url -- "vscode://file/x""#));
        assert_eq!(command_line(command, "t3code://x\" --inject"), None, "a quote would split the argument");
        assert_eq!(command_line(r#""C:\app.exe" "%1" %*"#, "app://x"), None, "other placeholders go through ShellExecute");
        assert_eq!(command_line(r#""C:\app.exe""#, "app://x"), None, "a command that takes no target");
    }
}
