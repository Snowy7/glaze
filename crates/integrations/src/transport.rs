//! Validation for legacy local integration endpoints.
use anyhow::{Result, ensure};

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

