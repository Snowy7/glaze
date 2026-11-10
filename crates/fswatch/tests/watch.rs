use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use glaze_fswatch::{Event, Watch};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "glaze-fswatch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_for(rx: &mpsc::Receiver<Event>, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert_ne!(event, Event::Rescan, "small fixture should not overflow");
        if matches!(event, Event::Paths(ref paths) if paths.iter().any(|p| p == path)) {
            return;
        }
    }
}

#[test]
fn create_modify_atomic_replace_rename_delete_recursive_and_stop() {
    let dir = Dir::new();
    std::fs::create_dir(dir.0.join("sub")).unwrap();
    let file = dir.0.join("config.json");
    let (tx, rx) = mpsc::channel();
    let watch = Watch::directory(&dir.0, true, move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    // No sleep before this write: successful construction promises the first read is armed.
    std::fs::write(&file, "{}").unwrap();
    wait_for(&rx, &file);
    std::fs::write(&file, "{ }").unwrap();
    wait_for(&rx, &file);
    let temp = dir.0.join("config.tmp");
    std::fs::write(&temp, "{  }").unwrap();
    std::fs::rename(&temp, &file).unwrap();
    wait_for(&rx, &file);
    let renamed = dir.0.join("renamed.json");
    std::fs::rename(&file, &renamed).unwrap();
    let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(event, Event::Paths(ref paths) if paths.contains(&file) && paths.contains(&renamed)),
        "both rename names: {event:?}"
    );
    std::fs::remove_file(&renamed).unwrap();
    wait_for(&rx, &renamed);
    let nested = dir.0.join("sub").join("child");
    std::fs::write(&nested, "x").unwrap();
    wait_for(&rx, &nested);
    // A write can produce several batches. A fresh watch isolates the read-only assertion
    // from notifications still queued for the preceding writes.
    drop(watch);
    let (tx, rx) = mpsc::channel();
    let watch = Watch::directory(&dir.0, true, move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&nested).unwrap(), "x");
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "reading doesn't report access events"
    );
    let started = Instant::now();
    drop(watch);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "drop cancels the idle read"
    );
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "worker joined, callback released"
    );
    std::fs::write(nested, "after drop").unwrap();
}

#[test]
fn named_files_ignore_noise_and_coalesce_atomic_saves() {
    let dir = Dir::new();
    let (tx, rx) = mpsc::channel();
    let watch = Watch::files(&dir.0, &["config.json"], move || {
        let _ = tx.send(());
    })
    .unwrap();
    std::fs::write(dir.0.join("unrelated"), "x").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    for _ in 0..10 {
        std::fs::write(dir.0.join("temp"), "{}").unwrap();
        std::fs::rename(dir.0.join("temp"), dir.0.join("CONFIG.JSON")).unwrap();
    }
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "one callback for a burst of saves"
    );
    // Also cancel while a debounce or native completion is pending.
    std::fs::write(dir.0.join("CONFIG.JSON"), "pending").unwrap();
    let started = Instant::now();
    drop(watch);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(Watch::directory(&dir.0.join("missing"), false, |_| {}).is_err());
}

#[test]
fn deleted_directory_recovers_and_drop_cancels_waiting_for_a_parent_change() {
    let parent = Dir::new();
    let dir = parent.0.join("glaze");
    std::fs::create_dir(&dir).unwrap();
    let (tx, rx) = mpsc::channel();
    let watch = Watch::directory(&dir, false, move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    std::fs::remove_dir(&dir).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Rescan
    );
    // Leave it absent long enough to enter the parent watch, then recreate and publish before
    // recovery. The recovery rescan must cover that publication, and later writes must arrive.
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("game-activity.json"), "game on").unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Rescan
    );
    while rx.try_recv().is_ok() {}
    let after = dir.join("after-recovery");
    std::fs::write(&after, "game off").unwrap();
    wait_for(&rx, &after);

    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Rescan
    );
    let started = Instant::now();
    drop(watch);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "drop cancels the parent watch too"
    );
}

#[test]
fn named_files_recover_when_their_parent_tree_is_recreated() {
    let parent = Dir::new();
    let tree = parent.0.join("settings");
    let dir = tree.join("glaze");
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, rx) = mpsc::channel();
    let watch = Watch::files(&dir, &["game-activity.json"], move || {
        let _ = tx.send(());
    })
    .unwrap();
    std::fs::remove_dir_all(&tree).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("game-activity.json"), "game on").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // Drain recovery/publication and then prove that the named-file filter still works.
    while rx.recv_timeout(Duration::from_millis(200)).is_ok() {}
    std::fs::write(dir.join("noise"), "x").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    std::fs::write(dir.join("game-activity.json"), "game off").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(watch);
}
