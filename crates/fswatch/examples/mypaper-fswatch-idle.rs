//! Reproducible idle-wake fixture. Run from a private target directory, then sample the
//! `mypaper-fswatch-idle` process with evidence/measure-wakes.ps1. No desktop/UI is touched.

fn main() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("mypaper-fswatch-idle-{}", std::process::id()));
    std::fs::create_dir(&dir)?;
    let watch = mypaper_fswatch::Watch::directory(&dir, false, |_| {})?;
    println!(
        "pid={}, watching {} for 90 seconds",
        std::process::id(),
        dir.display()
    );
    std::thread::sleep(std::time::Duration::from_secs(90));
    drop(watch);
    std::fs::remove_dir(&dir)?;
    Ok(())
}
