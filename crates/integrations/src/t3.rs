//! Effect RPC v2 shell summaries, with bounded metadata and no client ping task.
use crate::{Availability, Data, Publish, credentials, transport};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 2;
// The installed server's shell baseline is ~2.4 MiB. This is a wire bound; only
// typed metadata is retained, never the shell's message previews or transcripts.
pub const MESSAGE_LIMIT: usize = 32 * 1024 * 1024;

// These wire structs deliberately omit preview bodies, repository identities and scripts.
// Serde skips them on the socket buffer instead of allocating a full JSON tree for every wake.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellEnvelope {
    #[serde(rename = "_tag")]
    tag: String,
    request_id: Option<String>,
    #[serde(default)]
    values: Vec<ShellItem>,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct ShellItem {
    kind: String,
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
    /// One row per provider account and limit window, from `server.getConfig`.
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
    /// Sessions over the whole window, when T3 reports them.
    pub sessions: u64,
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
    fn configure(&mut self, value: crate::t3_client::Metadata) {
        self.omit_older_settled = value.shell_older_settled;
        self.omit_idle_subagents = value.shell_thread_families;
        self.environment_id = Some(value.environment.environment_id).filter(|s| !s.is_empty());
        self.models.clear();
        self.limits.clear();
        for provider in value
            .providers
            .into_iter()
            .filter(|p| p.enabled && p.installed)
        {
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
            for model in provider.models.into_iter().take(256) {
                if !model.slug.is_empty() {
                    self.models.push(ModelChoice {
                        provider: provider.instance_id.clone(),
                        provider_name: provider.display_name.clone(),
                        model: model.slug,
                        name: model.name,
                    });
                }
            }
        }
    }
    /// The 30-day history, and today's per-model totals for the existing usage face.
    fn history(&mut self, value: &Value, days: Vec<String>, today: &str) {
        let mut history = History {
            days,
            ..Default::default()
        };
        let mut today_buckets = Vec::new();
        // Field names only (no values), once per read, to confirm the wire shape.
        if let Some(first) = value["buckets"].as_array().and_then(|b| b.first()).and_then(Value::as_object) {
            log::info!("T3 usage bucket keys: {:?}", first.keys().collect::<Vec<_>>());
        }
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
            for (slot, key) in ["input", "cacheRead", "cacheWrite", "output"].iter().enumerate() {
                history.cost_by_type[slot] += micro(&bucket["categoryCostUsd"][*key]);
            }
            if let (Some(index), Some(provider)) = (
                history.days.iter().position(|d| d == day),
                bucket["provider"].as_str(),
            ) {
                let len = history.days.len();
                let row = match history.providers.iter().position(|p| p.provider == provider) {
                    Some(i) => &mut history.providers[i],
                    None => {
                        history.providers.push(ProviderHistory {
                            provider: provider.into(),
                            cost: vec![0; len],
                            tokens: vec![0; len],
                            sessions: 0,
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

pub(crate) async fn run(config: &crate::config::T3, publish: &Publish) -> Result<()> {
    ensure!(
        config.protocol_version == PROTOCOL_VERSION,
        "supported T3 orchestration protocol is 2"
    );
    ensure!(
        config.endpoint.is_empty() || crate::transport::loopback_url(&config.endpoint).is_ok(),
        "Use a local T3 Code address in Advanced settings."
    );
    // Background access first: T3's CLI issues (or renews) mypaper's token with no prompt.
    let issue = |force| tokio::task::spawn_blocking(move || crate::t3_auth::ensure(force));
    let auto = issue(false).await?;
    if let Err(error) = &auto {
        log::info!("T3 auto-connect: {error:#}");
    }
    let live = config.live_verified || auto.is_ok();
    let token = credentials::read("t3").map_err(|_| match &auto {
        Err(error) => anyhow::anyhow!("{error}"),
        Ok(_) => anyhow::anyhow!("T3 Code access is missing"),
    })?;
    let mut result = subscribe(config, live, publish, token.text()?).await;
    if result.as_ref().is_err_and(unauthorized) && crate::t3_auth::available() {
        // The server dropped our grant (revoked or expired early): re-issue once.
        issue(true).await??;
        let token = credentials::read("t3")?;
        result = subscribe(config, true, publish, token.text()?).await;
    }
    result.map_err(|error| {
        log::warn!("T3 subscription: {error:#}");
        if unauthorized(&error) {
            anyhow::anyhow!("T3 Code no longer accepts mypaper's access")
        } else {
            anyhow::anyhow!("T3 disconnected. Retry to reconnect to your local T3 Code.")
        }
    })
}

/// A 401/403 on the socket upgrade: the bearer itself was refused.
fn unauthorized(error: &anyhow::Error) -> bool {
    use tokio_tungstenite::tungstenite::Error;
    error.chain().any(|cause| {
        matches!(cause.downcast_ref::<Error>(), Some(Error::Http(response)) if matches!(response.status().as_u16(), 401 | 403))
    })
}

async fn subscribe(
    config: &crate::config::T3,
    live: bool,
    publish: &Publish,
    token: &str,
) -> Result<()> {
    let mut socket = transport::socket_with_limit(
        crate::t3_client::endpoint(&config.endpoint),
        Some(token),
        Some("orchestrationProtocol=2"),
        MESSAGE_LIMIT,
    )
    .await?;
    let mut state = Snapshot::default();
    if live {
        state.permissions = crate::t3_client::permissions(&config.endpoint)
            .await
            .unwrap_or_default();
        if let Ok(value) = crate::t3_client::metadata(&config.endpoint).await {
            state.configure(value);
        }
        refresh_usage(config, &mut state).await;
    }
    let mut payload = json!({"requestCompletionMarker":true});
    if state.omit_older_settled {
        payload["omitOlderSettled"] = json!(true);
    }
    if state.omit_idle_subagents {
        payload["omitIdleSubagents"] = json!(true);
    }
    transport::send(&mut socket,json!({"_tag":"Request","id":"1","tag":"orchestration.subscribeShell","payload":payload,"headers":[]})).await?;
    let mut baseline = false;
    let mut last_usage_transition = None;
    loop {
        let bytes = transport::receive_message(&mut socket).await?.into_data();
        publish.delivered();
        let frames: Vec<ShellEnvelope> = if bytes.first() == Some(&b'[') {
            serde_json::from_slice(&bytes)?
        } else {
            vec![serde_json::from_slice(&bytes)?]
        };
        for frame in frames {
            match frame.tag.as_str() {
                "Chunk" => {
                    ensure!(
                        frame.request_id.as_deref() == Some("1"),
                        "unexpected T3 request id"
                    );
                    for item in frame.values {
                        ensure!(
                            baseline || item.kind == "snapshot",
                            "T3 delta arrived before its baseline"
                        );
                        state.apply(&serde_json::to_value(item)?)?;
                        baseline = true;
                    }
                    let transition = state
                        .transitions
                        .last()
                        .filter(|t| !t.attention)
                        .map(|t| (t.run_id.clone(), t.status));
                    if transition.is_some() && transition != last_usage_transition {
                        last_usage_transition = transition;
                        // A finished run moves the limit windows too; getConfig carries them.
                        if let Ok(value) = crate::t3_client::metadata(&config.endpoint).await {
                            state.configure(value);
                        }
                        refresh_usage(config, &mut state).await;
                    }
                    publish.set(
                        if live {
                            Availability::Live
                        } else {
                            Availability::Cached
                        },
                        Some(Data::T3(state.clone())),
                    );
                    transport::send(&mut socket, json!({"_tag":"Ack","requestId":"1"})).await?;
                    if !live {
                        return Ok(());
                    }
                }
                "Ping" | "Pong" => {
                    bail!("T3 server maintenance frames detected; use snapshot-on-open")
                }
                "Exit" | "Defect" => bail!(
                    "T3 subscription ended or was rejected; check pairing and protocol version"
                ),
                _ => bail!("unsupported Effect RPC envelope"),
            }
        }
    }
}

async fn refresh_usage(config: &crate::config::T3, state: &mut Snapshot) {
    if !state.permissions.iter().any(|p| p == "diagnostics:read") {
        state.usage_notice = Some("Usage is unavailable. Refresh to reconnect.".into());
        return;
    }
    let now = chrono::Utc::now().date_naive();
    let days: Vec<String> = (0..30)
        .rev()
        .map(|back| (now - chrono::Days::new(back)).format("%Y-%m-%d").to_string())
        .collect();
    let day = days[29].clone();
    match crate::t3_client::request(
        &config.endpoint,
        "server.getUsageSummary",
        json!({"sinceDay":days[0],"untilDay":day,"timeZone":"UTC","resolution":"day"}),
    )
    .await
    {
        Ok(value) => {
            state.history(&value, days, &day);
            state.usage_day = Some(day);
        }
        Err(_) => state.usage_notice = Some("Usage is unavailable. Refresh to try again.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::{Arc, Mutex};
    use tokio_tungstenite::tungstenite::Message;
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
    #[test]
    #[allow(
        clippy::result_large_err,
        reason = "tungstenite handshake callback has a fixed external error type"
    )]
    fn authenticated_rpc_fixture_seeds_acknowledges_stops_and_reconnects() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config=crate::config::T3 { enabled:true,endpoint:format!("ws://{}/ws",listener.local_addr().unwrap()),..Default::default() };
            let server=tokio::spawn(async move {
                for _ in 0..2 {
                    let (stream,_)=listener.accept().await.unwrap();
                    let mut socket=tokio_tungstenite::accept_hdr_async(stream,|request:&tokio_tungstenite::tungstenite::handshake::server::Request,response| {
                        assert_eq!(request.uri().query(),Some("orchestrationProtocol=2"));
                        assert_eq!(request.headers()["authorization"],"Bearer fixture-only");
                        Ok(response)
                    }).await.unwrap();
                    let request:Value=serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                    assert_eq!(request["tag"],"orchestration.subscribeShell");
                    // Match the real failure: 996 threads and a >2 MiB single frame.
                    let threads: Vec<_> = (0..996).map(|i| json!({
                        "id":format!("thread-{i}"),"projectId":"p","title":"Fixture thread",
                        "providerInstanceId":"codex","status":"running","latestRunId":null,
                        "activeRunId":null,"pendingRuntimeRequest":null,"updatedAt":"2026-11-05",
                        "latestVisibleMessage":{"text":"x".repeat(2400)}
                    })).collect();
                    let baseline=json!({"_tag":"Chunk","requestId":"1","values":[{"kind":"snapshot","snapshot":{"schemaVersion":2,"snapshotSequence":4,"projects":[],"threads":threads}}]}).to_string();
                    assert!(baseline.len()>2*1024*1024);
                    socket.send(Message::Text(baseline.into())).await.unwrap();
                    let ack:Value=serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                    assert_eq!(ack,json!({"_tag":"Ack","requestId":"1"}));
                    assert!(socket.next().await.is_none_or(|r|r.is_err() || matches!(r,Ok(Message::Close(_)))));
                }
            });
            let publish=Publish { provider:crate::Provider::T3,snapshot:Arc::new(Mutex::new(crate::Snapshot::default())),wake:Arc::new(|_|{}),counters:Arc::new(crate::Counters::default()) };
            for _ in 0..2 {
                subscribe(&config,false,&publish,"fixture-only").await.unwrap();
                let snapshot=publish.snapshot.lock().unwrap();
                assert_eq!(snapshot.availability,Availability::Cached);
                assert!(matches!(&snapshot.data,Some(Data::T3(state)) if state.threads.len()==996));
            }
            server.await.unwrap();
        });
    }
}
