//! "Start with Windows". The work (a Task Scheduler logon task, with the `Run` key as fallback)
//! lives in `mypaper-core --autostart on|off|status`; Studio only runs it and relays its JSON line.
//! Core is the source of truth, so changes made elsewhere show up.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Autostart {
    pub enabled: bool,
    /// How it is wired up: `"task"` (Task Scheduler logon task) or `"run-key"`.
    pub method: Option<String>,
    /// The command Windows will run at sign-in.
    pub command: Option<String>,
}

/// Current state; "off" when core is missing or cannot say.
pub fn get() -> Autostart {
    run("status").unwrap_or_default()
}

/// Enables or disables autostart and returns the resulting state.
pub fn set(enabled: bool) -> Result<Autostart> {
    run(if enabled { "on" } else { "off" })
}

/// Runs `mypaper-core --autostart <command>` and parses its one-line JSON answer.
fn run(command: &str) -> Result<Autostart> {
    let core = crate::engine::find_core()
        .context("mypaper-core.exe not found; build the workspace first")?;
    let output = Command::new(&core)
        .args(["--autostart", command])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", core.display()))?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    serde_json::from_slice(&output.stdout).context("reading mypaper-core's autostart answer")
}
