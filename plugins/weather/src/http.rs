//! Blocking WinHTTP, used only on worker threads. Windows owns TLS, proxy and certificate policy.
use std::ffi::c_void;
use std::time::Duration;

use serde::de::DeserializeOwned;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{HSTRING, PCWSTR, w};

const MAX_RESPONSE: usize = 4 << 20;

struct Handle(*mut c_void);
impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, String> {
        if raw.is_null() {
            Err(windows::core::Error::from_thread().to_string())
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: each successful WinHTTP open is owned here and closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

pub struct Http {
    timeout: i32,
}
impl Http {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout: timeout.as_millis().min(i32::MAX as u128) as i32,
        }
    }
    pub fn get(&self, url: &str) -> Request {
        Request {
            url: url.into(),
            timeout: self.timeout,
        }
    }
}

pub struct Request {
    url: String,
    timeout: i32,
}
impl Request {
    pub fn query(mut self, key: &str, value: impl AsRef<str>) -> Self {
        self.url
            .push(if self.url.contains('?') { '&' } else { '?' });
        self.url.push_str(&encode(key));
        self.url.push('=');
        self.url.push_str(&encode(value.as_ref()));
        self
    }
    pub fn json<T: DeserializeOwned>(self) -> Result<T, String> {
        serde_json::from_slice(&self.bytes()?).map_err(|e| e.to_string())
    }
    pub fn bytes(self) -> Result<Vec<u8>, String> {
        let (secure, rest) = if let Some(rest) = self.url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = self.url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err("expected an http(s) URL".into());
        };
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".into()), |(h, p)| (h, format!("/{p}")));
        if authority.contains(['@', '#', '?', '\\']) || authority.is_empty() {
            return Err("invalid host".into());
        }
        let (host, port) = authority.rsplit_once(':').map_or(
            Ok((authority, if secure { 443 } else { 80 })),
            |(h, p)| {
                p.parse::<u16>()
                    .map(|p| (h, p))
                    .map_err(|_| "invalid port".to_owned())
            },
        )?;
        // SAFETY: handles are scoped RAII values, all strings live across their call, reads stay
        // within `chunk`, and the bounded output cannot allocate an unbounded server response.
        unsafe {
            let session = Handle::new(WinHttpOpen(
                w!("mypaper-native/1.0"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ))?;
            WinHttpSetTimeouts(
                session.0,
                self.timeout,
                self.timeout,
                self.timeout,
                self.timeout,
            )
            .map_err(|e| e.to_string())?;
            let connection = Handle::new(WinHttpConnect(session.0, &HSTRING::from(host), port, 0))?;
            let request = Handle::new(WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                if secure {
                    WINHTTP_FLAG_SECURE
                } else {
                    WINHTTP_OPEN_REQUEST_FLAGS(0)
                },
            ))?;
            WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| e.to_string())?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
            let mut status = 0u32;
            let mut length = 4;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&raw mut status).cast()),
                &mut length,
                std::ptr::null_mut(),
            )
            .map_err(|e| e.to_string())?;
            if !(200..300).contains(&status) {
                return Err(format!("HTTP {status}"));
            }
            let mut output = Vec::new();
            let mut chunk = [0u8; 16384];
            loop {
                let mut count = 0;
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut count,
                )
                .map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                if output.len() + count as usize > MAX_RESPONSE {
                    return Err("response exceeds 4 MB".into());
                }
                output.extend_from_slice(&chunk[..count as usize]);
            }
            Ok(output)
        }
    }
}

pub fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf8_query_is_encoded_bytewise() {
        assert_eq!(super::encode("a & 東京"), "a%20%26%20%E6%9D%B1%E4%BA%AC");
    }
}
