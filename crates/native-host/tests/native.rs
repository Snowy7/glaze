#![cfg(windows)]
use mypaper_native_host::{HostConfig, Isolation, PluginSpec, Supervisor};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn example() -> &'static PathBuf {
    static DLL: OnceLock<PathBuf> = OnceLock::new();
    DLL.get_or_init(|| {
        let folder =
            std::env::temp_dir().join(format!("mypaper-native-fixture-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let dll = folder.join("native_echo.dll");
        let status = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-Cpanic=abort",
                "-Copt-level=2",
                "-Clinker=rust-lld.exe",
            ])
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/native-echo/src/lib.rs"),
            )
            .arg("-o")
            .arg(&dll)
            .status()
            .unwrap();
        assert!(status.success());
        dll
    })
}
fn spec(id: &str, isolation: Isolation) -> PluginSpec {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let folder = std::env::temp_dir().join(format!(
        "mypaper-native-test-{}-{}-{id}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&folder).unwrap();
    fs::copy(example(), folder.join("native_echo.dll")).unwrap();
    PluginSpec {
        id: id.to_owned(),
        folder,
        native: vec!["native_echo.dll".to_owned()],
        permissions: vec!["native-code".to_owned()],
        isolation,
    }
}
fn supervisor(timeout: Duration) -> Supervisor {
    Supervisor::new(
        PathBuf::from(env!("CARGO_BIN_EXE_mypaper-native-host")),
        HostConfig {
            call_timeout: timeout,
            initial_backoff: Duration::from_millis(80),
            max_backoff: Duration::from_millis(320),
            ..HostConfig::default()
        },
    )
}
fn pid(supervisor: &Supervisor, plugin: &str) -> u32 {
    supervisor
        .hosts()
        .unwrap()
        .into_iter()
        .find(|host| host.modules.iter().any(|(id, _)| id == plugin))
        .unwrap()
        .pid
}
fn restarted(supervisor: &Supervisor, plugin: &str, old: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(event) = supervisor.recv_event_timeout(Duration::from_millis(100))
            && event.plugin == plugin
            && event.name == "native-module-restarted"
        {
            assert_ne!(pid(supervisor, plugin), old);
            assert_eq!(
                supervisor
                    .call(plugin, "native_echo.dll", "echo", b"alive")
                    .unwrap(),
                b"alive"
            );
            return;
        }
    }
    panic!("no restarted event for {plugin}");
}

#[test]
fn round_trips_background_events_shared_lifecycle_and_permission() {
    let supervisor = supervisor(Duration::from_secs(2));
    assert!(supervisor.hosts().unwrap().is_empty());
    let mut denied = spec("denied", Isolation::Shared);
    denied.permissions.clear();
    assert_eq!(
        supervisor.activate(denied).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    supervisor.activate(spec("one", Isolation::Shared)).unwrap();
    supervisor.activate(spec("two", Isolation::Shared)).unwrap();
    let hosts = supervisor.hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].modules.len(), 2);
    assert_ne!(hosts[0].pid, std::process::id());
    let bytes: Vec<_> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    assert_eq!(
        supervisor
            .call("one", "native_echo.dll", "echo", &bytes)
            .unwrap(),
        bytes
    );
    assert_eq!(
        supervisor
            .call("one", "native_echo.dll", "echo", b"")
            .unwrap(),
        b""
    );
    let before = pid(&supervisor, "one");
    assert!(
        supervisor
            .call("one", "native_echo.dll", "unknown", b"")
            .is_err()
    );
    assert_eq!(
        pid(&supervisor, "one"),
        before,
        "module return errors must preserve process"
    );
    assert_eq!(
        supervisor
            .call("one", "undeclared.dll", "echo", b"")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    supervisor
        .call(
            "two",
            "native_echo.dll",
            "background",
            b"from another thread",
        )
        .unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        let event = supervisor
            .recv_event_timeout(end.saturating_duration_since(Instant::now()))
            .expect("background event");
        if event.name == "background" {
            assert_eq!(event.plugin, "two");
            assert_eq!(event.bytes, b"from another thread");
            break;
        }
    }
    supervisor.deactivate("one").unwrap();
    assert_eq!(supervisor.hosts().unwrap().len(), 1);
    supervisor.deactivate("two").unwrap();
    assert!(supervisor.hosts().unwrap().is_empty());
}

#[test]
fn crashes_restart_native_process_and_automatically_isolate_repeat_offender() {
    let supervisor = supervisor(Duration::from_secs(2));
    supervisor.activate(spec("bad", Isolation::Shared)).unwrap();
    supervisor
        .activate(spec("peer", Isolation::Shared))
        .unwrap();
    supervisor
        .activate(spec("separate", Isolation::Plugin))
        .unwrap();
    while supervisor.try_event().is_some() {}
    let unrelated = pid(&supervisor, "separate");
    let parent = std::process::id();
    for _ in 0..3 {
        let old = pid(&supervisor, "bad");
        assert!(
            supervisor
                .call("bad", "native_echo.dll", "crash", b"")
                .is_err()
        );
        restarted(&supervisor, "bad", old);
        assert_eq!(pid(&supervisor, "separate"), unrelated);
        assert_eq!(std::process::id(), parent);
        while supervisor.try_event().is_some() {}
    }
    assert_eq!(supervisor.hosts().unwrap().len(), 3);
    assert_ne!(pid(&supervisor, "bad"), pid(&supervisor, "peer"));
    let peer = pid(&supervisor, "peer");
    let old = pid(&supervisor, "bad");
    assert!(
        supervisor
            .call("bad", "native_echo.dll", "crash", b"")
            .is_err()
    );
    restarted(&supervisor, "bad", old);
    assert_eq!(
        pid(&supervisor, "peer"),
        peer,
        "auto-isolation protects healthy native modules"
    );
    assert_eq!(pid(&supervisor, "separate"), unrelated);
}

#[test]
fn hung_call_is_killed_and_restarted_within_deadline() {
    let supervisor = supervisor(Duration::from_millis(150));
    supervisor
        .activate(spec("hung", Isolation::Plugin))
        .unwrap();
    let old = pid(&supervisor, "hung");
    let start = Instant::now();
    assert!(
        supervisor
            .call("hung", "native_echo.dll", "hang", b"")
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    restarted(&supervisor, "hung", old);
}

#[test]
fn refuses_paths_outside_plugin_folder_including_symlink_targets() {
    let supervisor = supervisor(Duration::from_secs(2));
    for name in [
        "../outside.dll",
        "..\\outside.dll",
        "C:\\outside.dll",
        "native_echo.dll:stream",
        "nested/echo.dll",
    ] {
        let mut plugin = spec("escape", Isolation::Shared);
        plugin.native = vec![name.to_owned()];
        assert!(supervisor.activate(plugin).is_err(), "accepted {name}");
    }
    let mut plugin = spec("link", Isolation::Shared);
    let link = plugin.folder.join("link.dll");
    if std::os::windows::fs::symlink_file(example(), &link).is_ok() {
        plugin.native = vec!["link.dll".to_owned()];
        assert_eq!(
            supervisor.activate(plugin).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    assert!(supervisor.hosts().unwrap().is_empty());
}
