//! `mypaper-styler`: the app styler's helper (docs/windows-styling.md, "App styler"). Core runs
//! it only while `experimental.app_styler` is on; when it exits (flag off, quit, panic hotkey,
//! killed), every styled app puts its original look back.
//!
//! ```text
//! mypaper-styler [--parent PID]        follow the config and style Explorer and Settings;
//!                                      with --parent (core passes its pid), exit with it
//! mypaper-styler --preview PRESET      the same with that preset, whatever the flag and theme
//! mypaper-styler --dump PID [--winui] [--out FILE] [--props SELECTOR]
//!                                      write that app's live XAML tree (for writing rules),
//!                                      and the property values of what SELECTOR picks
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
        (None, None) => {
            let parent = value("--parent")
                .map(|p| p.parse().context("--parent expects a process id"))
                .transpose()?;
            return helper::run::run(value("--preview"), parent);
        }
        (Some(pid), _) => (pid, false),
        (None, Some(pid)) => (pid, true),
    };
    let pid: u32 = pid.parse().context("expected a process id")?;
    let framework = if flag("--winui") {
        Framework::WinUi
    } else {
        Framework::System
    };
    // Stage the DLL first, then pin the process: from the check to the attach, its pid can't
    // name another process.
    let tap = helper::tap_path()?;
    let process = helper::check_allowed(pid)?;
    let app = process.app;
    let dir = helper::data_dir()?;
    // A dump styles nothing: its rules file never exists.
    let rules = if dump {
        let props = dir.join("props.txt");
        match value("--props") {
            Some(selector) => std::fs::write(&props, selector)?,
            None => {
                let _ = std::fs::remove_file(&props);
            }
        }
        "dump-no-rules.json".to_owned()
    } else {
        let rules = value("--rules").context("--attach needs --rules")?;
        std::path::absolute(rules)?.display().to_string()
    };
    let data = Attach {
        helper: std::process::id(),
        framework,
        dir: dir.clone(),
        rules,
        dump,
    };
    anyhow::ensure!(process.running(), "{app} ({pid}) exited");
    helper::attach::attach(pid, framework, &tap, &data)?;
    let seconds = value("--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let wait = if dump { 5 } else { seconds };
    log::info!("attached to {app} ({pid}); detaching in {wait} s");
    std::thread::sleep(Duration::from_secs(wait));
    if dump {
        let tree = dir.join(format!("tree-{pid}.txt"));
        let out = match value("--out") {
            Some(out) => {
                std::fs::copy(&tree, &out)?;
                PathBuf::from(out)
            }
            None => tree,
        };
        println!("{}", out.display());
    }
    Ok(())
}
