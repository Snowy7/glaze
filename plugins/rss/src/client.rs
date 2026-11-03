//! The real [`Fetcher`]: feeds over HTTP(S) with ureq.

use std::time::Duration;

use crate::service::Fetcher;

/// Largest feed document we read; bigger responses are an error.
const MAX_BODY: u64 = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT: &str =
    "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.8, */*;q=0.5";

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .user_agent("mypaper-rss/1.0 (feed reader plugin for the mypaper wallpaper app)")
            .build()
            .into();
        Self { agent }
    }
}

impl Fetcher for Http {
    fn fetch(&self, url: &str) -> Result<String, String> {
        let mut response = self
            .agent
            .get(url)
            .header("Accept", ACCEPT)
            .call()
            .map_err(|e| format!("cannot fetch feed: {e}"))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| format!("cannot read feed: {e}"))?;
        // Feeds that declare another encoding are decoded as UTF-8 with replacements: titles in
        // ASCII are unaffected and the rest degrades instead of failing.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
