//! Studio's persisted contract. Credential values never belong in this file.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Config {
    pub notifications: Notifications,
    pub media: Switch,
    pub launcher: Switch,
    pub t3: T3,
    pub obs: Obs,
    pub docker: Docker,
    pub tailscale: Tailscale,
    pub grok: Grok,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            notifications: Notifications::default(),
            media: Switch { enabled: true },
            launcher: Switch { enabled: true },
            t3: T3::default(),
            obs: Obs::default(),
            docker: Docker::default(),
            tailscale: Tailscale::default(),
            grok: Grok::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Switch {
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Notifications {
    pub enabled: bool,
    pub show_text: bool,
    /// Only set after the packaged listener has passed foreground/background delivery checks.
    pub live_verified: bool,
    /// Executable/pin identity -> Windows AUMID, for apps that do not publish window AUMIDs.
    pub app_ids: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct T3 {
    pub enabled: bool,
    pub endpoint: String,
    pub protocol_version: u32,
    pub live_verified: bool,
    /// Copied hosted links; the client never invents an OS URI or reads T3's credential files.
    pub thread_links: BTreeMap<String, String>,
}
impl Default for T3 {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            protocol_version: 2,
            live_verified: false,
            thread_links: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Obs {
    pub enabled: bool,
    pub endpoint: String,
}
impl Default for Obs {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "ws://127.0.0.1:4455".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Docker {
    pub enabled: bool,
    /// Explicit local context endpoint. Never guess a context or start Desktop/WSL.
    pub pipe: String,
    pub containers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Tailscale {
    pub enabled: bool,
    pub pipe: String,
    pub live_verified: bool,
}
impl Default for Tailscale {
    fn default() -> Self {
        Self {
            enabled: false,
            pipe: r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled".into(),
            live_verified: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Grok {
    pub enabled: bool,
    pub routines: Vec<Routine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        ensure!(
            text.len() <= 256 * 1024,
            "integrations.json exceeds 256 KiB"
        );
        let config: Self = serde_json::from_str(text)?;
        for endpoint in [&config.t3.endpoint, &config.obs.endpoint]
            .into_iter()
            .filter(|s| !s.is_empty())
        {
            super::transport::loopback_url(endpoint)?;
        }
        for pipe in [&config.docker.pipe, &config.tailscale.pipe]
            .into_iter()
            .filter(|s| !s.is_empty())
        {
            ensure!(
                pipe.starts_with(r"\\.\pipe\") && !pipe.contains(['\r', '\n']),
                "expected a local named pipe"
            );
        }
        ensure!(config.grok.routines.len() <= 32, "at most 32 Grok routines");
        let mut ids = std::collections::BTreeSet::new();
        for routine in &config.grok.routines {
            ensure!(
                !routine.id.is_empty()
                    && routine.id.len() <= 64
                    && routine
                        .id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)),
                "invalid routine id"
            );
            ensure!(ids.insert(&routine.id), "duplicate routine id");
            super::launcher::routine_url(&routine.url)?;
        }
        for link in config.t3.thread_links.values() {
            ensure!(
                link.starts_with("https://") && !link.contains(['\r', '\n']),
                "thread links must be copied HTTPS links"
            );
        }
        Ok(config)
    }
}
