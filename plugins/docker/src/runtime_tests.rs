//! Real Wasmtime guest tests. Effects are inspected, never executed against the user's engine.
use crate::runtime::{Effect, Instance, Manifest, Runtime};
use crate::runtime::exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen};
use crate::runtime::mypaper::plugin::{http::{Reply, Response}, types::{Error, SizeClass}};
use std::path::Path;

fn request(effects: &[Effect], method: &str, path: &str) -> u64 {
    effects.iter().find_map(|effect| match effect {
        Effect::PipeHttp { id, name, method: verb, url, .. } if *verb == method && url.ends_with(path) => {
            assert_eq!(name, "docker_engine");
            Some(*id)
        }
        _ => None,
    }).unwrap_or_else(|| panic!("missing {method} {path}: {effects:?}"))
}

fn reply(guest: &mut Instance, id: u64, status: u16, body: &str) -> Vec<Effect> {
    guest.event(&Event::Http(Reply { id, outcome: Ok(Response { status, headers: vec![], body: body.as_bytes().to_vec() }) })).unwrap()
}

#[test]
fn docker_component_reads_metrics_controls_and_offline_state_without_engine_side_effects() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/docker");
    let scratch = std::env::temp_dir().join(format!("mypaper-docker-component-{}", std::process::id()));
    let rt = Runtime::new(scratch.join("cache"), scratch.join("data")).unwrap();
    let manifest = Manifest::load(&dir).unwrap();
    let (component, _) = rt.component(&manifest, &dir).unwrap();
    let mut guest = rt.instantiate(&manifest, &component, None).unwrap();
    let open = || Event::Open(FaceOpen { instance: "docker-test".into(), face: "containers".into(), size: SizeClass::Large, settings: "{}".into() });
    let action = |action: &str| Event::Action(Action { instance: "docker-test".into(), action: action.into(), value: None });
    let effects = guest.event(&open()).unwrap();
    let id = request(&effects, "GET", "/containers/json?all=1&limit=128");
    let effects = reply(&mut guest, id, 200, r#"[{"Id":"web","Names":["/web"],"State":"running","Status":"Up"}]"#);
    let id = request(&effects, "GET", "/containers/web/stats?stream=false&one-shot=true");
    let effects = reply(&mut guest, id, 200, r#"{"cpu_stats":{"cpu_usage":{"total_usage":500},"system_cpu_usage":2000,"online_cpus":4},"precpu_stats":{"cpu_usage":{"total_usage":400},"system_cpu_usage":1000},"memory_stats":{"usage":4096,"stats":{"inactive_file":1024}}}"#);
    let text = format!("{effects:?}");
    assert!(text.contains("40%"), "{text}");
    assert!(text.contains("stop:web"));
    assert!(!guest.event(&action("stop:missing")).unwrap().iter().any(|e| matches!(e, Effect::PipeHttp { .. })));
    let effects = guest.event(&action("stop:web")).unwrap();
    let id = request(&effects, "POST", "/containers/web/stop");
    assert!(!guest.event(&action("stop:web")).unwrap().iter().any(|e| matches!(e, Effect::PipeHttp { .. })), "pending actions are coalesced");
    let effects = reply(&mut guest, id, 204, "");
    let id = request(&effects, "GET", "/containers/json?all=1&limit=128");
    reply(&mut guest, id, 200, r#"[{"Id":"web","Names":["/web"],"State":"exited","Status":"Exited (0)"}]"#);
    let effects = guest.event(&action("start:web")).unwrap();
    request(&effects, "POST", "/containers/web/start");
    let effects = guest.event(&Event::Close("docker-test".into())).unwrap();
    assert!(effects.iter().any(|e| matches!(e, Effect::Cancel(id) if id == "containers")));
    let effects = guest.event(&open()).unwrap();
    let id = request(&effects, "GET", "/containers/json?all=1&limit=128");
    let effects = guest.event(&Event::Http(Reply { id, outcome: Err(Error::Failed("pipe not found".into())) })).unwrap();
    assert!(format!("{effects:?}").contains("Docker isn't running"));
    assert!(!guest.event(&action("start:web")).unwrap().iter().any(|e| matches!(e, Effect::PipeHttp { .. })));
    println!("Docker WASM memory: {} bytes; {} guest events", guest.memory(), guest.events);
    drop(guest);
    drop(rt);
    let _ = std::fs::remove_dir_all(scratch);
}
