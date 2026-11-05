//! Native named-pipe HTTP fixtures cover streaming boundaries, cancellation and explicit reconnect.
use mypaper_integrations::{Action, Availability, Hub, Provider, config::Config};
use std::{sync::mpsc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
};

async fn serve(mut pipe: NamedPipeServer, events: mpsc::Sender<String>) {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        if pipe.read_exact(&mut byte).await.is_err() {
            return;
        }
        request.push(byte[0]);
        assert!(request.len() < 8192);
    }
    let request = String::from_utf8(request).unwrap();
    let path = request
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap();
    if path.starts_with("/events?") || path.starts_with("/localapi/v0/watch-ipn-bus?") {
        let tailscale = path.starts_with("/localapi");
        if tailscale {
            assert!(path.ends_with(&format!(
                "mask={}",
                mypaper_integrations::tailscale::WATCH_MASK
            )));
        }
        pipe.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        if tailscale {
            // Deliberately split a JSON object over separate HTTP reads.
            pipe.write_all(b"{\"State\":\"Run").await.unwrap();
            pipe.write_all(b"ning\",\"InitialStatus\":{\"BackendState\":\"Running\",\"TailscaleIPs\":[\"100.64.0.1\"]}}\n").await.unwrap();
        }
        events.send("stream".into()).unwrap();
        let mut buffer = [0; 64];
        let result = pipe.read(&mut buffer).await;
        assert!(matches!(result, Err(_) | Ok(0)));
        let _ = events.send("closed".into());
    } else {
        events.send(path.to_owned()).unwrap();
        let body = if path.starts_with("/containers/json") {
            r#"[{"Id":"a","Names":["/web"],"State":"running","Status":"Up"}]"#
        } else if path.starts_with("/containers/a/stats?") {
            r#"{"cpu_stats":{"cpu_usage":{"total_usage":500},"system_cpu_usage":2000,"online_cpus":4},"precpu_stats":{"cpu_usage":{"total_usage":400},"system_cpu_usage":1000},"memory_stats":{"usage":4096,"limit":8192}}"#
        } else {
            "{}"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = pipe.write_all(response.as_bytes()).await;
    }
}

#[test]
fn docker_and_tailscale_start_stop_reconnect_and_controls_use_the_existing_pipe() {
    for provider in [Provider::Docker, Provider::Tailscale] {
        let path = format!(
            r"\\.\pipe\mypaper-fixture-{}-{provider:?}",
            std::process::id()
        );
        let (events, rx) = mpsc::channel();
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let (ready, ready_rx) = mpsc::channel();
        let server_path = path.clone();
        let server = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
                let mut first=true;
                loop {
                    let pipe=ServerOptions::new().create(&server_path).unwrap();
                    if first { ready.send(()).unwrap();first=false; }
                    tokio::select! { _=&mut stopped=>break,result=pipe.connect()=>{result.unwrap();tokio::spawn(serve(pipe,events.clone()));} }
                }
            });
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (changed, changes) = mpsc::channel();
        let hub = Hub::new(move |_| {
            let _ = changed.send(());
        });
        let mut config = Config::default();
        config.docker.enabled = true;
        config.docker.pipe = path.clone();
        config.tailscale.enabled = true;
        config.tailscale.pipe = path;
        config.tailscale.live_verified = true;
        let lease = hub.acquire(provider, &config).unwrap();
        while lease.snapshot().availability != Availability::Live {
            changes.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "stream");
        if provider == Provider::Docker {
            assert!(
                rx.recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .starts_with("/containers/json")
            );
            lease.action(Action::DockerMetrics).unwrap();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                "/containers/a/stats?stream=false&one-shot=true"
            );
            loop {
                if let Some(mypaper_integrations::Data::Docker(state)) = lease.snapshot().data
                    && let Some(usage) = state.usage.get("a")
                {
                    assert_eq!(usage.cpu_hundredths, Some(4000));
                    break;
                }
                changes.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
        lease
            .action(match provider {
                Provider::Docker => Action::Docker {
                    id: "a".into(),
                    start: false,
                },
                _ => Action::Tailscale { connected: false },
            })
            .unwrap();
        assert!(
            rx.recv_timeout(Duration::from_secs(5))
                .unwrap()
                .starts_with(if provider == Provider::Docker {
                    "/containers/a/stop"
                } else {
                    "/localapi/v0/prefs"
                })
        );
        lease.retry();
        let mut reconnected = false;
        for _ in 0..3 {
            if rx.recv_timeout(Duration::from_secs(5)).unwrap() == "stream" {
                reconnected = true;
                break;
            }
        }
        assert!(reconnected);
        drop(lease);
        drop(hub);
        let _ = stop.send(());
        server.join().unwrap();
    }
}
