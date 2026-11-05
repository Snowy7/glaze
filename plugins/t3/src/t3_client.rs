//! Small T3 RPC requests. Native pairing and MCP authorization are different audiences.
use serde::Deserialize;


#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Metadata {
    pub environment: Environment,
    pub providers: Vec<ProviderModels>,
    pub usage_limit_sources: Vec<LimitSource>,
    pub shell_older_settled: bool,
    pub shell_thread_families: bool,
}
/// Stream-only accounts, including accounts without an installed provider instance.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct LimitSource {
    pub id: String,
    pub accounts: Vec<LimitAccount>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimitAccount {
    pub id: String,
    pub driver: String,
    pub email: Option<String>,
    pub usage_limits: UsageLimits,
}
#[derive(Deserialize)]
pub(crate) struct ConfigEvent {
    #[serde(rename = "type")]
    kind: String,
    config: Option<Metadata>,
    #[serde(default)]
    payload: ConfigUpdate,
}
// A direct typed decode skips settings/skills instead of buffering an internally tagged enum.
#[derive(Default, Deserialize)]
struct ConfigUpdate {
    providers: Option<Vec<ProviderModels>>,
    sources: Option<Vec<LimitSource>>,
}
impl Metadata {
    /// Config snapshots omit sources; only a source event replaces their current set.
    pub(crate) fn apply(&mut self, event: ConfigEvent) -> bool {
        match (event.kind.as_str(), event.config, event.payload) {
            ("snapshot", Some(mut config), _) => {
                config.usage_limit_sources = std::mem::take(&mut self.usage_limit_sources);
                *self = config;
            }
            (
                "providerStatuses",
                _,
                ConfigUpdate {
                    providers: Some(providers),
                    ..
                },
            ) => self.providers = providers,
            (
                "usageLimitSourcesUpdated",
                _,
                ConfigUpdate {
                    sources: Some(sources),
                    ..
                },
            ) => self.usage_limit_sources = sources,
            _ => return false,
        }
        true
    }
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Environment {
    pub environment_id: String,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderModels {
    pub instance_id: String,
    pub driver: String,
    pub display_name: String,
    pub auth: Auth,
    pub usage_limits: Option<UsageLimits>,
    pub installed: bool,
    pub enabled: bool,
    pub models: Vec<ProviderModel>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Auth {
    pub email: Option<String>,
    pub label: Option<String>,
}
/// A provider account's rate-limit windows, as T3's own limits page reads them.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageLimits {
    pub windows: Vec<LimitWindow>,
}
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimitWindow {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_duration_mins: Option<u64>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct ProviderModel {
    pub slug: String,
    pub name: String,
}
