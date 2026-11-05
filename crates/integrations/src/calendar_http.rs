//! Bounded native HTTPS for calendar OAuth and Discord bots. No redirects or hidden retries.
use anyhow::{Result, ensure};
use windows::{Win32::Networking::WinHttp::*, core::{PCWSTR, w}};

struct Handle(*mut std::ffi::c_void);
impl Handle {
    fn new(raw: *mut std::ffi::c_void) -> Result<Self> {
        ensure!(!raw.is_null(), "Could not open HTTPS connection");
        Ok(Self(raw))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this is an owned WinHTTP handle, closed exactly once.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}
fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }

/// Call on a worker. HTTP errors expose only the status, never a token response or request URL.
pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8], limit: usize) -> Result<Vec<u8>> {
    let uri: hyper::Uri = url.parse()?;
    ensure!(uri.scheme_str() == Some("https") && uri.host().is_some(), "Expected HTTPS URL");
    ensure!(!uri.authority().unwrap().as_str().contains('@') && !url.contains(['\r', '\n', '#']), "Invalid HTTPS URL");
    ensure!(body.len() <= 64 * 1024, "Request exceeds 64 KiB");
    let host = wide(uri.host().unwrap());
    let path = wide(uri.path_and_query().map_or("/", |p| p.as_str()));
    let method = wide(method);
    let mut header = String::new();
    for (key, value) in headers {
        ensure!(!key.contains(['\r', '\n', ':']) && !value.contains(['\r', '\n']), "Invalid HTTP header");
        header.push_str(&format!("{key}: {value}\r\n"));
    }
    let mut header = wide(&header);
    // SAFETY: all UTF-16 and byte buffers remain live for these synchronous WinHTTP calls.
    let result = unsafe { (|| -> Result<Vec<u8>> {
        let session = Handle::new(WinHttpOpen(w!("mypaper/1"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, None, None, 0))?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000)?;
        let connection = Handle::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), uri.port_u16().unwrap_or(443), 0))?;
        let request = Handle::new(WinHttpOpenRequest(connection.0, PCWSTR(method.as_ptr()), PCWSTR(path.as_ptr()), None, None, std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes()))?;
        WinHttpSendRequest(request.0, Some(&header[..header.len()-1]), if body.is_empty() { None } else { Some(body.as_ptr().cast()) }, body.len() as u32, body.len() as u32, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
        let (mut status, mut size) = (0_u32, 4_u32);
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, None, Some((&mut status as *mut u32).cast()), &mut size, std::ptr::null_mut())?;
        ensure!((200..300).contains(&status), "Service returned HTTP {status}; reconnect or check permissions (429 means rate limited)");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            let mut read = 0;
            WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read)?;
            if read == 0 { break; }
            ensure!(bytes.len() + read as usize <= limit, "Response exceeds size limit");
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        Ok(bytes)
    })() };
    header.fill(0);
    result
}

pub fn json(url: &str, bearer: &str) -> Result<serde_json::Value> {
    let mut auth = format!("Bearer {bearer}");
    let result = request("GET", url, &[("Authorization", &auth)], &[], 2 * 1024 * 1024)
        .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?));
    // Do not retain the duplicate authorization string.
    // SAFETY: zeroing bytes preserves valid UTF-8.
    unsafe { auth.as_bytes_mut().fill(0) };
    result
}

pub fn encode(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
pub fn form(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(k,v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}
