//! The shipped guest in Wasmtime. Preference effects are inspected, never run on the user's VPN.
use crate::runtime::{Effect, Instance, Manifest, Runtime};
use crate::runtime::exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen};
use crate::runtime::mypaper::plugin::{http::{Reply, Response}, types::{Error, SizeClass}};
use serde_json::{Value, json};
use std::path::Path;

fn request(effects: &[Effect], verb: &str, path: &str) -> (u64, Option<Value>) {
    effects.iter().find_map(|effect| match effect {
        Effect::PipeHttp { id, name, method, url, body, .. } if *method == verb && url.ends_with(path) => {
            assert_eq!(name, r"ProtectedPrefix\Administrators\Tailscale\tailscaled");
            Some((*id, body.as_ref().map(|b| serde_json::from_slice(b).unwrap())))
        }
        _ => None,
    }).unwrap_or_else(|| panic!("missing {verb} {path}: {effects:?}"))
}

fn reply(guest: &mut Instance, id: u64, status: u16, body: Value) -> Vec<Effect> {
    guest.event(&Event::Http(Reply { id, outcome: Ok(Response { status, headers: vec![], body: body.to_string().into_bytes() }) })).unwrap()
}

#[test]
fn tailscale_component_masks_actions_rejects_stale_replies_and_reports_disconnects() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/tailscale");
    let scratch = std::env::temp_dir().join(format!("mypaper-tailscale-component-{}", std::process::id()));
    let runtime = Runtime::new(scratch.join("cache"), scratch.join("data")).unwrap();
    let manifest = Manifest::load(&dir).unwrap();
    let (component, _) = runtime.component(&manifest, &dir).unwrap();
    let mut guest = runtime.instantiate(&manifest, &component, None).unwrap();
    let action = |name: &str| Event::Action(Action { instance: "test".into(), action: name.into(), value: None });
    let effects = guest.event(&Event::Open(FaceOpen { instance: "test".into(), face: "connection".into(), size: SizeClass::Large, settings: "{}".into() })).unwrap();
    let (id, _) = request(&effects, "GET", "/status");
    let status = json!({"BackendState":"Running","TailscaleIPs":["100.64.0.1"],"Peer":{
        "home":{"ID":"home","HostName":"Home","OS":"Linux","Online":true,"ExitNodeOption":true,"RxBytes":1024,"TxBytes":2048},
        "away":{"ID":"away","HostName":"Away","OS":"Windows","Online":false,"LastSeen":"2026-11-04T00:00:00Z"}
    }});
    let live = reply(&mut guest, id, 200, status.clone());
    assert!(format!("{live:?}").contains("Connected"));
    assert!(!guest.event(&action("exit:forged")).unwrap().iter().any(|e| matches!(e, Effect::PipeHttp { .. })));
    let effects = guest.event(&action("refresh")).unwrap();
    let (stale, _) = request(&effects, "GET", "/status");
    let effects = guest.event(&action("disconnect")).unwrap();
    let (patch, body) = request(&effects, "PATCH", "/prefs");
    assert_eq!(body.unwrap(), json!({"WantRunning":false,"WantRunningSet":true}));
    assert!(!guest.event(&action("connect")).unwrap().iter().any(|e| matches!(e, Effect::PipeHttp { .. })));
    let effects = reply(&mut guest, patch, 200, json!({}));
    let (fresh, _) = request(&effects, "GET", "/status");
    reply(&mut guest, fresh, 200, json!({"BackendState":"Stopped"}));
    let effects = reply(&mut guest, stale, 200, status.clone());
    let text = format!("{effects:?}");
    assert!(text.contains("Disconnected") && !text.contains("\"Connected\""), "{text}");
    let effects = guest.event(&action("connect")).unwrap();
    let (patch, body) = request(&effects, "PATCH", "/prefs");
    assert_eq!(body.unwrap(), json!({"WantRunning":true,"WantRunningSet":true}));
    let effects = reply(&mut guest, patch, 200, json!({}));
    let (fresh, _) = request(&effects, "GET", "/status");
    reply(&mut guest, fresh, 200, status);
    for (action_name, expected) in [("exit:home", "home"), ("exit:none", "")] {
        let effects = guest.event(&action(action_name)).unwrap();
        let (patch, body) = request(&effects, "PATCH", "/prefs");
        assert_eq!(body.unwrap(), json!({"ExitNodeID":expected,"ExitNodeIDSet":true,"ExitNodeIP":"","ExitNodeIPSet":true,"AutoExitNode":"","AutoExitNodeSet":true}));
        let effects = reply(&mut guest, patch, 200, json!({}));
        let (id, _) = request(&effects, "GET", "/status");
        let effects = guest.event(&Event::Http(Reply { id, outcome: Err(Error::Failed("pipe closed".into())) })).unwrap();
        assert!(!format!("{effects:?}").contains("\"Connected\""));
    }
    assert!(guest.memory() <= 16 * 1024 * 1024);
    println!("Tailscale guest: {} bytes, {} events", guest.memory(), guest.events);
    drop(guest);
    drop(component);
    drop(runtime);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn tailscale_pipe_http_provides_identity_without_impersonation_privileges() {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, RevertToSelf, SecurityAnonymous, SecurityIdentification, TokenImpersonationLevel, TOKEN_QUERY},
        Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile},
        System::Threading::{GetCurrentThread, OpenThreadToken},
        System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, ImpersonateNamedPipeClient, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT},
    };
    struct Server(HANDLE);
    // SAFETY: the server handle is exclusively transferred to its worker.
    unsafe impl Send for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            // SAFETY: this fixture owns the server handle and closes it once after its I/O ends.
            unsafe { let _ = CloseHandle(self.0); }
        }
    }
    let name = format!("mypaper-tailscale-auth-{}", std::process::id());
    // SAFETY: creates an owned local pipe; all buffers below live through synchronous calls.
    let server = Server(unsafe { CreateNamedPipeW(&windows::core::HSTRING::from(format!(r"\\.\pipe\{name}")), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, None) });
    assert!(!server.0.is_invalid());
    let worker = std::thread::spawn(move || {
        let server = server;
        // SAFETY: exclusively owned pipe; revert immediately after the identity check.
        unsafe {
            let _ = ConnectNamedPipe(server.0, None);
            let mut buffer = [0u8; 4096];
            let mut read = 0;
            ReadFile(server.0, Some(&mut buffer), Some(&mut read), None).unwrap();
            let impersonating = ImpersonateNamedPipeClient(server.0).is_ok();
            let mut token = HANDLE::default();
            let identity = if impersonating && OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token).is_ok() {
                let mut level = SecurityAnonymous;
                let mut needed = 0;
                GetTokenInformation(token, TokenImpersonationLevel, Some((&raw mut level).cast()), size_of_val(&level) as u32, &mut needed).unwrap();
                CloseHandle(token).unwrap();
                level == SecurityIdentification
            } else { false };
            if impersonating { RevertToSelf().unwrap(); }
            let response = if identity { b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".as_slice() } else { b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n".as_slice() };
            WriteFile(server.0, Some(response), Some(&mut read), None).unwrap();
            FlushFileBuffers(server.0).unwrap();
            identity
        }
    });
    let response = crate::io::pipe_http(&name, "GET", "http://local-tailscaled.sock/localapi/v0/status", &[], None).unwrap();
    let identity = worker.join().unwrap();
    assert_eq!(response.status, 200, "LocalAPI needs identification-level client identity");
    assert!(identity);
}
