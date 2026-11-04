//! `user.info`: the signed-in user's display name and account name.
//!
//! The display name can need a domain controller (`GetUserNameExW`), so it is read on a worker
//! thread; the answer is cached for the life of the host.

use std::sync::OnceLock;

use serde::Serialize;
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
use windows::Win32::System::WindowsProgramming::GetUserNameW;
use windows::core::PWSTR;

use crate::bridge::Target;
use crate::wake::{Msg, Waker};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserInfo {
    /// The display name ("Alex K."), else the account name.
    pub name: String,
    /// The account name ("you").
    pub account: String,
}

static CACHE: OnceLock<UserInfo> = OnceLock::new();

/// The answer, if it has been read before.
pub fn cached() -> Option<&'static UserInfo> {
    CACHE.get()
}

/// Reads the user on a worker thread and answers `target` with it.
pub fn read_for(target: Target, waker: Waker) {
    let spawned = std::thread::Builder::new()
        .name("user-info".into())
        .spawn(move || {
            let info = CACHE.get_or_init(read);
            waker.send(Msg::Answered(target, Ok(serde_json::json!(info))));
        });
    if let Err(e) = spawned {
        log::warn!("user.info: {e}");
    }
}

/// The display name, or the account name when Windows has none.
fn pick_name(display: Option<String>, account: &str) -> String {
    display
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| account.to_owned())
}

fn read() -> UserInfo {
    let account = read_string(|buffer, size| {
        // SAFETY: `buffer` is `size` wide characters long, as the call requires.
        unsafe { GetUserNameW(Some(PWSTR(buffer)), size) }.is_ok()
    })
    .unwrap_or_default();
    let display = read_string(|buffer, size| {
        // SAFETY: as above.
        unsafe { GetUserNameExW(NameDisplay, Some(PWSTR(buffer)), size) }
    });
    UserInfo {
        name: pick_name(display, &account),
        account,
    }
}

/// Calls a "fill this buffer, `size` in characters" API and returns the text.
fn read_string(fill: impl Fn(*mut u16, *mut u32) -> bool) -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut size = buffer.len() as u32;
    fill(buffer.as_mut_ptr(), &mut size).then(|| {
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_name_falls_back_to_the_account() {
        assert_eq!(pick_name(Some("Alex K.".into()), "you"), "Alex K.");
        assert_eq!(pick_name(Some("  ".into()), "you"), "you");
        assert_eq!(pick_name(None, "you"), "you");
    }

    #[test]
    fn this_machines_account_is_readable() {
        let info = read();
        assert!(!info.account.is_empty());
        assert!(!info.name.is_empty());
    }
}
