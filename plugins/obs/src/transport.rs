//! Bounded, loopback-only OBS websocket transport.
use anyhow::{Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

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

pub async fn socket(endpoint: &str, authorization: Option<&str>, query: Option<&str>) -> Result<Socket> {
    let uri = loopback_url(endpoint)?;
    ensure!(uri.scheme_str() == Some("ws") && uri.host() == Some("127.0.0.1"), "OBS requires the declared ws://127.0.0.1 origin");
    socket_with_limit(endpoint, authorization, query, 512 * 1024).await
}

/// Bound both messages and frames before reading JSON.
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

