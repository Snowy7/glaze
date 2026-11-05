//! Desktop PKCE sign-in using the system browser, an ephemeral loopback listener and Credential Manager.
use super::{calendar_http as http, credentials};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Google,
    Microsoft,
}
impl Service {
    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "calendar/google",
            Self::Microsoft => "calendar/microsoft",
        }
    }
    fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }
    fn authorize_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }
    fn scope(self) -> &'static str {
        match self {
            Self::Google => "https://www.googleapis.com/auth/calendar.readonly",
            Self::Microsoft => "offline_access https://graph.microsoft.com/Calendars.Read",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
    expires: i64,
    client_id: String,
}
impl Drop for Tokens {
    fn drop(&mut self) {
        // SAFETY: replacing UTF-8 bytes by zero preserves UTF-8 validity.
        unsafe {
            self.access.as_bytes_mut().fill(0);
            self.refresh.as_bytes_mut().fill(0);
        }
    }
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default = "hour")]
    expires_in: i64,
}
fn hour() -> i64 {
    3600
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn random() -> Result<String> {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let mut bytes = [0_u8; 32];
    // SAFETY: fills our own 32-byte buffer using the Windows system CSPRNG.
    unsafe {
        BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok()?;
    }
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Large Graph tokens are split across generic credentials, whose individual blob limit is 2560 bytes.
fn save(service: Service, tokens: &Tokens) -> Result<()> {
    let mut value = serde_json::to_string(tokens)?;
    let bytes = value.as_bytes();
    ensure!(
        bytes.len() <= 32 * 1024,
        "OAuth token exceeds Credential Manager budget"
    );
    // Token JSON is ASCII in practice; split at char boundaries regardless.
    let mut chunks = Vec::new();
    let mut begin = 0;
    while begin < value.len() {
        let mut end = (begin + 2400).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&value[begin..end]);
        begin = end;
    }
    for (i, part) in chunks.iter().enumerate() {
        credentials::write(&format!("{}/token-{i}", service.key()), part)?;
    }
    credentials::write(
        &format!("{}/parts", service.key()),
        &chunks.len().to_string(),
    )?;
    // SAFETY: zeroing the serialized token preserves UTF-8.
    unsafe {
        value.as_bytes_mut().fill(0);
    }
    Ok(())
}
fn load(service: Service) -> Result<Tokens> {
    let count = credentials::read(&format!("{}/parts", service.key()))?
        .text()?
        .parse::<usize>()?;
    ensure!((1..=16).contains(&count), "Invalid stored calendar token");
    let mut value = String::new();
    for i in 0..count {
        value.push_str(credentials::read(&format!("{}/token-{i}", service.key()))?.text()?);
    }
    let parsed = serde_json::from_str(&value);
    // SAFETY: zeroing this serialized token preserves UTF-8.
    unsafe {
        value.as_bytes_mut().fill(0);
    }
    Ok(parsed?)
}
pub fn connected(service: Service) -> bool {
    credentials::read(&format!("{}/parts", service.key())).is_ok()
}
pub fn disconnect(service: Service) -> Result<()> {
    for i in 0..16 {
        let _ = credentials::remove(&format!("{}/token-{i}", service.key()));
    }
    if connected(service) {
        credentials::remove(&format!("{}/parts", service.key()))?;
    }
    Ok(())
}

fn exchange(service: Service, fields: Vec<(&str, &str)>) -> Result<TokenResponse> {
    let secret = if service == Service::Google {
        credentials::read("calendar/google/client-secret").ok()
    } else {
        None
    };
    let mut body = http::form(&fields);
    if let Some(secret) = &secret {
        body.push_str(&format!("&client_secret={}", http::encode(secret.text()?)));
    }
    let result = http::request(
        "POST",
        service.token_url(),
        &[("Content-Type", "application/x-www-form-urlencoded")],
        body.as_bytes(),
        64 * 1024,
    )
    .and_then(|mut bytes| {
        let parsed = serde_json::from_slice(&bytes);
        bytes.fill(0);
        Ok(parsed?)
    });
    // SAFETY: zeroing form bytes preserves UTF-8.
    unsafe {
        body.as_bytes_mut().fill(0);
    }
    result
}

/// Refresh only while a calendar consumer explicitly loads data; no token-renewal timer.
pub fn access_token(service: Service, client_id: &str) -> Result<String> {
    let mut tokens = load(service).context("Calendar is not connected; choose Connect")?;
    ensure!(
        tokens.client_id == client_id,
        "Calendar client ID changed; reconnect"
    );
    if tokens.expires <= now() + 60 {
        ensure!(
            !tokens.refresh.is_empty(),
            "Calendar session expired; reconnect"
        );
        let response = exchange(
            service,
            vec![
                ("client_id", client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", &tokens.refresh),
            ],
        )?;
        tokens.access = response.access_token;
        if !response.refresh_token.is_empty() {
            tokens.refresh = response.refresh_token;
        }
        tokens.expires = now() + response.expires_in;
        save(service, &tokens)?;
    }
    Ok(tokens.access.clone())
}

fn decode(value: &str) -> Option<String> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                out.push(
                    u8::from_str_radix(std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?, 16)
                        .ok()?,
                );
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}
fn callback(request: &str, state: &str) -> Result<Option<String>> {
    let target = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.split_once(' '))
        .map(|(target, _)| target)
        .context("Invalid OAuth callback")?;
    if !target.starts_with("/oauth/callback?") {
        return Ok(None);
    }
    let query: std::collections::BTreeMap<_, _> = target
        .split_once('?')
        .unwrap()
        .1
        .split('&')
        .filter_map(|p| p.split_once('='))
        .filter_map(|(k, v)| Some((decode(k)?, decode(v)?)))
        .collect();
    ensure!(
        query.get("state").is_some_and(|v| v == state),
        "OAuth state did not match; sign-in cancelled"
    );
    if query.contains_key("error") {
        bail!("Calendar sign-in was declined or unavailable");
    }
    let code = query
        .get("code")
        .filter(|s| !s.is_empty())
        .context("OAuth callback has no code")?;
    Ok(Some(code.clone()))
}
fn respond(stream: &mut TcpStream, success: bool) {
    let body = if success {
        "Sign-in received. Return to mypaper to check the connection. You can close this tab."
    } else {
        "This sign-in callback was not accepted. Return to mypaper."
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        if success { "200 OK" } else { "400 Bad Request" },
        body.len(),
        body
    );
}

