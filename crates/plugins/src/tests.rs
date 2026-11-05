//! Runtime tests against the real `hello` component (`scripts/build-plugins.ps1` builds it into
//! `plugins/hello/hello.wasm`).
#[path = "../../../plugins/docker/src/runtime_tests.rs"]
mod docker;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mypaper_plugin_api::SizeClass;
use mypaper_plugin_api::wire::{self, HelperMsg, HostMsg, Status};
use mypaper_plugin_sdk::view::Face;

use crate::host::{Host, Input};
use crate::runtime::{Manifest, Runtime};

fn hello_wasm() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/hello/hello.wasm");
    assert!(path.is_file(), "build the guest first: scripts/build-plugins.ps1 ({})", path.display());
    path
}

/// A temp root with `hello` installed under each id, plus the host's cache and data dirs.
fn root(name: &str, ids: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mypaper-plugins-{name}-{}", std::process::id()));
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
    refused(&original.replace("\"api\": \"0.1\"", "\"api\": \"1.0\""), "newer mypaper");
    refused(&original.replace("\"api\": \"0.1\"", "\"api\": \"0.9\""), "newer mypaper");
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
    assert!(spun < Duration::from_millis(500), "the budget stopped it after {spun:?}");
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
