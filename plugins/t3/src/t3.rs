//! Effect RPC v2 shell summaries, with bounded metadata and no client ping task.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// These wire structs deliberately omit preview bodies, repository identities and scripts.
// Serde skips them on the socket buffer instead of allocating a full JSON tree for every wake.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellEnvelope {
    #[serde(rename = "_tag")]
    pub(crate) tag: String,
    pub(crate) request_id: Option<String>,
    #[serde(default)]
    // Route by request id before decoding. An untagged enum buffers even discarded previews.
    pub(crate) values: Vec<Box<serde_json::value::RawValue>>,
    /// A finished request's `{"_tag":"Success"|"Failure","value":…}`.
    #[serde(default)]
    pub(crate) exit: Option<Box<serde_json::value::RawValue>>,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ShellItem {
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<ShellBaseline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_repository_identity_roots: Option<Vec<String>>,
    sequence: Option<u64>,
    location: Option<String>,
    thread: Option<ShellThread>,
    thread_id: Option<String>,
    project: Option<ShellProject>,
    project_id: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellBaseline {
    schema_version: u64,
    snapshot_sequence: u64,
    threads: Vec<ShellThread>,
    projects: Vec<ShellProject>,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct ShellProject {
    id: String,
    #[serde(alias = "name")]
    title: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellThread {
    id: String,
    project_id: String,
    title: String,
    provider_instance_id: String,
    status: Status,
    latest_run_id: Option<String>,
    active_run_id: Option<String>,
    pending_runtime_request: Option<Request>,
    updated_at: String,
    #[serde(default)]
    model_selection: ShellModel,
    #[serde(default)]
    lineage: ShellLineage,
    // Only the count is retained in Snapshot. These records are bounded by the wire limit.
    #[serde(default)]
    pending_background_tasks: Vec<ShellTask>,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ShellTask {}
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ShellModel {
    model: String,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct ShellLineage {
    parent_thread_id: Option<String>,
    relationship_to_parent: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider_instance_id: String,
    pub status: Status,
    pub latest_run_id: Option<String>,
    pub active_run_id: Option<String>,
    pub pending_runtime_request: Option<Request>,
    pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Request {
    pub id: String,
    pub kind: String,
}
impl Request {
    pub fn approvable(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "command" | "file-read" | "file-change" | "mcp-elicitation" | "permission"
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Idle,
    Queued,
    Preparing,
    Starting,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    #[serde(rename = "rolled_back")]
    RolledBack,
}
impl Thread {
    pub fn attention(&self) -> bool {
        self.pending_runtime_request.is_some()
    }
    pub fn running(&self) -> bool {
        matches!(
            self.status,
            Status::Queued
                | Status::Preparing
                | Status::Starting
                | Status::Running
                | Status::Waiting
        )
    }
    pub fn finished(&self) -> bool {
        matches!(
            self.status,
            Status::Completed
                | Status::Failed
                | Status::Cancelled
                | Status::Interrupted
                | Status::RolledBack
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub sequence: u64,
    pub threads: Vec<Thread>,
    pub projects: BTreeMap<String, String>,
    pub transitions: Vec<Transition>,
    pub details: BTreeMap<String, ThreadDetail>,
    pub models: Vec<ModelChoice>,
    pub environment_id: Option<String>,
    pub permissions: Vec<String>,
    /// The last completed usage read. T3 exposes reads, rather than usage change events.
    pub usage: Option<Vec<Usage>>,
    pub usage_notice: Option<String>,
    pub usage_day: Option<String>,
    pub omit_older_settled: bool,
    pub omit_idle_subagents: bool,
    /// One row per provider account and limit window, from the live config stream.
    pub limits: Vec<Limit>,
    /// The last 30 UTC days of `server.getUsageSummary`, oldest first.
    pub history: Option<History>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Limit {
    pub driver: String,
    pub provider: String,
    pub instance: String,
    pub account: String,
    pub window: String,
    /// Used, in tenths of a percent.
    pub used_permille: u32,
    pub resets_at: Option<String>,
    pub duration_mins: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub days: Vec<String>,
    /// Per provider: cost (micro-USD) and processed tokens for each day in `days`.
    pub providers: Vec<ProviderHistory>,
    pub uncached: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
    pub savings_microusd: u64,
    /// Input, cache read, cache write, output, in micro-USD.
    pub cost_by_type: [u64; 4],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderHistory {
    pub provider: String,
    pub cost: Vec<u64>,
    pub tokens: Vec<u64>,
    /// Distinct sessions when available; otherwise summed daily/model bucket counts.
    pub sessions: u64,
    pub sessions_distinct: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadDetail {
    pub model: String,
    pub parent_id: Option<String>,
    pub agent: bool,
    pub background_tasks: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    pub provider: String,
    pub provider_name: String,
    pub model: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub provider: String,
    pub model: String,
    pub tokens: u64,
    pub cost_microusd: u64,
    pub unpriced: bool,
}
impl Snapshot {
    pub fn can_operate(&self) -> bool {
        self.permissions
            .iter()
            .any(|p| p == "orchestration:operate")
    }
    /// Installed T3 registers t3code:// and accepts environment/thread prompt links.
    pub fn thread_link(&self, thread_id: &str) -> Option<String> {
        self.environment_id.as_ref().map(|environment| {
            format!(
                "t3code://threads/{}/{}",
                crate::launcher::encode(environment),
                crate::launcher::encode(thread_id)
            )
        })
    }
    fn detail(&mut self, thread: &Value) {
        if let Some(id) = thread["id"].as_str() {
            self.details.insert(
                id.into(),
                ThreadDetail {
                    model: thread["modelSelection"]["model"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(128)
                        .collect(),
                    parent_id: thread["lineage"]["parentThreadId"]
                        .as_str()
                        .map(str::to_owned),
                    agent: thread["lineage"]["relationshipToParent"] == "subagent",
                    background_tasks: thread["pendingBackgroundTasks"]
                        .as_array()
                        .map_or(0, Vec::len),
                },
            );
        }
    }
    pub(crate) fn configure(&mut self, value: &crate::t3_client::Metadata) {
        self.omit_older_settled = value.shell_older_settled;
        self.omit_idle_subagents = value.shell_thread_families;
        self.environment_id =
            Some(value.environment.environment_id.clone()).filter(|s| !s.is_empty());
        self.models.clear();
        self.limits.clear();
        for provider in value.providers.iter().filter(|p| p.enabled && p.installed) {
            let account = provider
                .auth
                .label
                .clone()
                .or_else(|| provider.auth.email.clone())
                .unwrap_or_else(|| provider.display_name.clone());
            for window in provider
                .usage_limits
                .iter()
                .flat_map(|u| u.windows.iter())
                .take(16)
            {
                self.limits.push(Limit {
                    driver: provider.driver.clone(),
                    provider: provider.display_name.clone(),
                    instance: provider.instance_id.clone(),
                    account: account.chars().take(128).collect(),
                    window: window.label.chars().take(64).collect(),
                    used_permille: (window.used_percent.clamp(0.0, 100.0) * 10.0).round() as u32,
                    resets_at: window.resets_at.clone(),
                    duration_mins: window.window_duration_mins,
                });
            }
            for model in provider.models.iter().take(256) {
                if !model.slug.is_empty() {
                    self.models.push(ModelChoice {
                        provider: provider.instance_id.clone(),
                        provider_name: provider.display_name.clone(),
                        model: model.slug.clone(),
                        name: model.name.clone(),
                    });
                }
            }
        }
        for source in &value.usage_limit_sources {
            for account in &source.accounts {
                let provider = self
                    .limits
                    .iter()
                    .find(|l| l.driver == account.driver)
                    .map(|l| l.provider.clone())
                    .unwrap_or_else(|| match account.driver.as_str() {
                        "claudeAgent" | "claude" => "Claude".into(),
                        "codex" => "Codex".into(),
                        driver => driver.into(),
                    });
                for window in &account.usage_limits.windows {
                    self.limits.push(Limit {
                        driver: account.driver.clone(),
                        provider: provider.clone(),
                        instance: format!("{}/{}", source.id, account.id),
                        account: account
                            .email
                            .as_deref()
                            .unwrap_or(&account.id)
                            .chars()
                            .take(128)
                            .collect(),
                        window: window.label.chars().take(64).collect(),
                        used_permille: (window.used_percent.clamp(0.0, 100.0) * 10.0).round()
                            as u32,
                        resets_at: window.resets_at.clone(),
                        duration_mins: window.window_duration_mins,
                    });
                }
            }
        }
    }
    /// The 30-day history, and today's per-model totals for the existing usage face.
    pub(crate) fn history(&mut self, value: &Value, days: Vec<String>, today: &str) {
        let mut history = History {
            days,
            ..Default::default()
        };
        let mut today_buckets = Vec::new();
        for bucket in value["buckets"].as_array().into_iter().flatten() {
            let day = ["day", "date", "bucketDay", "startDay"]
                .iter()
                .find_map(|k| bucket[*k].as_str())
                .unwrap_or(today);
            let t = &bucket["totals"];
            let n = |k: &str| t[k].as_u64().unwrap_or(0);
            let tokens = n("uncachedInputTokens")
                + n("cachedInputTokens")
                + n("cacheCreationTokens")
                + n("outputTokens");
            let micro = |v: &Value| {
                v.as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .map_or(0, |n| (n * 1_000_000.0).round() as u64)
            };
            let cost = micro(&bucket["costUsd"]);
            history.uncached += n("uncachedInputTokens");
            history.cached += n("cachedInputTokens");
            history.cache_write += n("cacheCreationTokens");
            history.output += n("outputTokens");
            history.savings_microusd += micro(&bucket["cacheSavingsUsd"]);
            for (slot, key) in ["input", "cacheRead", "cacheWrite", "output"]
                .iter()
                .enumerate()
            {
                history.cost_by_type[slot] += micro(&bucket["categoryCostUsd"][*key]);
            }
            if let (Some(index), Some(provider)) = (
                history.days.iter().position(|d| d == day),
                bucket["provider"].as_str(),
            ) {
                let len = history.days.len();
                let row = match history
                    .providers
                    .iter()
                    .position(|p| p.provider == provider)
                {
                    Some(i) => &mut history.providers[i],
                    None => {
                        history.providers.push(ProviderHistory {
                            provider: provider.into(),
                            cost: vec![0; len],
                            tokens: vec![0; len],
                            sessions: 0,
                            sessions_distinct: false,
                        });
                        history.providers.last_mut().unwrap()
                    }
                };
                row.cost[index] += cost;
                row.tokens[index] += tokens;
                row.sessions += ["sessionCount", "sessions", "threadCount"]
                    .iter()
                    .find_map(|k| bucket[*k].as_u64())
                    .unwrap_or(0);
            }
            if day == today {
                today_buckets.push(bucket.clone());
            }
        }
        // Sources deduplicate sessions spanning days/models. Older servers lack this field.
        for row in &mut history.providers {
            let sources: Vec<_> = value["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["fingerprint"]["provider"] == row.provider)
                .collect();
            if !sources.is_empty()
                && sources
                    .iter()
                    .all(|s| s["distinctSessions"].as_u64().is_some())
            {
                row.sessions = sources
                    .iter()
                    .filter_map(|s| s["distinctSessions"].as_u64())
                    .sum();
                row.sessions_distinct = true;
            }
        }
        self.usage(&json!({ "buckets": today_buckets }));
        self.history = Some(history);
    }
    fn usage(&mut self, value: &Value) {
        let mut groups: BTreeMap<(String, String), Usage> = BTreeMap::new();
        for bucket in value["buckets"].as_array().into_iter().flatten() {
            let (Some(provider), Some(model)) =
                (bucket["provider"].as_str(), bucket["model"].as_str())
            else {
                continue;
            };
            let usage = groups
                .entry((provider.into(), model.into()))
                .or_insert_with(|| Usage {
                    provider: provider.into(),
                    model: model.into(),
                    ..Default::default()
                });
            // Reasoning is already included in output, so never add it twice.
            for key in [
                "uncachedInputTokens",
                "cachedInputTokens",
                "cacheCreationTokens",
                "outputTokens",
            ] {
                usage.tokens = usage
                    .tokens
                    .saturating_add(bucket["totals"][key].as_u64().unwrap_or(0));
            }
            if let Some(cost) = bucket["costUsd"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
            {
                usage.cost_microusd = usage
                    .cost_microusd
                    .saturating_add((cost * 1_000_000.0).round() as u64);
            }
            usage.unpriced |= bucket["unpricedRecords"].as_u64().unwrap_or(0) > 0;
        }
        self.usage = Some(groups.into_values().collect());
        self.usage_notice = None;
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub thread_id: String,
    pub run_id: String,
    pub status: Status,
    pub attention: bool,
}

impl Snapshot {
    pub fn apply(&mut self, item: &Value) -> Result<()> {
        match item["kind"].as_str() {
            Some("snapshot") => {
                let snapshot = &item["snapshot"];
                ensure!(
                    snapshot["schemaVersion"].as_u64().is_some(),
                    "missing T3 schema version"
                );
                // Enrichment snapshots update project identity only; their empty thread array is not authoritative.
                if item.get("resolvedRepositoryIdentityRoots").is_some() {
                    for project in snapshot["projects"].as_array().into_iter().flatten() {
                        if let (Some(id), Some(title)) =
                            (project["id"].as_str(), project["title"].as_str())
                        {
                            self.projects.insert(id.into(), title.into());
                        }
                    }
                    return Ok(());
                }
                let threads: Vec<Thread> = serde_json::from_value(snapshot["threads"].clone())?;
                ensure!(
                    threads.len() <= 16_384,
                    "T3 has too many threads to load. Archive old threads, then retry."
                );
                self.threads = threads;
                self.details.clear();
                for thread in snapshot["threads"].as_array().into_iter().flatten() {
                    self.detail(thread);
                }
                self.sequence = snapshot["snapshotSequence"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("missing T3 sequence"))?;
                self.projects.clear();
                for project in snapshot["projects"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(title)) = (
                        project["id"].as_str(),
                        project["title"]
                            .as_str()
                            .or_else(|| project["name"].as_str()),
                    ) {
                        self.projects.insert(id.into(), title.into());
                    }
                }
                self.transitions.clear(); // A new/reconnected snapshot is a baseline, never historic completion notices.
            }
            Some("synchronized") => {}
            Some(
                kind
                @ ("thread.updated" | "thread.removed" | "project.updated" | "project.removed"),
            ) => {
                let sequence = item["sequence"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("missing T3 sequence"))?;
                if sequence <= self.sequence {
                    return Ok(());
                }
                // Filtered summary streams legitimately skip global sequence numbers.
                self.sequence = sequence;
                match kind {
                    "thread.updated" => {
                        let thread: Thread = serde_json::from_value(item["thread"].clone())?;
                        self.detail(&item["thread"]);
                        let previous = self.threads.iter().position(|t| t.id == thread.id);
                        if item["location"] == "archive" {
                            self.threads.retain(|t| t.id != thread.id);
                            self.details.remove(&thread.id);
                            return Ok(());
                        }
                        if let Some(index) = previous {
                            let old = &self.threads[index];
                            let attention = thread.pending_runtime_request.as_ref().map(|r| &r.id)
                                != old.pending_runtime_request.as_ref().map(|r| &r.id)
                                && thread.attention();
                            let completion = thread.finished()
                                && (old.latest_run_id != thread.latest_run_id || !old.finished());
                            if (attention || completion)
                                && let Some(run_id) = thread
                                    .active_run_id
                                    .as_ref()
                                    .or(thread.latest_run_id.as_ref())
                            {
                                let transition = Transition {
                                    thread_id: thread.id.clone(),
                                    run_id: run_id.clone(),
                                    status: thread.status,
                                    attention,
                                };
                                if !self.transitions.contains(&transition) {
                                    self.transitions.push(transition);
                                    if self.transitions.len() > 16 {
                                        self.transitions.remove(0);
                                    }
                                }
                            }
                            self.threads[index] = thread;
                        } else {
                            ensure!(
                                self.threads.len() < 16_384,
                                "T3 has too many threads to load. Archive old threads, then retry."
                            );
                            self.threads.push(thread);
                        }
                    }
                    "thread.removed" => {
                        self.threads
                            .retain(|t| Some(t.id.as_str()) != item["threadId"].as_str());
                        if let Some(id) = item["threadId"].as_str() {
                            self.details.remove(id);
                        }
                    }
                    "project.updated" => {
                        if let (Some(id), Some(title)) = (
                            item["project"]["id"].as_str(),
                            item["project"]["title"].as_str(),
                        ) {
                            self.projects.insert(id.into(), title.into());
                        }
                    }
                    "project.removed" => {
                        if let Some(id) = item["projectId"].as_str() {
                            self.projects.remove(id);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => bail!("unsupported T3 shell event; update the adapter"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_stream_keeps_both_claude_accounts_across_provider_updates() {
        let mut metadata = crate::t3_client::Metadata::default();
        let mut state = Snapshot::default();
        let provider = json!({"instanceId":"codex","driver":"codex","displayName":"Codex","installed":true,"enabled":true,
            "usageLimits":{"windows":[{"label":"Weekly","usedPercent":66}]}});
        for event in [
            json!({"type":"snapshot","config":{"providers":[provider.clone()]}}),
            json!({"type":"usageLimitSourcesUpdated","payload":{"sources":[{"id":"hub","accounts":[
                {"id":"c1","driver":"claudeAgent","email":"one@example.test","usageLimits":{"windows":[{"label":"Weekly","usedPercent":43}]}},
                {"id":"c2","driver":"claudeAgent","email":"two@example.test","usageLimits":{"windows":[{"label":"Weekly","usedPercent":29}]}}
            ]}]}}),
            json!({"type":"providerStatuses","payload":{"providers":[provider.clone()]}}),
            json!({"type":"snapshot","config":{"providers":[provider]}}),
        ] {
            let frame: ShellEnvelope = serde_json::from_value(
                json!({"_tag":"Chunk","requestId":"config","values":[event]}),
            )
            .unwrap();
            for item in frame.values {
                assert!(metadata.apply(serde_json::from_str(item.get()).unwrap()));
            }
            state.configure(&metadata);
        }
        assert_eq!(state.limits.len(), 3);
        let claude: Vec<_> = state
            .limits
            .iter()
            .filter(|l| l.driver == "claudeAgent")
            .collect();
        assert_eq!(claude.len(), 2);
        assert_eq!(
            (&claude[0].account, claude[0].used_permille),
            (&"one@example.test".to_owned(), 430)
        );
        assert_eq!(
            (&claude[1].account, claude[1].used_permille),
            (&"two@example.test".to_owned(), 290)
        );
        assert!(
            metadata.apply(
                serde_json::from_value(
                    json!({"type":"usageLimitSourcesUpdated","payload":{"sources":[]}})
                )
                .unwrap()
            )
        );
        state.configure(&metadata);
        assert_eq!(state.limits.len(), 1);
        assert_eq!(state.limits[0].driver, "codex");
    }

    #[test]
    fn usage_prefers_source_distinct_sessions_and_marks_older_bucket_counts() {
        let mut state = Snapshot::default();
        let days = vec!["2026-11-04".into(), "2026-11-05".into()];
        let mut summary = json!({"buckets":[
            {"day":"2026-11-04","provider":"codex","sessions":3},
            {"day":"2026-11-05","provider":"codex","sessions":3},
            {"day":"2026-11-05","provider":"codex","sessions":2}
        ],"sources":[{"fingerprint":{"provider":"codex"},"distinctSessions":4}]});
        state.history(&summary, days.clone(), "2026-11-05");
        let row = &state.history.as_ref().unwrap().providers[0];
        assert_eq!(row.sessions, 4);
        assert!(row.sessions_distinct);
        summary["sources"] = json!([]);
        state.history(&summary, days, "2026-11-05");
        let row = &state.history.as_ref().unwrap().providers[0];
        assert_eq!(row.sessions, 8);
        assert!(!row.sessions_distinct);
    }
    #[test]
    fn usage_groups_models_and_does_not_double_count_reasoning() {
        let mut state = Snapshot::default();
        state.usage(&json!({"buckets":[
            {"provider":"codex","model":"sol","totals":{"uncachedInputTokens":10,"cachedInputTokens":20,"cacheCreationTokens":30,"outputTokens":40,"reasoningTokens":35},"costUsd":0.125,"unpricedRecords":0},
            {"provider":"codex","model":"sol","totals":{"outputTokens":5},"costUsd":0.025,"unpricedRecords":1},
            {"provider":"claude","model":"opus","totals":{"outputTokens":7},"costUsd":0.02,"unpricedRecords":0}
        ]}));
        let usage = state.usage.unwrap();
        assert_eq!(usage.len(), 2);
        let sol = usage.iter().find(|u| u.model == "sol").unwrap();
        assert_eq!(sol.tokens, 105);
        assert_eq!(sol.cost_microusd, 150_000);
        assert!(sol.unpriced);
    }
}
