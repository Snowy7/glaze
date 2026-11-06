//! Core resets its environment to the user's default one at startup.
//!
//! Everything mypaper launches (the shell, the engine, plugins, and through them the user's own
//! apps from the dock, launcher, menus and plugin links) inherits core's environment. When core is
//! started from a developer shell, that shell's leftovers leak into those apps. On 2026-11-06 an
//! agent's PowerShell inside T3 Code carried `ELECTRON_RUN_AS_NODE=1`: every Electron app mypaper
//! opened (T3 Code itself for a thread link, VS Code, Discord...) started as plain Node.js, failed
//! to load its URL argument as a script and exited, so a T3 thread link only raised T3's window.
//!
//! So core replaces its environment with the block Windows builds for the user's token
//! (`CreateEnvironmentBlock`, the same variables Explorer gives apps), keeping only mypaper's own
//! diagnostic switches (`MYPAPER_*`, `RUST_LOG`, `RUST_BACKTRACE`).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Replaces the process environment with the user's default one. Call it first thing in `main`,
/// before any thread starts. Failure keeps the inherited environment and logs why.
pub fn reset() {
    match user_default() {
        Ok(fresh) => apply(fresh),
        Err(e) => log::warn!("keeping the inherited environment: {e:#}"),
    }
}

/// Variables that survive the reset: mypaper's own switches, set on purpose for a run.
fn kept(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    key.starts_with("MYPAPER_") || key == "RUST_LOG" || key == "RUST_BACKTRACE"
}

/// `remove_var`/`set_var` panic on an empty key or one containing `=` (Windows' hidden `=C:`
/// per-drive entries); those are left alone.
fn settable(key: &OsString) -> bool {
    key.to_str().is_some_and(|k| !k.is_empty() && !k.contains('=') && !k.contains('\0'))
}

fn apply(fresh: Vec<(OsString, OsString)>) {
    let keep: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(k, _)| k.to_str().is_some_and(kept))
        .collect();
    for (key, _) in std::env::vars_os() {
        if settable(&key) {
            // SAFETY: called at the start of `main`, before core starts any thread.
            unsafe { std::env::remove_var(&key) };
        }
    }
    for (key, value) in fresh.into_iter().chain(keep) {
        if settable(&key) {
            // SAFETY: as above.
            unsafe { std::env::set_var(&key, &value) };
        }
    }
}

/// The environment Windows builds for the current user's token, without inheriting ours.
fn user_default() -> Result<Vec<(OsString, OsString)>> {
    let mut token = HANDLE::default();
    // SAFETY: opens our own process token; closed below.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
            &mut token,
        )
    }
    .context("opening the process token")?;
    let mut block: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: `block` receives a Windows-allocated environment block, freed below.
    let created = unsafe { CreateEnvironmentBlock(&mut block, Some(token), false) };
    // SAFETY: the token handle is ours.
    let _ = unsafe { CloseHandle(token) };
    created.context("building the user's environment block")?;
    // SAFETY: the block is a sequence of NUL-terminated UTF-16 strings ending with an empty one.
    let units = unsafe { block_units(block as *const u16) };
    let vars = parse_block(units);
    // SAFETY: the block came from CreateEnvironmentBlock.
    let _ = unsafe { DestroyEnvironmentBlock(block) };
    Ok(vars)
}

/// The UTF-16 units of an environment block, up to and including its final empty string.
///
/// # Safety
/// `block` must point at a valid environment block (strings each ending in NUL, then a NUL).
unsafe fn block_units<'a>(block: *const u16) -> &'a [u16] {
    let mut len = 0;
    // SAFETY: walk until the double NUL that ends the block.
    unsafe {
        while !(*block.add(len) == 0 && (len == 0 || *block.add(len - 1) == 0)) {
            len += 1;
        }
        std::slice::from_raw_parts(block, len + 1)
    }
}

/// Splits an environment block into `(key, value)` pairs. A key may start with `=` (Windows'
/// per-drive current directories), so the separator is the first `=` after the first unit.
fn parse_block(units: &[u16]) -> Vec<(OsString, OsString)> {
    units
        .split(|&u| u == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let eq = entry.iter().skip(1).position(|&u| u == u16::from(b'='))? + 1;
            Some((
                OsString::from_wide(&entry[..eq]),
                OsString::from_wide(&entry[eq + 1..]),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn parses_keys_values_and_drive_entries() {
        let units = wide("=C:=C:\\x\0PATH=a;b\0EMPTY=\0X=1=2\0\0");
        let vars = parse_block(&units);
        let pairs: Vec<(String, String)> = vars
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("=C:".into(), "C:\\x".into()),
                ("PATH".into(), "a;b".into()),
                ("EMPTY".into(), String::new()),
                ("X".into(), "1=2".into()),
            ]
        );
    }

    #[test]
    fn keeps_only_mypaper_switches() {
        assert!(kept("MYPAPER_WAKES"));
        assert!(kept("rust_log"));
        assert!(!kept("ELECTRON_RUN_AS_NODE"));
        assert!(!kept("CARGO_TARGET_DIR"));
        assert!(!settable(&OsString::from("=C:")));
        assert!(settable(&OsString::from("PATH")));
    }

    #[test]
    fn user_default_has_the_basics_and_not_electron() {
        let vars = user_default().unwrap();
        let has = |name: &str| vars.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
        assert!(has("LOCALAPPDATA") && has("APPDATA") && has("PATH"));
        assert!(!has("ELECTRON_RUN_AS_NODE"));
    }
}
