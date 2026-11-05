//! T3's one-time bootstrap exchange, pinned to the same protocol source as the summary client.
//! Only the resulting scoped bearer is persisted, in Windows Credential Manager.
use anyhow::{Result, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde_json::Value;

/// Runs on the Connect worker; no async runtime or listener remains after pairing.
pub fn t3_blocking(endpoint: &str, code: &str) -> Result<()> {
    save_exchange(endpoint, code, "orchestration:read")
}

fn save_exchange(endpoint: &str, code: &str, scope: &str) -> Result<()> {
    let mut token = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(exchange_scoped(endpoint, code, scope))?;
    let saved = std::str::from_utf8(&token)
        .map_err(anyhow::Error::from)
        .and_then(|token| crate::credentials::write("t3", token));
    token.fill(0);
    saved
}

/// Existing pairing is reused. New pairing uses T3's supported native one-time code;
/// its browser OAuth tokens are MCP-only and cannot authorize the live /ws stream.
pub fn connect_t3_blocking(endpoint: &str) -> Result<bool> {
    connect_t3(endpoint, true)
}

pub fn reauthorize_t3_blocking(endpoint: &str) -> Result<bool> {
    connect_t3(endpoint, false)
}

fn connect_t3(endpoint: &str, reuse: bool) -> Result<bool> {
    let endpoint = crate::t3_client::endpoint(endpoint);
    if reuse
        && crate::credentials::read("t3").is_ok()
        && crate::t3_client::check_blocking(endpoint).is_ok()
    {
        return Ok(true);
    }
    let uri = crate::transport::loopback_url(endpoint)?;
    let host = uri.host().unwrap_or("127.0.0.1").trim_matches(['[', ']']);
    let addresses: Vec<_> =
        std::net::ToSocketAddrs::to_socket_addrs(&(host, uri.port_u16().unwrap_or(80)))?.collect();
    ensure!(
        addresses
            .iter()
            .any(|address| std::net::TcpStream::connect_timeout(
                address,
                std::time::Duration::from_secs(2)
            )
            .is_ok()),
        "T3 Code is not running. Open it, then Connect again."
    );
    let Some(code) = crate::credentials::prompt_t3_pairing()? else {
        return Ok(false);
    };
    save_exchange(
        endpoint,
        code.text()?,
        "orchestration:read orchestration:operate diagnostics:read",
    )?;
    Ok(true)
}

struct Driver(tokio::task::JoinHandle<Result<(), hyper::Error>>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One bounded loopback request after a user explicitly supplies a pairing code. No renewals.
#[cfg(test)]
async fn exchange(endpoint: &str, code: &str) -> Result<Vec<u8>> {
    exchange_scoped(endpoint, code, "orchestration:read").await
}

async fn exchange_scoped(endpoint: &str, code: &str, requested_scope: &str) -> Result<Vec<u8>> {
    ensure!(
        !code.trim().is_empty() && code.len() <= 2560,
        "Enter a T3 pairing code"
    );
    let uri = crate::transport::loopback_url(endpoint)?;
    let host = uri.host().unwrap_or("127.0.0.1").trim_matches(['[', ']']);
    let authority = uri
        .authority()
        .ok_or_else(|| anyhow::anyhow!("T3 address is missing"))?
        .as_str();
    let mut body = format!("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange&subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap&requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token&scope={}&client_label=mypaper&client_device_type=desktop&client_os=Windows&subject_token={}", crate::launcher::encode(requested_scope), crate::launcher::encode(code.trim())).into_bytes();
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let stream = tokio::net::TcpStream::connect((host, uri.port_u16().unwrap_or(80))).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        let _driver = Driver(tokio::spawn(connection));
        async {
            let request = Request::builder()
                .method("POST")
                .uri("/oauth/token")
                .header("Host", authority)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Full::new(Bytes::copy_from_slice(&body)))?;
            let response = sender.send_request(request).await?;
            ensure!(
                response.status().is_success(),
                "T3 pairing was rejected (HTTP {})",
                response.status().as_u16()
            );
            let mut incoming = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = incoming.frame().await {
                if let Ok(data) = frame?.into_data() {
                    ensure!(
                        bytes.len() + data.len() <= 16 * 1024,
                        "T3 pairing response is too large"
                    );
                    bytes.extend_from_slice(&data);
                }
            }
            let parsed = serde_json::from_slice::<Value>(&bytes);
            bytes.fill(0);
            let mut response =
                parsed.map_err(|_| anyhow::anyhow!("T3 pairing response is invalid"))?;
            ensure!(
                response["token_type"] == "Bearer"
                    && response["scope"].as_str().is_some_and(|scope| {
                        let granted: std::collections::BTreeSet<_> =
                            scope.split_whitespace().collect();
                        let requested: std::collections::BTreeSet<_> =
                            requested_scope.split_whitespace().collect();
                        granted == requested
                    }),
                "T3 did not grant the requested access"
            );
            let Value::String(token) = response["access_token"].take() else {
                anyhow::bail!("T3 did not return an access token");
            };
            Ok(token.into_bytes())
        }
        .await
    })
    .await;
    body.fill(0);
    result.map_err(|_| anyhow::anyhow!("T3 pairing timed out"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn pairing_posts_a_narrow_bootstrap_exchange_without_persisting_fixture_tokens() {
        for scope in [
            "orchestration:read",
            "orchestration:read orchestration:operate",
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("ws://{}/ws", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(boundary) = request.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        let header = String::from_utf8_lossy(&request[..boundary]);
                        let length: usize = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if request.len() >= boundary + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("POST /oauth/token "));
                assert!(request.contains("scope=orchestration%3Aread"));
                assert!(request.contains("subject_token=fixture%20pairing%20code"));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                let response = serde_json::json!({"access_token":"fixture-only","scope":scope,"token_type":"Bearer"}).to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                )
                .unwrap();
            });
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(exchange(&endpoint, "fixture pairing code"));
            if scope == "orchestration:read" {
                assert_eq!(result.unwrap(), b"fixture-only");
            } else {
                assert!(result.is_err());
            }
            server.join().unwrap();
        }
    }
}
