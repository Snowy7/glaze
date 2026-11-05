//! Small T3 RPC requests. Native pairing and MCP authorization are different audiences.
use crate::{credentials, transport};
use anyhow::{Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

pub const DEFAULT_ENDPOINT: &str = "ws://127.0.0.1:3773/ws";

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Metadata {
    pub environment: Environment,
    pub providers: Vec<ProviderModels>,
    pub shell_older_settled: bool,
    pub shell_thread_families: bool,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Environment {
    pub environment_id: String,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderModels {
    pub instance_id: String,
    pub driver: String,
    pub display_name: String,
    pub auth: Auth,
    pub usage_limits: Option<UsageLimits>,
    pub installed: bool,
    pub enabled: bool,
    pub models: Vec<ProviderModel>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Auth {
    pub email: Option<String>,
    pub label: Option<String>,
}
/// A provider account's rate-limit windows, as T3's own limits page reads them.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageLimits {
    pub windows: Vec<LimitWindow>,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimitWindow {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_duration_mins: Option<u64>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct ProviderModel {
    pub slug: String,
    pub name: String,
}
#[derive(Deserialize)]
struct MetadataEnvelope {
    exit: Option<MetadataExit>,
}
#[derive(Deserialize)]
struct MetadataExit {
    value: Option<Metadata>,
}

/// getConfig includes every installed skill. Decode only models/capabilities directly from
/// the wire, so those descriptions and settings are never cloned or retained in mypaper.
pub async fn metadata(address: &str) -> Result<Metadata> {
    let secret = credentials::read("t3")?;
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        let mut socket=transport::socket_with_limit(endpoint(address),Some(secret.text()?),Some("orchestrationProtocol=2"),crate::t3::MESSAGE_LIMIT).await?;
        transport::send(&mut socket,json!({"_tag":"Request","id":"metadata","tag":"server.getConfig","payload":{},"headers":[]})).await?;
        loop {
            let message=transport::receive_message(&mut socket).await?.into_data();
            let frames:Vec<MetadataEnvelope>=if message.first()==Some(&b'['){serde_json::from_slice(&message)?}else{vec![serde_json::from_slice(&message)?]};
            for frame in frames {
                if let Some(exit)=frame.exit {return exit.value.ok_or_else(||anyhow::anyhow!("T3 pairing has expired. Connect again."));}
            }
        }
    }).await.map_err(|_|anyhow::anyhow!("T3 did not respond. Check that it is running, then retry."))?
}

pub fn endpoint(configured: &str) -> &str {
    if configured.is_empty() {
        DEFAULT_ENDPOINT
    } else {
        configured
    }
}

pub async fn request(address: &str, tag: &str, payload: Value) -> Result<Value> {
    let secret = credentials::read("t3")?;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut socket = transport::socket_with_limit(endpoint(address), Some(secret.text()?), Some("orchestrationProtocol=2"), crate::t3::MESSAGE_LIMIT).await?;
        transport::send(&mut socket, json!({"_tag":"Request","id":"request","tag":tag,"payload":payload,"headers":[]})).await?;
        loop {
            let frames = transport::receive(&mut socket).await?;
            for frame in frames.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(&frames)) {
                if frame["_tag"] == "Exit" && frame["requestId"] == "request" {
                    ensure!(frame["exit"]["_tag"] == "Success", "T3 has not granted access to this feature. Reconnect with the required permissions.");
                    return Ok(frame["exit"]["value"].clone());
                }
                if frame["_tag"] == "Defect" { bail!("T3 could not complete the request. Retry in T3 Code."); }
            }
        }
    }).await.map_err(|_| anyhow::anyhow!("T3 did not respond. Check that T3 Code is running, then retry."))?
}

pub fn check_blocking(address: &str) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async { metadata(address).await.map(|_| ()) })
}

/// A click/Enter in mypaper is the only caller. The server still enforces its client's grant.
pub fn launch_blocking(
    address: &str,
    project: &str,
    model: &crate::t3::ModelChoice,
    prompt: &str,
) -> Result<()> {
    ensure!(
        !prompt.trim().is_empty() && prompt.len() <= 32 * 1024,
        "Write a prompt of up to 32 KiB."
    );
    let payload = json!({"commandId":command_id(),"projectId":project,"title":prompt.trim().chars().take(80).collect::<String>(),
        "generateTitle":true,"modelSelection":{"instanceId":model.provider,"model":model.model},
        "runtimeMode":"approval-required","interactionMode":"default","workspaceStrategy":{"type":"root"},
        "initialMessage":{"text":prompt,"attachments":[]}});
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(request(address, "orchestration.launchThread", payload))?;
    Ok(())
}

pub fn respond_blocking(
    address: &str,
    thread: &str,
    request_id: &str,
    approve: bool,
) -> Result<()> {
    let payload = json!({"type":"runtime-request.respond","commandId":command_id(),"threadId":thread,"requestId":request_id,
        "decision":if approve {"accept"} else {"decline"}});
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(request(address, "orchestration.dispatchCommand", payload))?;
    Ok(())
}

fn command_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "mypaper-{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_micros(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Reads only the paired client's own grant; never reads T3's private credential files.
pub async fn permissions(address: &str) -> Result<Vec<String>> {
    use bytes::Bytes;
    use http_body_util::{BodyExt, Empty};
    use hyper_util::rt::TokioIo;
    let address = transport::loopback_url(endpoint(address))?;
    let host = address
        .host()
        .unwrap_or("127.0.0.1")
        .trim_matches(['[', ']']);
    let secret = credentials::read("t3")?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let stream =
            tokio::net::TcpStream::connect((host, address.port_u16().unwrap_or(80))).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        let driver = tokio::spawn(connection);
        let result = async {
            let request = hyper::Request::builder()
                .uri("/api/auth/session")
                .header("Host", address.authority().unwrap().as_str())
                .header("Authorization", format!("Bearer {}", secret.text()?))
                .body(Empty::<Bytes>::new())?;
            let response = sender.send_request(request).await?;
            ensure!(
                response.status().is_success(),
                "T3 pairing has expired. Connect again."
            );
            let mut body = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = body.frame().await {
                if let Ok(chunk) = frame?.into_data() {
                    ensure!(
                        bytes.len() + chunk.len() <= 32 * 1024,
                        "Invalid T3 session response"
                    );
                    bytes.extend_from_slice(&chunk);
                }
            }
            let response: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                response["authenticated"] == true,
                "T3 pairing has expired. Connect again."
            );
            let scopes = response.get("permissions").unwrap_or(&response["scopes"]);
            Ok(scopes
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect())
        }
        .await;
        driver.abort();
        result
    })
    .await
    .map_err(|_| anyhow::anyhow!("T3 did not respond. Retry to reconnect."))?
}
