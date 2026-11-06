//! `mypaper-styler`: the app styler's helper (docs/windows-styling.md, "App styler"). Core runs
//! it only while `experimental.app_styler` is on; when it exits (flag off, quit, panic hotkey,
//! killed), every styled app puts its original look back.
//!
//! ```text
//! mypaper-styler                       follow the config and style Explorer and Settings
//! mypaper-styler --preview PRESET      the same with that preset, whatever the flag and theme
//! mypaper-styler --dump PID [--winui] [--out FILE]
//!                                      write that app's live XAML tree (for writing rules)
//! mypaper-styler --attach PID [--winui] --rules FILE [--seconds N]
//!                                      style one process with one rules file for N seconds
//! ```

// No console window in release builds; logs go to mypaper.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod helper;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_app_styler::{Attach, Framework};

fn main() -> std::process::ExitCode {
    mypaper_desktop::init_logging();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        let at = args.iter().position(|a| a == name)?;
        args.get(at + 1).cloned()
    };
    let (pid, dump) = match (value("--attach"), value("--dump")) {
        (None, None) => return helper::run::run(value("--preview")),
        (Some(pid), _) => (pid, None),
        (None, Some(pid)) => {
            let out = value("--out").map_or_else(
                || helper::data_dir().map(|d| d.join(format!("tree-{pid}.txt"))),
                |p| Ok(PathBuf::from(p)),
            )?;
            (pid, Some(out))
        }
    };
    let pid: u32 = pid.parse().context("expected a process id")?;
    let framework = if flag("--winui") {
        Framework::WinUi
    } else {
        Framework::System
    };
    let app = helper::check_allowed(pid)?;
    // A dump styles nothing: its rules file never exists.
    let rules = match &dump {
        Some(_) => helper::data_dir()?.join("dump-no-rules.json"),
        None => value("--rules")
            .map(PathBuf::from)
            .context("--attach needs --rules")?,
    };
    let data = Attach {
        helper: std::process::id(),
        framework,
        rules,
        log: Some(helper::log_path()?),
        dump: dump.clone(),
    };
    helper::attach::attach(pid, framework, &helper::tap_path()?, &data)?;
    let seconds = value("--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let wait = if dump.is_some() { 5 } else { seconds };
    log::info!("attached to {app} ({pid}); detaching in {wait} s");
    std::thread::sleep(Duration::from_secs(wait));
    if let Some(out) = dump {
        println!("{}", out.display());
    }
    Ok(())
}
