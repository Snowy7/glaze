//! Timer-free loopback WebSockets and HTTP/1 over existing local named pipes.
use anyhow::{Result, bail, ensure};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Incoming};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::{TcpStream, windows::named_pipe::ClientOptions};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

pub const LIMIT: usize = 512 * 1024;
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub fn loopback_url(endpoint: &str) -> Result<hyper::Uri> {
    let uri: hyper::Uri = endpoint.parse()?;
    ensure!(
        matches!(uri.scheme_str(), Some("ws" | "http")),
        "only plain loopback endpoints are supported"
    );
    ensure!(
        matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]")),
        "endpoint must be loopback"
    );
    ensure!(
        uri.authority().is_some_and(|a| !a.as_str().contains('@')),
        "credentials do not belong in URLs"
    );
    ensure!(
        uri.query().is_none(),
        "endpoint query parameters are not supported"
    );
    Ok(uri)
}

pub async fn socket(
    endpoint: &str,
    authorization: Option<&str>,
    query: Option<&str>,
) -> Result<Socket> {
    socket_with_limit(endpoint, authorization, query, LIMIT).await
}

/// Protocol-specific bounds keep large T3 shell summaries separate from small control APIs.
pub async fn socket_with_limit(
    endpoint: &str,
    authorization: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Socket> {
    loopback_url(endpoint)?;
    let url = match query {
        Some(query) => format!("{endpoint}?{query}"),
        None => endpoint.to_owned(),
    };
    let mut request = url.into_client_request()?;
    if let Some(token) = authorization {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse()?);
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let (socket, _) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio_tungstenite::connect_async_with_config(request, Some(config), true),
    )
    .await??;
    Ok(socket)
}

pub async fn send(socket: &mut Socket, value: Value) -> Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(&value)?.into()))
        .await?;
    Ok(())
}

pub async fn receive(socket: &mut Socket) -> Result<Value> {
    let message = receive_message(socket).await?;
    Ok(serde_json::from_slice(&message.into_data())?)
}

/// Typed decoders can skip unneeded large fields without first allocating a JSON tree.
pub async fn receive_message(socket: &mut Socket) -> Result<Message> {
    loop {
        match socket.next().await.transpose()? {
            Some(message @ (Message::Text(_) | Message::Binary(_))) => return Ok(message),
            // Server maintenance disqualifies the persistent mode. Do not conceal it with a pong loop.
            Some(Message::Ping(_) | Message::Pong(_)) => {
                bail!("server heartbeat detected; use snapshot-on-open")
            }
            Some(Message::Close(_)) | None => bail!("connection closed"),
            Some(_) => {}
        }
    }
}

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
    pub async fn finish(mut self) -> Result<()> {
        while let Some(frame) = self.body.frame().await.transpose()? {
            let _ = frame;
        }
        Ok(())
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
