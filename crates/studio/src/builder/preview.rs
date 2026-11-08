//! A dedicated headless engine child: it never shares the desktop engine's active theme.
use anyhow::{Context, Result};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::SystemTime,
};

#[derive(Debug, Default)]
pub struct Preview {
    child: Option<Child>,
    attempted: Option<(PathBuf, Option<SystemTime>)>,
    pub error: Option<String>,
}
impl Preview {
    /// Restart after an engine update; an older engine is tried only once per executable.
    pub fn tick(&mut self, dir: &Path) {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.error = Some(format!(
                        "Engine preview stopped ({status}). Update Glaze to preview this draft."
                    ));
                    self.child = None;
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.child = None;
                }
                Ok(None) => return,
            }
        }
        let Some(exe) = engine_exe() else {
            self.error = Some("Start the latest Glaze engine to preview this draft.".into());
            return;
        };
        let stamp = std::fs::metadata(&exe).ok().and_then(|m| m.modified().ok());
        if self.attempted.as_ref() == Some(&(exe.clone(), stamp)) {
            return;
        }
        self.attempted = Some((exe.clone(), stamp));
        match spawn(&exe, dir) {
            Ok(child) => {
                self.child = Some(child);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Engine preview: {error:#}")),
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn engine_exe() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GLAZE_PREVIEW_ENGINE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(path);
    }
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().skip(1) {
            paths.push(parent.join("glaze-wallpaper.exe"));
        }
    }
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/release/glaze-wallpaper.exe"),
    );
    paths.push(PathBuf::from(
        r"C:\glaze-targets\main\release\glaze-wallpaper.exe",
    ));
    paths.into_iter().find(|p| p.is_file())
}

fn spawn(exe: &Path, dir: &Path) -> Result<Child> {
    let log = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .context("LOCALAPPDATA is unavailable")?
        .join("glaze/preview");
    std::fs::create_dir_all(&log)?;
    let name = dir.file_name().context("draft has no folder name")?;
    let stderr = std::fs::File::create(log.join(name).with_extension("log"))?;
    Command::new(exe)
        .arg("--preview")
        .arg(dir)
        .arg("--out")
        .arg(dir.join("preview.jpg"))
        .args(["--size", "1280x720", "--fps", "10"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: even a console dev build stays hidden.
        .spawn()
        .context("starting the headless engine")
}
