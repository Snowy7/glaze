//! Runtime tests against the real `hello` component (`scripts/build-plugins.ps1` builds it into
//! `plugins/hello/hello.wasm`).
#[path = "../../../plugins/docker/src/runtime_tests.rs"]
mod docker;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use glaze_plugin_api::SizeClass;
use glaze_plugin_api::wire::{self, HelperMsg, HostMsg, Status};
use glaze_plugin_sdk::view::Face;

use crate::host::{Host, Input};
use crate::runtime::{Manifest, Runtime};

fn hello_wasm() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/hello/hello.wasm");
    assert!(path.is_file(), "build the guest first: scripts/build-plugins.ps1 ({})", path.display());
    path
}

/// A temp root with `hello` installed under each id, plus the host's cache and data dirs.
fn root(name: &str, ids: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("glaze-plugins-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/hello/plugin.json")).unwrap();
    for id in ids {
        let dir = root.join("plugins").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), manifest.replace("\"id\": \"hello\"", &format!("\"id\": \"{id}\""))).unwrap();
        std::fs::copy(hello_wasm(), dir.join("hello.wasm")).unwrap();
    }
    root
}

fn host(root: &Path) -> Host<Vec<u8>> {
    let rt = Runtime::new(root.join("cache"), root.join("data")).unwrap();
    let (tx, _rx) = mpsc::channel::<Input>();
    Host::new(rt, Vec::new(), tx)
}

fn open(host: &mut Host<Vec<u8>>, root: &Path, plugin: &str, instance: &str) {
    let dir = root.join("plugins").join(plugin).to_string_lossy().into_owned();
    let message = HostMsg::Open { plugin: plugin.into(), dir, instance: instance.into(), face: "main".into(), size: SizeClass::Medium, settings: "{}".into() };
    assert!(host.on_host(message, Instant::now()));
}

fn act(host: &mut Host<Vec<u8>>, instance: &str, action: &str) {
    host.on_host(HostMsg::Action { instance: instance.into(), action: action.into(), value: None }, Instant::now());
}

/// Every frame written since the last call.
fn frames(host: &mut Host<Vec<u8>>) -> Vec<HelperMsg> {
    let bytes = std::mem::take(host.out());
    let mut input = bytes.as_slice();
    std::iter::from_fn(|| wire::read(&mut input).unwrap()).collect()
}

fn headline(frames: &[HelperMsg], instance: &str) -> Option<String> {
    frames.iter().rev().find_map(|f| match f {
        HelperMsg::View { instance: i, face, .. } if i == instance => {
            let face: Face = serde_json::from_slice(face).unwrap();
            let json = serde_json::to_value(&face.body).unwrap();
            json["children"][0]["text"].as_str().map(str::to_owned)
        }
        _ => None,
    })
}

fn footer(frames: &[HelperMsg], instance: &str) -> Option<String> {
    frames.iter().rev().find_map(|f| match f {
        HelperMsg::View { instance: i, face, .. } if i == instance => serde_json::from_slice::<Face>(face).unwrap().footer,
        _ => None,
    })
}

#[test]
fn face_options_arrive_with_defaults_on_open_and_live_as_options() {
    let root = root("options", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "o1");
    assert_eq!(footer(&frames(&mut host), "o1").as_deref(), Some("Hi · 0 s awake"), "the default, though nothing was saved");
    let settings = r#"{"greeting":"Salaam"}"#.into();
    assert!(host.on_host(HostMsg::Options { instance: "o1".into(), settings }, Instant::now()));
    assert_eq!(footer(&frames(&mut host), "o1").as_deref(), Some("Salaam · 0 s awake"), "delivered live, no reopen");
    // An unknown instance is ignored, and the helper keeps running.
    assert!(host.on_host(HostMsg::Options { instance: "nope".into(), settings: "{}".into() }, Instant::now()));
}