/// Explicit Connect click only. Microsoft must register http://localhost:53683/oauth/callback.
pub fn connect(service: Service, client_id: &str) -> Result<()> {
    ensure!(
        !client_id.trim().is_empty(),
        "Register a desktop OAuth client and set its client ID in integrations.json"
    );
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        if service == Service::Microsoft {
            53683
        } else {
            0
        },
    ))
    .context("Could not bind calendar sign-in loopback port")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let redirect = format!(
        "http://{}:{port}/oauth/callback",
        if service == Service::Google {
            "127.0.0.1"
        } else {
            "localhost"
        }
    );
    let state = random()?;
    let verifier = random()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut fields = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", &redirect),
        ("scope", service.scope()),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ];
    if service == Service::Google {
        fields.extend([("access_type", "offline"), ("prompt", "consent")]);
    }
    super::launcher::open(&super::launcher::Recent {
        title: String::new(),
        target: format!("{}?{}", service.authorize_url(), http::form(&fields)),
        arguments: String::new(),
    })?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let code = loop {
        ensure!(
            Instant::now() < deadline,
            "Calendar sign-in timed out; choose Connect to try again"
        );
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut bytes = [0_u8; 8192];
                let read = stream.read(&mut bytes).unwrap_or(0);
                match callback(std::str::from_utf8(&bytes[..read]).unwrap_or(""), &state) {
                    Ok(Some(code)) => {
                        respond(&mut stream, true);
                        break code;
                    }
                    Err(error)
                        if error.to_string() == "Calendar sign-in was declined or unavailable" =>
                    {
                        respond(&mut stream, false);
                        return Err(error);
                    }
                    Ok(None) | Err(_) => {
                        respond(&mut stream, false);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(75))
            }
            Err(e) => return Err(e.into()),
        }
    };
    let response = exchange(
        service,
        vec![
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("code_verifier", &verifier),
        ],
    )?;
    let previous_refresh = load(service)
        .ok()
        .map(|t| t.refresh.clone())
        .unwrap_or_default();
    save(
        service,
        &Tokens {
            access: response.access_token,
            refresh: if response.refresh_token.is_empty() {
                previous_refresh
            } else {
                response.refresh_token
            },
            expires: now() + response.expires_in,
            client_id: client_id.into(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_rejects_forgery_and_decodes_only_matching_path() {
        assert!(
            callback(
                "GET /oauth/callback?state=evil&code=secret HTTP/1.1",
                "right"
            )
            .is_err()
        );
        assert_eq!(
            callback("GET /favicon.ico HTTP/1.1", "right").unwrap(),
            None
        );
        assert_eq!(
            callback(
                "GET /oauth/callback?state=right&code=a%2Bb HTTP/1.1",
                "right"
            )
            .unwrap()
            .as_deref(),
            Some("a+b")
        );
        assert!(
            callback(
                "GET /oauth/callback?state=right&error=access_denied HTTP/1.1",
                "right"
            )
            .is_err()
        );
    }
}
