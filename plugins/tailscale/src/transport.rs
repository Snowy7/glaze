//! HTTP/1 over the existing Tailscale LocalAPI pipe, copied from the native transport.
use anyhow::{Result, bail, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Incoming};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::windows::named_pipe::ClientOptions;
pub const LIMIT: usize = 512 * 1024;
pub struct JsonStream {
    body: Incoming,
    buffer: Vec<u8>,
    _task: ConnectionTask,
}
struct ConnectionTask(tokio::task::JoinHandle<Result<(), hyper::Error>>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Each response owns its connection driver, canceled with its stream/consumer.
pub async fn pipe_request(
    pipe: &str,
    method: &str,
    path: &str,
    data: Option<Value>,
) -> Result<JsonStream> {
    ensure!(
        pipe.starts_with(r"\\.\pipe\") && !pipe.contains(['\r', '\n']),
        "expected a local pipe"
    );
    let pipe = ClientOptions::new()
        .security_qos_flags(0x0010_0000 | 0x0001_0000)
        .open(pipe)?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(pipe)).await?;
    let task = ConnectionTask(tokio::spawn(connection));
    let bytes = data
        .map(|d| serde_json::to_vec(&d))
        .transpose()?
        .unwrap_or_default();
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("Host", "local-tailscaled.sock")
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(bytes)))?;
    let response = match sender.send_request(request).await {
        Ok(response) => response,
        Err(e) => {
            return Err(e.into());
        }
    };
    if !response.status().is_success() {
        bail!("local API returned HTTP {}", response.status().as_u16());
    }
    Ok(JsonStream {
        body: response.into_body(),
        buffer: Vec::new(),
        _task: task,
    })
}

impl JsonStream {
    /// Incremental JSON decoding works across HTTP chunks and adjacent/newline-delimited objects.
    pub async fn next(&mut self) -> Result<Option<Value>> {
        loop {
            let mut decoder =
                serde_json::Deserializer::from_slice(&self.buffer).into_iter::<Value>();
            match decoder.next() {
                Some(Ok(value)) => {
                    let consumed = decoder.byte_offset();
                    self.buffer.drain(..consumed);
                    return Ok(Some(value));
                }
                Some(Err(error)) if !error.is_eof() => return Err(error.into()),
                _ => {}
            }
            match self.body.frame().await.transpose()? {
                Some(frame) => {
                    if let Ok(bytes) = frame.into_data() {
                        ensure!(
                            self.buffer.len() + bytes.len() <= LIMIT,
                            "local API message exceeds 512 KiB"
                        );
                        self.buffer.extend_from_slice(&bytes);
                    }
                }
                None => {
                    ensure!(
                        self.buffer.iter().all(u8::is_ascii_whitespace),
                        "truncated local API message"
                    );
                    return Ok(None);
                }
            }
        }
    }
}

pub async fn pipe_json(pipe: &str, method: &str, path: &str, data: Option<Value>) -> Result<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        pipe_request(pipe, method, path, data)
            .await?
            .next()
            .await?
            .ok_or_else(|| anyhow::anyhow!("empty API response"))
    })
    .await?
}