#[test]
fn manifests_that_ask_for_too_much_are_refused() {
    let root = root("manifest", &["hello"]);
    let dir = root.join("plugins/hello");
    assert!(Manifest::load(&dir).is_ok());
    let original = std::fs::read_to_string(dir.join("plugin.json")).unwrap();
    let refused = |edit: &str, want: &str| {
        std::fs::write(dir.join("plugin.json"), edit).unwrap();
        let error = format!("{:#}", Manifest::load(&dir).unwrap_err());
        assert!(error.contains(want), "{error} should mention {want}");
    };
    refused(&original.replace("\"api\": \"0.1\"", "\"api\": \"1.0\""), "newer Glaze");
    refused(&original.replace("\"api\": \"0.1\"", "\"api\": \"0.9\""), "newer Glaze");
    refused(&original.replace("network:https://api.open-meteo.com", "filesystem"), "unknown or malformed permission");
    refused(&original.replace("network:https://api.open-meteo.com", "network:ftp://x"), "unknown or malformed permission");
    refused(&original.replace("\"top-bar\"", "\"explorer\""), "unknown style surface");
    refused(&original.replace("\"id\": \"hello\"", "\"id\": \"other\""), "folder");
    refused(&original.replace("hello.wasm", "..\\\\evil.wasm"), ".wasm file in the plugin folder");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn undeclared_capabilities_are_refused_at_call_time() {
    let root = root("denied", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    act(&mut host, "h1", "denied");
    let text = frames(&mut host)
        .iter()
        .rev()
        .find_map(|f| match f {
            HelperMsg::View { face, .. } => Some(String::from_utf8_lossy(face).into_owned()),
            _ => None,
        })
        .unwrap();
    assert!(text.contains("Denied"), "{text}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_spinning_or_crashing_plugin_is_stopped_without_affecting_others() {
    let root = root("spin", &["hello", "other"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    open(&mut host, &root, "other", "o1");
    frames(&mut host);

    let started = Instant::now();
    act(&mut host, "h1", "spin");
    let spun = started.elapsed();
    assert!(spun < Duration::from_millis(1500), "the hard ceiling (1 s) stopped it after {spun:?}");
    let out = frames(&mut host);
    assert!(out.iter().any(|f| matches!(f, HelperMsg::Status { plugin, status: Status::Restarting(why) } if plugin == "hello" && why.contains("CPU"))), "{out:?}");

    // The other plugin keeps working while hello is down.
    act(&mut host, "o1", "click");
    assert_eq!(headline(&frames(&mut host), "o1").as_deref(), Some("Clicked 1×"));

    // Hello comes back after its backoff, then two more traps stop it for good.
    let later = Instant::now() + Duration::from_secs(2);
    host.tick(later);
    assert!(frames(&mut host).iter().any(|f| matches!(f, HelperMsg::Status { plugin, status: Status::Live } if plugin == "hello")));
    act(&mut host, "h1", "crash");
    host.tick(Instant::now() + Duration::from_secs(10));
    act(&mut host, "h1", "spin");
    let out = frames(&mut host);
    assert!(out.iter().any(|f| matches!(f, HelperMsg::Status { plugin, status: Status::Stopped(_) } if plugin == "hello")), "{out:?}");
    act(&mut host, "o1", "click");
    assert_eq!(headline(&frames(&mut host), "o1").as_deref(), Some("Clicked 2×"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn slow_events_warn_then_throttle_and_only_a_constantly_slow_plugin_stops() {
    let root = root("slow", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    frames(&mut host);
    let (now, slow, budget) = (Instant::now(), Duration::from_millis(120), Duration::from_millis(50));
    let stopped = |frames: &[HelperMsg]| frames.iter().any(|f| matches!(f, HelperMsg::Status { plugin, status: Status::Stopped(_) } if plugin == "hello"));
    // One slow click, then a few more: a warning and a throttle, never a stop.
    for i in 0..19 {
        host.overrun("hello", slow, budget, now + Duration::from_millis(i));
    }
    assert!(!stopped(&frames(&mut host)));
    act(&mut host, "h1", "click");
    assert_eq!(headline(&frames(&mut host), "h1").as_deref(), Some("Clicked 1×"), "input still reaches a throttled plugin");
    // Slow all the time: stopped, with the reason Studio shows.
    host.overrun("hello", slow, budget, now + Duration::from_millis(20));
    let out = frames(&mut host);
    assert!(out.iter().any(|f| matches!(f, HelperMsg::Status { plugin, status: Status::Stopped(why) } if plugin == "hello" && why.contains("slow events"))), "{out:?}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unloading_and_reloading_keeps_state() {
    let root = root("unload", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    for _ in 0..3 {
        act(&mut host, "h1", "click");
    }
    assert_eq!(headline(&frames(&mut host), "h1").as_deref(), Some("Clicked 3×"));
    host.unload("hello");
    assert_eq!(host.loaded(), 0);
    assert!(frames(&mut host).iter().any(|f| matches!(f, HelperMsg::Status { status: Status::Unloaded, .. })));
    // The next event reloads it from the saved state and re-opens its face.
    act(&mut host, "h1", "click");
    assert_eq!(host.loaded(), 1);
    assert_eq!(headline(&frames(&mut host), "h1").as_deref(), Some("Clicked 4×"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn nothing_visible_unloads_after_the_idle_period() {
    let root = root("idle", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    host.on_host(HostMsg::Close { instance: "h1".into() }, Instant::now());
    host.tick(Instant::now());
    assert_eq!(host.loaded(), 1);
    host.tick(Instant::now() + crate::host::IDLE_UNLOAD + Duration::from_secs(1));
    assert_eq!(host.loaded(), 0);
    assert!(host.deadline().is_none(), "nothing left to wake for");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unchanged_views_are_not_sent_again() {
    let root = root("diff", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    frames(&mut host);
    act(&mut host, "h1", "noop");
    assert!(!frames(&mut host).iter().any(|f| matches!(f, HelperMsg::View { .. })), "same tree, no frame");
    act(&mut host, "h1", "click");
    let out = frames(&mut host);
    let changed = out.iter().find_map(|f| match f {
        HelperMsg::View { changed, .. } => Some(*changed),
        _ => None,
    });
    assert_eq!(changed, Some(1), "only the headline changed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn warm_loads_come_from_the_aot_cache() {
    let root = root("cache", &["hello"]);
    let rt = Runtime::new(root.join("cache"), root.join("data")).unwrap();
    let dir = root.join("plugins/hello");
    let manifest = Manifest::load(&dir).unwrap();
    assert!(!rt.component(&manifest, &dir).unwrap().1, "first load compiles");
    assert!(rt.component(&manifest, &dir).unwrap().1, "second load is cached");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn removal_acknowledges_only_after_the_instance_is_dropped() {
    let root=root("remove", &["hello"]);
    let mut host=host(&root);
    open(&mut host,&root,"hello","h1");
    frames(&mut host);
    host.on_host(HostMsg::Remove {plugin:"hello".into(),acknowledge:true},Instant::now());
    assert_eq!(host.loaded(),0);
    let out=frames(&mut host);
    assert!(out.iter().any(|m|matches!(m,HelperMsg::Removed{plugin} if plugin=="hello")));
    act(&mut host,"h1","click");
    assert_eq!(host.loaded(),0,"stale actions cannot reload an uninstalled plugin");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn compile_accepts_a_single_plugin_and_reports_refusals() {
    let root=root("compile-install", &["hello"]);
    let rt=Runtime::new(root.join("cache"),root.join("data")).unwrap();
    let dir=root.join("plugins/hello");
    assert_eq!(crate::host::plugin_dirs(std::slice::from_ref(&dir)),vec![dir.clone()]);
    crate::host::compile_all(&rt,std::slice::from_ref(&dir)).unwrap();
    std::fs::write(dir.join("hello.wasm"),b"invalid wasm").unwrap();
    assert!(crate::host::compile_all(&rt,&[dir]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

fn has_live(frames: &[HelperMsg], plugin: &str) -> bool {
    frames.iter().any(|f| matches!(f, HelperMsg::Status { plugin: p, status: Status::Live } if p == plugin))
}

#[test]
fn a_command_loads_a_plugin_no_face_opened() {
    let root = root("command", &["hello"]);
    // No toast from a test run: without `notifications` the plugin's `notify.show` is refused.
    let manifest = root.join("plugins/hello/plugin.json");
    let text = std::fs::read_to_string(&manifest).unwrap().replace("\"notifications\", ", "");
    std::fs::write(&manifest, text).unwrap();
    let mut host = host(&root);
    let dir = root.join("plugins/hello").to_string_lossy().into_owned();
    host.on_host(HostMsg::Command { plugin: "hello".into(), dir: dir.clone(), id: "nope".into() }, Instant::now());
    assert_eq!(host.loaded(), 0, "an undeclared command doesn't load the plugin");
    host.on_host(HostMsg::Command { plugin: "hello".into(), dir, id: "say-hi".into() }, Instant::now());
    assert_eq!(host.loaded(), 1);
    assert!(has_live(&frames(&mut host), "hello"));
    // Nothing of it is visible, so it unloads again after the idle period.
    let now = Instant::now();
    host.tick(now);
    host.tick(now + crate::host::IDLE_UNLOAD + Duration::from_secs(1));
    assert_eq!(host.loaded(), 0);
}

#[test]
fn topic_demand_follows_visible_subscribers() {
    let root = root("demand", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "w1");
    let demand = |frames: &[HelperMsg]| frames.iter().rev().find_map(|f| match f { HelperMsg::Demand { topics } => Some(topics.clone()), _ => None });
    assert_eq!(demand(&frames(&mut host)), Some(vec!["windows".to_owned()]), "hello subscribes to windows and holds the permission");
    host.on_host(HostMsg::Held(true), Instant::now());
    assert_eq!(demand(&frames(&mut host)), Some(Vec::new()), "nothing is wanted while the desktop is held");
    host.on_host(HostMsg::Held(false), Instant::now());
    assert_eq!(demand(&frames(&mut host)), Some(vec!["windows".to_owned()]));
    host.on_host(HostMsg::Close { instance: "w1".into() }, Instant::now());
    assert_eq!(demand(&frames(&mut host)), Some(Vec::new()), "a closed face releases the lease");
}

#[test]
fn a_clicked_toast_comes_back_as_an_action() {
    let root = root("toast", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "w1");
    frames(&mut host);
    host.on_input(Input::Toast { plugin: "hello".into(), action: "hi".into() }, Instant::now());
    let caption = frames(&mut host).iter().rev().find_map(|f| match f {
        HelperMsg::View { instance, face, .. } if instance == "w1" => {
            let face: Face = serde_json::from_slice(face).unwrap();
            serde_json::to_value(&face.body).unwrap()["children"][1]["text"].as_str().map(str::to_owned)
        }
        _ => None,
    });
    assert_eq!(caption.as_deref(), Some("Toast clicked"));
}

#[test]
fn window_events_reach_a_subscribed_plugin() {
    let root = root("focus", &["hello"]);
    let mut host = host(&root);
    let dir = root.join("plugins/hello").to_string_lossy().into_owned();
    let message = HostMsg::Open { plugin: "hello".into(), dir, instance: "p1".into(), face: "details".into(), size: SizeClass::Large, settings: "{}".into() };
    host.on_host(message, Instant::now());
    frames(&mut host);
    let focused = wire::SystemEvent::Window { change: wire::WindowChange::Focused, id: 7, title: "Notes".into(), process: "notepad.exe".into() };
    host.on_host(HostMsg::System(focused), Instant::now());
    let caption = frames(&mut host).iter().rev().find_map(|f| match f {
        HelperMsg::View { instance, face, .. } if instance == "p1" => {
            let face: Face = serde_json::from_slice(face).unwrap();
            serde_json::to_value(&face.body).unwrap()["children"][1]["text"].as_str().map(str::to_owned)
        }
        _ => None,
    });
    assert_eq!(caption.as_deref(), Some("Focused: notepad.exe"));
}

#[test]
fn game_pause_has_no_deadline_and_resumes_one_timer_and_latest_system_state() {
    let root = root("game-pause", &["hello"]);
    let mut host = host(&root);
    host.on_host(HostMsg::Open { plugin: "hello".into(), dir: root.join("plugins/hello").to_string_lossy().into_owned(), instance: "h1".into(), face: "details".into(), size: SizeClass::Large, settings: "{}".into() }, Instant::now());
    frames(&mut host);
    let now = Instant::now();
    host.on_host(HostMsg::GameActivity(true), now);
    assert!(host.topics().is_empty());
    frames(&mut host);
    for process in ["first.exe", "latest.exe"] {
        host.on_host(HostMsg::System(wire::SystemEvent::Window {
            change: wire::WindowChange::Focused, id: 7, title: "Test".into(), process: process.into(),
        }), now);
    }
    let later = now + Duration::from_secs(600);
    host.tick(later);
    assert!(host.deadline().is_none(), "timers, retries, moves and unloads cannot wake the loop");
    assert!(frames(&mut host).is_empty(), "no guest event or repaint during a game");
    assert_eq!(host.loaded(), 1, "connections and state survive long games");
    host.on_host(HostMsg::GameActivity(false), later);
    let out = frames(&mut host);
    assert_eq!(out.iter().filter(|m| matches!(m, HelperMsg::View { .. })).count(), 1, "only the latest window event");
    assert!(host.topics().iter().any(|topic| topic == "windows"));
    host.tick(later);
    assert!(footer(&frames(&mut host), "h1").is_some_and(|f| f.contains("1 s awake")), "overdue periodic timer fires once");
    host.tick(later);
    assert!(frames(&mut host).is_empty(), "no catch-up burst");
    assert!(host.deadline().is_some_and(|due| due > later));
}

#[test]
fn game_pause_defers_initial_load_and_remove_discards_pending_open() {
    let root = root("game-open", &["hello"]);
    let mut host = host(&root);
    host.on_host(HostMsg::GameActivity(true), Instant::now());
    open(&mut host, &root, "hello", "h1");
    assert_eq!(host.loaded(), 0);
    host.on_host(HostMsg::Remove { plugin: "hello".into(), acknowledge: true }, Instant::now());
    host.on_host(HostMsg::GameActivity(false), Instant::now());
    assert_eq!(host.loaded(), 0, "removal cannot resurrect a deferred plugin");
    open(&mut host, &root, "hello", "h1");
    assert_eq!(host.loaded(), 1);
}

#[test]
fn game_pause_preserves_an_independent_desktop_hold() {
    let root = root("game-independent", &["hello"]);
    let mut host = host(&root);
    open(&mut host, &root, "hello", "h1");
    let now = Instant::now();
    host.on_host(HostMsg::GameActivity(true), now);
    host.on_host(HostMsg::Held(true), now);
    host.on_host(HostMsg::GameActivity(false), now);
    assert!(host.topics().is_empty(), "game exit cannot undo user/session pause");
    host.on_host(HostMsg::Held(false), now);
    assert_eq!(host.topics(), vec!["windows"]);
}

/// `cargo test --release -p glaze-plugins live_t3_event_costs -- --ignored --nocapture`, with
/// T3 Code running: the real `plugins/t3` against the local server (read-only: it subscribes and
/// redraws), printing what each event cost, the baseline decode included, and a click-style
/// action (a project pick, which only redraws) after it. Nothing is launched.
#[test]
#[ignore]
fn live_t3_event_costs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/t3");
    let scratch = std::env::temp_dir().join(format!("glaze-plugins-live-t3-{}", std::process::id()));
    let rt = Runtime::new(scratch.join("cache"), scratch.join("data")).unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let mut host = Host::new(rt, Vec::new(), tx);
    let open = HostMsg::Open { plugin: "t3".into(), dir: dir.to_string_lossy().into_owned(), instance: "t1".into(), face: "running".into(), size: SizeClass::Large, settings: "{}".into() };
    host.on_host(open, Instant::now());
    let mut slowest = Duration::ZERO;
    let until = Instant::now() + Duration::from_secs(20);
    while let Ok(input) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        let kind = match &input {
            Input::Socket { event: crate::runtime::glaze::plugin::websocket::SocketEvent::Message((_, m)), .. } => format!("socket message {} KiB", match m { crate::runtime::glaze::plugin::websocket::Message::Text(t) => t.len(), crate::runtime::glaze::plugin::websocket::Message::Binary(b) => b.len() } / 1024),
            Input::Socket { .. } => "socket open/close".into(),
            Input::Http { .. } | Input::PipeHttp { .. } => "http/process reply".into(),
            _ => "other".into(),
        };
        let (before, started) = (host.memory("t3").unwrap_or(0), Instant::now());
        host.on_input(input, Instant::now());
        slowest = slowest.max(started.elapsed());
        let after = host.memory("t3").unwrap_or(0);
        println!("{kind}: {:.1} ms, linear memory {:.2} -> {:.2} MiB", started.elapsed().as_secs_f64() * 1000.0, before as f64 / 1048576.0, after as f64 / 1048576.0);
    }
    let started = Instant::now();
    host.on_host(HostMsg::Action { instance: "t1".into(), action: "project".into(), value: Some("none".into()) }, Instant::now());
    let click = started.elapsed();
    let out = frames(&mut host);
    let stopped = out.iter().any(|f| matches!(f, HelperMsg::Status { status: Status::Stopped(_) | Status::Restarting(_), .. }));
    println!("slowest event {:.1} ms; click {:.2} ms; linear memory {:.2} MiB; stopped/restarting: {stopped}",
        slowest.as_secs_f64() * 1000.0, click.as_secs_f64() * 1000.0, host.memory("t3").unwrap_or(0) as f64 / 1048576.0);
    assert!(!stopped);
    drop(host);
    let _ = std::fs::remove_dir_all(scratch);
}
