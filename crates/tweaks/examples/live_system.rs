//! Opt-in REAL per-user setting round trips. Run under the desktop lock with a
//! scratch LOCALAPPDATA, so the recovery journal cannot overlap core's journal.
//! `--hold SECONDS FILE` holds a system-config JSON file for native Settings inspection.
//! On failure, rerun with the same LOCALAPPDATA: construction recovers its journal.

use anyhow::{Result, ensure};
use glaze_config::{PowerMode, SystemConfig};
use glaze_tweaks::system::{SystemStore, SystemTweaks, writes};
use glaze_tweaks::undo::Store;
use std::time::Duration;
use windows::Win32::UI::Shell::{
    SHELLSTATEA, SHGetSetSettings, SSF_SEPPROCESS, SSF_SHOWALLOBJECTS, SSF_SHOWEXTENSIONS,
};

fn shell_flags() -> (i32, i32) {
    let mut state = SHELLSTATEA::default();
    // SAFETY: read only; the supplied shell-state buffer is valid for the call.
    unsafe {
        SHGetSetSettings(
            Some(&mut state),
            SSF_SHOWALLOBJECTS | SSF_SHOWEXTENSIONS | SSF_SEPPROCESS,
            false,
        );
    }
    (state._bitfield1, state._bitfield2)
}

fn main() -> Result<()> {
    let lock =
        std::path::Path::new(r"C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock");
    ensure!(
        lock.exists(),
        "caller must hold the desktop lock before running live_system"
    );
    let config: SystemConfig = match std::env::args().nth(3) {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
        None => SystemConfig::default(),
    };
    let hold = std::env::args().nth(1).as_deref() == Some("--hold");
    let seconds: u64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1".into())
        .parse()?;
    let all = SystemConfig {
        game_mode: Some(true),
        show_extensions: Some(true),
        show_hidden: Some(true),
        separate_process: Some(true),
        power_mode: Some(PowerMode::Performance),
        ..Default::default()
    };
    let plan = writes(&all);
    // Recover any interrupted previous probe before capturing the comparison baseline.
    let mut tweaks = SystemTweaks::new()?;
    let store = SystemStore;
    let before = plan
        .iter()
        .map(|write| store.read(write.key, write.name))
        .collect::<Result<Vec<_>>>()?;
    println!("BEFORE {}", serde_json::to_string(&before)?);
    let cases = if hold {
        vec![config]
    } else {
        let mut cases = Vec::new();
        for on in [true, false] {
            cases.extend([
                SystemConfig {
                    game_mode: Some(on),
                    ..Default::default()
                },
                SystemConfig {
                    show_extensions: Some(on),
                    ..Default::default()
                },
                SystemConfig {
                    show_hidden: Some(on),
                    ..Default::default()
                },
                SystemConfig {
                    separate_process: Some(on),
                    ..Default::default()
                },
            ]);
        }
        cases.extend(
            [
                PowerMode::Efficiency,
                PowerMode::Balanced,
                PowerMode::Performance,
            ]
            .map(|mode| SystemConfig {
                power_mode: Some(mode),
                ..Default::default()
            }),
        );
        cases
    };
    for case in cases {
        tweaks.apply(&case)?;
        for write in writes(&case) {
            ensure!(
                store.read(write.key, write.name)?.as_ref() == Some(&write.value),
                "readback differs: {}\\{}",
                write.key,
                write.name
            );
            if !write.key.starts_with('@') {
                let _ = std::process::Command::new("reg.exe")
                    .args(["query", &format!("HKCU\\{}", write.key), "/v", write.name])
                    .status()?;
            }
        }
        let (visibility, process) = shell_flags();
        if let Some(on) = case.show_extensions {
            ensure!(
                (visibility & 2 != 0) == on,
                "shell extension flag did not update"
            );
        }
        if let Some(on) = case.show_hidden {
            ensure!(
                (visibility & 1 != 0) == on,
                "shell hidden flag did not update"
            );
        }
        if let Some(on) = case.separate_process {
            ensure!(
                (process & 1 != 0) == on,
                "shell separate-process flag did not update"
            );
        }
        println!(
            "APPLIED {} shell_flags={visibility:#x}/{process:#x}",
            serde_json::to_string(&case)?
        );
        std::thread::sleep(Duration::from_secs(seconds));
        tweaks.apply(&SystemConfig::default())?;
        for (write, original) in plan.iter().zip(&before) {
            ensure!(
                &store.read(write.key, write.name)? == original,
                "undo differs: {}\\{}",
                write.key,
                write.name
            );
        }
        println!("RESTORED exact type/bytes/absence (all six values)");
    }
    println!("PASS originals {}", serde_json::to_string(&before)?);
    Ok(())
}
