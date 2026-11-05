//! Effect RPC JSON summary subscription pinned to T3 protocol 2 / a21d7887. No client ping task.
use crate::{Availability, Data, Publish, credentials, transport};
use anyhow::{Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 2;
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider_instance_id: String,
    pub status: Status,
    pub latest_run_id: Option<String>,
    pub active_run_id: Option<String>,
    pub pending_runtime_request: Option<Request>,
    pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Request {
    pub id: String,
    pub kind: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Idle,
    Queued,
    Preparing,
    Starting,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    #[serde(rename = "rolled_back")]
    RolledBack,
}
impl Thread {
    pub fn attention(&self) -> bool {
        self.pending_runtime_request.is_some()
    }
    pub fn running(&self) -> bool {
        matches!(
            self.status,
            Status::Queued
                | Status::Preparing
                | Status::Starting
                | Status::Running
                | Status::Waiting
        )
    }
    pub fn finished(&self) -> bool {
        matches!(
            self.status,
            Status::Completed
                | Status::Failed
                | Status::Cancelled
                | Status::Interrupted
                | Status::RolledBack
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub sequence: u64,
    pub threads: Vec<Thread>,
    pub projects: BTreeMap<String, String>,
    pub transitions: Vec<Transition>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub thread_id: String,
    pub run_id: String,
    pub status: Status,
    pub attention: bool,
}

impl Snapshot {
    pub fn apply(&mut self, item: &Value) -> Result<()> {
        match item["kind"].as_str() {
            Some("snapshot") => {
                let snapshot = &item["snapshot"];
                ensure!(
                    snapshot["schemaVersion"].as_u64().is_some(),
                    "missing T3 schema version"
                );
                // Enrichment snapshots update project identity only; their empty thread array is not authoritative.
                if item.get("resolvedRepositoryIdentityRoots").is_some() {
                    for project in snapshot["projects"].as_array().into_iter().flatten() {
                        if let (Some(id), Some(title)) =
                            (project["id"].as_str(), project["title"].as_str())
                        {
                            self.projects.insert(id.into(), title.into());
                        }
                    }
                    return Ok(());
                }
                let threads: Vec<Thread> = serde_json::from_value(snapshot["threads"].clone())?;
                ensure!(
                    threads.len() <= 256,
                    "T3 snapshot exceeds 256 threads; narrow the server projection"
                );
                self.threads = threads;
                self.sequence = snapshot["snapshotSequence"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("missing T3 sequence"))?;
                self.projects.clear();
                for project in snapshot["projects"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(title)) = (
                        project["id"].as_str(),
                        project["title"]
                            .as_str()
                            .or_else(|| project["name"].as_str()),
                    ) {
                        self.projects.insert(id.into(), title.into());
                    }
                }
                self.transitions.clear(); // A new/reconnected snapshot is a baseline, never historic completion notices.
            }
            Some("synchronized") => {}
            Some(
                kind
                @ ("thread.updated" | "thread.removed" | "project.updated" | "project.removed"),
            ) => {
                let sequence = item["sequence"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("missing T3 sequence"))?;
                if sequence <= self.sequence {
                    return Ok(());
                }
                // Filtered summary streams legitimately skip global sequence numbers.
                self.sequence = sequence;
                match kind {
                    "thread.updated" => {
                        let thread: Thread = serde_json::from_value(item["thread"].clone())?;
                        let previous = self.threads.iter().position(|t| t.id == thread.id);
                        if item["location"] == "archive" {
                            self.threads.retain(|t| t.id != thread.id);
                            return Ok(());
                        }
                        if let Some(index) = previous {
                            let old = &self.threads[index];
                            let attention = thread.pending_runtime_request.as_ref().map(|r| &r.id)
                                != old.pending_runtime_request.as_ref().map(|r| &r.id)
                                && thread.attention();
                            let completion = thread.finished()
                                && (old.latest_run_id != thread.latest_run_id || !old.finished());
                            if (attention || completion)
                                && let Some(run_id) = thread
                                    .active_run_id
                                    .as_ref()
                                    .or(thread.latest_run_id.as_ref())
                            {
                                let transition = Transition {
                                    thread_id: thread.id.clone(),
                                    run_id: run_id.clone(),
                                    status: thread.status,
                                    attention,
                                };
                                if !self.transitions.contains(&transition) {
                                    self.transitions.push(transition);
                                    if self.transitions.len() > 16 {
                                        self.transitions.remove(0);
                                    }
                                }
                            }
                            self.threads[index] = thread;
                        } else {
                            ensure!(
                                self.threads.len() < 256,
                                "T3 thread bound exceeded; refresh the projection"
                            );
                            self.threads.push(thread);
                        }
                    }
                    "thread.removed" => self
                        .threads
                        .retain(|t| Some(t.id.as_str()) != item["threadId"].as_str()),
                    "project.updated" => {
                        if let (Some(id), Some(title)) = (
                            item["project"]["id"].as_str(),
                            item["project"]["title"].as_str(),
                        ) {
                            self.projects.insert(id.into(), title.into());
                        }
                    }
                    "project.removed" => {
                        if let Some(id) = item["projectId"].as_str() {
                            self.projects.remove(id);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => bail!("unsupported T3 shell event; update the adapter"),
        }
        Ok(())
    }
}

pub(crate) async fn run(config: &crate::config::T3, publish: &Publish) -> Result<()> {
    ensure!(
        config.protocol_version == PROTOCOL_VERSION,
        "supported T3 orchestration protocol is 2"
    );
    ensure!(
        !config.endpoint.is_empty(),
        "configure the T3 /ws loopback endpoint and pair a read-only mypaper client"
    );
    let token = credentials::read("t3").map_err(|_| anyhow::anyhow!("pair mypaper with T3 using orchestration:read and store the token in Credential Manager"))?;
    subscribe(config, publish, token.text()?).await
}
async fn subscribe(config: &crate::config::T3, publish: &Publish, token: &str) -> Result<()> {
    let mut socket = transport::socket(
        &config.endpoint,
        Some(token),
        Some("orchestrationProtocol=2"),
    )
    .await?;
    transport::send(&mut socket,json!({"_tag":"Request","id":"1","tag":"orchestration.subscribeShell","payload":{"requestCompletionMarker":true},"headers":[]})).await?;
    let mut state = Snapshot::default();
    let mut baseline = false;
    loop {
        let frames = transport::receive(&mut socket).await?;
        publish.delivered();
        let frames = match frames {
            Value::Array(frames) => frames,
            frame => vec![frame],
        };
        for frame in frames {
            match frame["_tag"].as_str() {
                Some("Chunk") => {
                    ensure!(frame["requestId"] == "1", "unexpected T3 request id");
                    for item in frame["values"]
                        .as_array()
                        .ok_or_else(|| anyhow::anyhow!("invalid T3 chunk"))?
                    {
                        ensure!(
                            baseline || item["kind"] == "snapshot",
                            "T3 delta arrived before its baseline"
                        );
                        state.apply(item)?;
                        baseline = true;
                    }
                    publish.set(
                        if config.live_verified {
                            Availability::Live
                        } else {
                            Availability::Cached
                        },
                        Some(Data::T3(state.clone())),
                    );
                    transport::send(&mut socket, json!({"_tag":"Ack","requestId":"1"})).await?;
                    if !config.live_verified {
                        return Ok(());
                    }
                }
                Some("Ping" | "Pong") => {
                    bail!("T3 server maintenance frames detected; use snapshot-on-open")
                }
                Some("Exit" | "Defect") => bail!(
                    "T3 subscription ended or was rejected; check pairing and protocol version"
                ),
                _ => bail!("unsupported Effect RPC envelope"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::{Arc, Mutex};
    use tokio_tungstenite::tungstenite::Message;
    #[test]
    #[allow(
        clippy::result_large_err,
        reason = "tungstenite handshake callback has a fixed external error type"
    )]
    fn authenticated_rpc_fixture_seeds_acknowledges_stops_and_reconnects() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config=crate::config::T3 { enabled:true,endpoint:format!("ws://{}/ws",listener.local_addr().unwrap()),..Default::default() };
            let server=tokio::spawn(async move {
                for _ in 0..2 {
                    let (stream,_)=listener.accept().await.unwrap();
                    let mut socket=tokio_tungstenite::accept_hdr_async(stream,|request:&tokio_tungstenite::tungstenite::handshake::server::Request,response| {
                        assert_eq!(request.uri().query(),Some("orchestrationProtocol=2"));
                        assert_eq!(request.headers()["authorization"],"Bearer fixture-only");
                        Ok(response)
                    }).await.unwrap();
                    let request:Value=serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                    assert_eq!(request["tag"],"orchestration.subscribeShell");
                    socket.send(Message::Text(json!({"_tag":"Chunk","requestId":"1","values":[{"kind":"snapshot","snapshot":{"schemaVersion":1,"snapshotSequence":4,"projects":[],"threads":[]}}]}).to_string().into())).await.unwrap();
                    let ack:Value=serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                    assert_eq!(ack,json!({"_tag":"Ack","requestId":"1"}));
                    assert!(socket.next().await.is_none_or(|r|r.is_err() || matches!(r,Ok(Message::Close(_)))));
                }
            });
            let publish=Publish { provider:crate::Provider::T3,snapshot:Arc::new(Mutex::new(crate::Snapshot::default())),wake:Arc::new(|_|{}),counters:Arc::new(crate::Counters::default()) };
            for _ in 0..2 { subscribe(&config,&publish,"fixture-only").await.unwrap();assert_eq!(publish.snapshot.lock().unwrap().availability,Availability::Cached); }
            server.await.unwrap();
        });
    }
}
