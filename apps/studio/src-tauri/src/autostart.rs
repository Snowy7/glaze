//! "Start with Windows": the `mypaper` value in the per-user Run key.
//! The registry is the source of truth, so changes made elsewhere show up.

use std::io::ErrorKind;

use anyhow::{Context, Result};
use serde::Serialize;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "mypaper";

#[derive(Debug, Serialize)]
pub struct Autostart {
    pub enabled: bool,
    /// The command Windows will run at sign-in.
    pub command: Option<String>,
}

pub fn get() -> Autostart {
    let command = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|key| key.get_value::<String, _>(VALUE))
        .ok();
    Autostart { enabled: command.is_some(), command }
}

/// Enabling registers the quoted path of `mypaper-core.exe`; disabling removes the value.
pub fn set(enabled: bool) -> Result<Autostart> {
    let run = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .context("opening the Run registry key")?
        .0;
    if enabled {
        let core = crate::engine::find_core().context("mypaper-core.exe not found; build the workspace first")?;
        run.set_value(VALUE, &format!("\"{}\"", core.display()))
            .context("writing the Run registry value")?;
    } else {
        match run.delete_value(VALUE) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e).context("removing the Run registry value"),
            _ => {}
        }
    }
    Ok(get())
}
