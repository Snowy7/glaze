//! Loads the shipped component in the real runtime; no desktop input or real OBS actions.
use std::{path::PathBuf, time::SystemTime};
use serde_json::json;
use crate::runtime::{Effect, Manifest, Runtime};
use crate::runtime::exports::mypaper::plugin::lifecycle::{Event, FaceOpen};
use crate::runtime::mypaper::plugin::{types::SizeClass, websocket::{Message, SocketEvent}};

fn latest(effects: &[Effect]) -> mypaper_plugin_sdk::Face {
    effects.iter().rev().find_map(|e| match e { Effect::View(_, face) => Some(face.clone()), _ => None }).expect("component rendered a face")
}

#[test]
fn obs_component_uses_scoped_websocket_and_renders_offline_without_controls() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/obs");
    let scratch = std::env::temp_dir().join(format!("mypaper-obs-component-{}", std::process::id()));
    let runtime = Runtime::new(scratch.join("cache"), scratch.join("data")).unwrap();
    let manifest = Manifest::load(&root).unwrap();
    let (component, _) = runtime.component(&manifest, &root).unwrap();
    let mut instance = runtime.instantiate_with_settings(&manifest, &component, None, "{\"port\":4466}").unwrap();
    let open = Event::Open(FaceOpen { instance: "obs-fixture".into(), face: "controls".into(), size: SizeClass::Large, settings: "{}".into() });
    let effects = instance.event(&open).unwrap();
    let socket = effects.iter().find_map(|e| match e {
        Effect::Socket { id, request } => {
            assert_eq!(request.url, "ws://127.0.0.1:4466");
            assert_eq!(request.protocols, ["obswebsocket.json"]);
            Some(*id)
        }
        _ => None,
    }).expect("runtime accepted the configured origin");
    let socket_event = |value: serde_json::Value| Event::Socket(SocketEvent::Message((socket, Message::Text(value.to_string()))));
    instance.event(&socket_event(json!({"op":0,"d":{"rpcVersion":1}}))).unwrap();
    instance.event(&socket_event(json!({"op":2,"d":{"negotiatedRpcVersion":1}}))).unwrap();
    let responses = [
        ("GetRecordStatus", json!({"outputActive":true,"outputDuration":1000})),
        ("GetStreamStatus", json!({"outputActive":true,"outputDuration":2000,"outputBytes":1000,"outputSkippedFrames":2})),
        ("GetCurrentProgramScene", json!({"currentProgramSceneName":"Camera"})),
        ("GetSceneList", json!({"scenes":[{"sceneName":"Camera"},{"sceneName":"Desktop"}]})),
        ("GetInputList", json!({"inputs":[{"inputName":"Desk mic","inputKind":"wasapi_input_capture"}]})),
        ("GetStats", json!({"cpuUsage":2.4})),
        ("GetInputMute", json!({"inputMuted":false})),
    ];
    let mut effects = vec![];
    for (i, (kind, response)) in responses.into_iter().enumerate() {
        effects = instance.event(&socket_event(json!({"op":7,"d":{"requestType":kind,"requestId":(i+1).to_string(),"requestStatus":{"result":true},"responseData":response}}))).unwrap();
    }
    let live = serde_json::to_string(&latest(&effects)).unwrap();
    for expected in ["record.stop", "stream.stop", "mic:0", "scene:1", "2.4% CPU", "2 dropped"] { assert!(live.contains(expected), "missing {expected}: {live}"); }
    let offline = instance.event(&Event::Socket(SocketEvent::Closed((socket, None)))).unwrap();
    let offline = serde_json::to_string(&latest(&offline)).unwrap();
    assert!(offline.contains("OBS isn't running"));
    for action in ["record.stop", "stream.stop", "mic:0", "scene:1"] { assert!(!offline.contains(action)); }
    let memory = instance.store.data().limiter.used;
    println!("OBS linear memory: {} bytes; sampled at {:?}", memory, SystemTime::now());
    assert!(memory <= 8 * 1024 * 1024);
    instance.event(&Event::Close("obs-fixture".into())).unwrap();
    drop(instance);
    drop(runtime);
    std::fs::remove_dir_all(&scratch).unwrap();
}
