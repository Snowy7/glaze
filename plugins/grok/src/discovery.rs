//! The public desktop marker is read by the launch broker, never by the guest's filesystem.
use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    pub installed: bool,
    pub running: bool,
    pub signed_in: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker { version: u32, signed_in: bool }

impl Discovery {
    pub fn from_marker(installed: bool, running: bool, marker: Option<&str>) -> Self {
        let signed_in = running && marker.and_then(|s| serde_json::from_str::<Marker>(s).ok()).is_some_and(|m| m.version == 1 && m.signed_in);
        Self { installed: installed || running, running, signed_in }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_or_unsupported_marker_cannot_claim_a_sign_in() {
        let marker = r#"{"version":1,"signedIn":true}"#;
        assert!(Discovery::from_marker(true, true, Some(marker)).signed_in);
        assert!(!Discovery::from_marker(true, false, Some(marker)).signed_in);
        assert!(!Discovery::from_marker(true, true, Some(r#"{"version":2,"signedIn":true}"#)).signed_in);
        assert!(!Discovery::from_marker(true, true, Some("garbage")).signed_in);
    }
}
