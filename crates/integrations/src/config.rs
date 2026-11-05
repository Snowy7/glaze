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
    /// Accepted and discarded: T3 settings and access now belong to the component plugin.
    #[serde(rename = "t3", skip_serializing, deserialize_with = "ignore_legacy_t3")]
    pub legacy_t3: (),
    pub obs: Obs,
    /// Legacy read contract only; Docker now runs as the bundled component.
    pub docker: Docker,
    pub tailscale: Tailscale,
    pub grok: Grok,
    pub calendar: Calendar,
    pub discord: Discord,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            notifications: Notifications::default(),
            media: Switch { enabled: true },
            launcher: Switch { enabled: true },
            legacy_t3: (),
            obs: Obs::default(),
            docker: Docker::default(),
            tailscale: Tailscale::default(),
            grok: Grok::default(),
            calendar: Calendar::default(),
            discord: Discord::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Switch {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Calendar {
    pub enabled: bool,
    pub windows: bool,
    pub google_client_id: String,
    pub microsoft_client_id: String,
}
impl Default for Calendar {
    fn default() -> Self {
        Self { enabled: true, windows: true, google_client_id: option_env!("MYPAPER_GOOGLE_CLIENT_ID").unwrap_or("").into(), microsoft_client_id: option_env!("MYPAPER_MICROSOFT_CLIENT_ID").unwrap_or("").into() }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Discord {
    pub enabled: bool,
    pub client_id: String,
    /// Only one explicitly configured local pipe; no discovery scan.
    pub pipe: String,
    pub rpc: bool,
    /// Request write scope only when the user explicitly enables voice controls.
    pub voice_controls: bool,
    pub channel_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Notifications {
    pub enabled: bool,
    pub show_text: bool,
    /// Discord's Connect click opts into previews for recognized Discord identities only.
    pub discord_previews: bool,
    /// Only set after the packaged listener has passed foreground/background delivery checks.
    pub live_verified: bool,
    /// Executable/pin identity -> Windows AUMID, for apps that do not publish window AUMIDs.
    pub app_ids: BTreeMap<String, String>,
}

fn ignore_legacy_t3<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    <serde::de::IgnoredAny as Deserialize>::deserialize(deserializer).map(|_| ())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
// Retained to read existing integrations.json files. OBS runs only in its component plugin.
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
/// Read compatibility for integrations.json predating the component migration. No native provider uses it.
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
        ensure!(config.discord.channel_ids.len() <= 8, "Choose at most eight Discord channels");
        for id in config.discord.channel_ids.iter().chain(std::iter::once(&config.discord.client_id)).filter(|s| !s.is_empty()) {
            ensure!(id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()), "Discord IDs must be numeric snowflakes");
        }
        ensure!(config.discord.pipe.is_empty() || config.discord.pipe.strip_prefix(r"\\.\pipe\discord-ipc-").is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit()), "Expected a local Discord IPC pipe");
        for endpoint in [&config.obs.endpoint]
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
            let uri: hyper::Uri = routine.url.parse()?;
            ensure!(uri.scheme_str() == Some("https") && uri.host().is_some(), "routine webhook must use HTTPS");
            ensure!(uri.authority().is_some_and(|a| !a.as_str().contains('@')) && uri.query().is_none() && !routine.url.contains('#'), "webhook credentials belong in Credential Manager, not the URL");
        }
        Ok(config)
    }
}
