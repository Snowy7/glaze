//! Pure webhook validation and bounded request receipts. I/O belongs to the host broker.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineRun {
    pub id: String,
    pub title: String,
    pub state: RoutineState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutineState {
    Sending,
    Accepted,
    Failed,
}

pub fn routine_url(value: &str) -> Result<http::Uri, String> {
    let uri: http::Uri = value.parse().map_err(|_| "Use an HTTPS routine webhook")?;
    if uri.scheme_str() != Some("https") || uri.host().is_none() {
        return Err("Routine webhook must use HTTPS".into());
    }
    if uri.authority().is_none_or(|a| a.as_str().contains('@'))
        || uri.query().is_some()
        || value.contains('#')
    {
        return Err("Webhook credentials belong in the secrets broker, not the URL".into());
    }
    Ok(uri)
}

pub fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn prompt_target(prompt: &str) -> Result<String, String> {
    let prompt = prompt.trim();
    let target = format!("grokbot://app/v1/prompt?text={}", encode(prompt));
    if prompt.is_empty() || prompt.chars().count() > 1000 || target.len() > 2048 {
        return Err("Use a shorter prompt (up to 1,000 characters).".into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drafts_are_encoded_bounded_and_never_submitted() {
        assert_eq!(
            prompt_target(" hello & world ").unwrap(),
            "grokbot://app/v1/prompt?text=hello%20%26%20world"
        );
        assert!(prompt_target(" ").is_err());
        assert!(prompt_target(&"a".repeat(1001)).is_err());
        assert!(prompt_target(&"é".repeat(500)).is_err());
    }
    #[test]
    fn webhooks_do_not_embed_credentials_or_accept_insecure_urls() {
        for bad in [
            "http://example.test/run",
            "https://key@example.test/run",
            "https://example.test/run?key=secret",
            "https://example.test/run#secret",
        ] {
            assert!(routine_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            routine_url("https://example.test:9443/run")
                .unwrap()
                .authority()
                .unwrap()
                .as_str(),
            "example.test:9443"
        );
    }
}
