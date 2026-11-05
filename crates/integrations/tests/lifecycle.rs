//! Real loopback protocol fixture: sharing, cancellation, explicit reconnect and no retry loop.
use mypaper_integrations::{Availability, Hub, Provider, config::Config};
use serde_json::json;
use std::{net::TcpListener, sync::mpsc, time::Duration};
use tokio_tungstenite::tungstenite::{Message, accept};

#[test]
fn obs_shared_start_stop_and_explicit_reconnect_release_the_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let (events, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = accept(stream).unwrap();
            socket
                .send(Message::Text(
                    json!({"op":0,"d":{"rpcVersion":1}}).to_string().into(),
                ))
                .unwrap();
            let identify: serde_json::Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(identify["d"]["eventSubscriptions"], 68);
            socket
                .send(Message::Text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            for _ in 0..3 {
                let request: serde_json::Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                let mut reply = request["d"].clone();
                reply["requestStatus"] = json!({"result":true});
                reply["responseData"] = json!({"outputActive":false,"outputPaused":false,"currentProgramSceneName":"Desktop"});
                socket
                    .send(Message::Text(json!({"op":7,"d":reply}).to_string().into()))
                    .unwrap();
            }
            events.send("connected").unwrap();
            assert!(socket.read().is_err()); // No outgoing ping, state read, or retry is permitted while idle.
            events.send("closed").unwrap();
        }
    });
    let (changed, changes) = mpsc::channel();
    let hub = Hub::new(move |_| {
        let _ = changed.send(());
    });
    let mut config = Config::default();
    config.obs.enabled = true;
    config.obs.endpoint = endpoint;
    let a = hub.acquire(Provider::Obs, &config).unwrap();
    let b = hub.acquire(Provider::Obs, &config).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "connected"
    );
    while a.snapshot().availability != Availability::Live {
        changes.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    drop(a);
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    b.retry();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "closed");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "connected"
    );
    hub.stop_all();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "closed");
    let c = hub.acquire(Provider::Obs, &config).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "connected"
    );
    drop(b); // An old lease cannot decrement the replacement generation.
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    drop(c);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "closed");
    server.join().unwrap();
}
