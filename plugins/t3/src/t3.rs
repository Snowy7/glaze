//! Effect RPC v2 shell summaries, with bounded metadata and no client ping task.
use anyhow::{Result, bail, ensure};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// More threads than this in one baseline is refused (the faces can't use them anyway).
const MAX_THREADS: usize = 16_384;
const TOO_MANY: &str = "T3 has too many threads to load. Archive old threads, then retry.";

// These wire structs deliberately omit preview bodies, repository identities and scripts.
// Serde skips them on the socket buffer instead of allocating a full JSON tree for every wake.
// Memory: the baseline is one ~2.4 MiB frame and the guest's linear memory never shrinks, so the
// decode holds the frame once (borrowed `RawValue`s) and builds the kept threads straight from
// it: no `Value` tree and no second copy of the frame.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellEnvelope<'a> {
    #[serde(rename = "_tag")]
    pub(crate) tag: String,
    pub(crate) request_id: Option<String>,
    // Route by request id before decoding. An untagged enum buffers even discarded previews.
    #[serde(default, borrow)]
    pub(crate) values: Vec<&'a RawValue>,
    /// A finished request's `{"_tag":"Success"|"Failure","value":…}`.
    #[serde(default, borrow)]
    pub(crate) exit: Option<&'a RawValue>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ShellItem {
    pub(crate) kind: String,
    snapshot: Option<ShellBaseline>,
    resolved_repository_identity_roots: Option<Vec<String>>,
    sequence: Option<u64>,
    location: Option<String>,
    thread: Option<ShellThread>,
    thread_id: Option<String>,
    project: Option<ShellProject>,
    project_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellBaseline {
    #[allow(dead_code)] // required: a baseline without it is refused
    schema_version: u64,
    snapshot_sequence: u64,
    #[serde(deserialize_with = "baseline_threads")]
    threads: (Vec<Thread>, BTreeMap<String, ThreadDetail>),
    projects: Vec<ShellProject>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ShellProject {
    id: String,
    #[serde(alias = "name")]
    title: String,
}
#[derive(Deserialize)]
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
    #[serde(default)]
    blocked_on_thread_wait: bool,
    #[serde(default)]
    latest_run_completed_at: Option<String>,
    #[serde(default)]
    last_visited_at: Option<String>,
    #[serde(default)]
    settled_override: Option<String>,
    #[serde(default)]
    settled_at: Option<String>,
    #[serde(default)]
    snoozed_until: Option<String>,
    #[serde(default)]
    archived_at: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct ShellTask {
    kind: String,
    awaited: bool,
}
impl ShellTask {
    /// T3's `backgroundWorkHoldsCompletion`: subagents and background tasks hold a thread's
    /// completion; commands and monitors only when the agent awaits them.
    fn holds(&self) -> bool {
        self.awaited || matches!(self.kind.as_str(), "subagent" | "background_task")
    }
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct ShellModel {
    model: String,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ShellLineage {
    parent_thread_id: Option<String>,
    relationship_to_parent: Option<String>,
}
impl ShellThread {
    /// The thread the faces list and the detail drawn beside it (the strings are moved).
    fn split(self) -> (Thread, ThreadDetail) {
        let detail = ThreadDetail {
            model: self.model_selection.model.chars().take(128).collect(),
            parent_id: self.lineage.parent_thread_id,
            agent: self.lineage.relationship_to_parent.as_deref() == Some("subagent"),
            background_tasks: self.pending_background_tasks.len(),
        };
        let held = self.pending_background_tasks.iter().any(ShellTask::holds);
        // Settled by the user (or by T3, until they make it active again).
        let settled = match self.settled_override.as_deref() {
            Some("settled") => true,
            Some("active") => false,
            _ => self.settled_at.is_some(),
        };
        let thread = Thread {
            settled,
            archived: self.archived_at.is_some(),
            snoozed_until: self.snoozed_until,
            blocked_on_thread_wait: self.blocked_on_thread_wait,
            held,
            latest_run_completed_at: self.latest_run_completed_at,
            last_visited_at: self.last_visited_at,
            id: self.id,
            project_id: self.project_id,
            title: self.title,
            provider_instance_id: self.provider_instance_id,
            status: self.status,
            latest_run_id: self.latest_run_id,
            active_run_id: self.active_run_id,
            pending_runtime_request: self.pending_runtime_request,
            updated_at: self.updated_at,
        };
        (thread, detail)
    }
}
/// Decodes the baseline's thread array element by element into what `Snapshot` keeps, so the
/// wire records never exist as a second array beside the kept ones.
fn baseline_threads<'de, D: Deserializer<'de>>(d: D) -> Result<(Vec<Thread>, BTreeMap<String, ThreadDetail>), D::Error> {
    struct Threads;
    impl<'de> Visitor<'de> for Threads {
        type Value = (Vec<Thread>, BTreeMap<String, ThreadDetail>);
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array of T3 threads")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let (mut threads, mut details) = (Vec::new(), BTreeMap::new());
            while let Some(thread) = seq.next_element::<ShellThread>()? {
                if threads.len() >= MAX_THREADS {
                    return Err(de::Error::custom(TOO_MANY));
                }
                let (thread, detail) = thread.split();
                details.insert(thread.id.clone(), detail);
                threads.push(thread);
            }
            threads.shrink_to_fit();
            Ok((threads, details))
        }
    }
    d.deserialize_seq(Threads)
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
    /// Running, but only waiting on another thread (T3's `blockedOnThreadWait`).
    #[serde(default)]
    pub blocked_on_thread_wait: bool,
    /// Background work holds its completion (a subagent, an awaited task).
    #[serde(default)]
    pub held: bool,
    #[serde(default)]
    pub latest_run_completed_at: Option<String>,
    /// When the user last opened it in T3 (T3's read state).
    #[serde(default)]
    pub last_visited_at: Option<String>,
    #[serde(default)]
    pub settled: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub snoozed_until: Option<String>,
}

/// A thread's sidebar state, derived as T3's `resolveThreadStatusPill` does (approval, input,
/// working, connecting, waiting, unseen failure, unseen completion; otherwise none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    Approval,
    Input,
    Working,
    Connecting,
    Waiting,
    Failed,
    Done,
}

fn instant(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp_millis())
}

impl Thread {
    /// Off every list and count: settled or archived, or snoozed until later (T3's
    /// `effectiveSnoozed`; a snoozed thread that asks for something comes back, as in T3).
    pub fn hidden(&self, now_ms: i64) -> bool {
        let snoozed = self.snoozed_until.as_deref().and_then(instant).is_some_and(|wake| wake > now_ms) && self.pending_runtime_request.is_none();
        self.settled || self.archived || snoozed
    }

    /// `agents_busy`: one of its subagents is still working, which keeps an idle parent Waiting.
    pub fn pill(&self, agents_busy: bool) -> Option<Pill> {
        if let Some(request) = &self.pending_runtime_request {
            let input = matches!(request.kind.as_str(), "user_input" | "user-input");
            return Some(if input { Pill::Input } else { Pill::Approval });
        }
        match self.status {
            Status::Running if !self.blocked_on_thread_wait => return Some(Pill::Working),
            Status::Waiting => return Some(Pill::Working),
            Status::Preparing | Status::Starting | Status::Queued => return Some(Pill::Connecting),
            _ => {}
        }
        if self.status == Status::Running || self.held || agents_busy {
            return Some(Pill::Waiting);
        }
        let completed = self.latest_run_completed_at.as_deref().and_then(instant);
        let visited = self.last_visited_at.as_deref().map(instant);
        // Unopened failures count as unseen; unopened completions don't (T3's rule).
        if self.status == Status::Failed {
            return match (completed, visited) {
                (Some(done), Some(Some(seen))) if done <= seen => None,
                _ => Some(Pill::Failed),
            };
        }
        match (completed, visited) {
            (Some(done), Some(Some(seen))) if done > seen => Some(Pill::Done),
            (Some(_), Some(None)) => Some(Pill::Done),
            _ => None,
        }
    }
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
    pub(crate) fn configure(&mut self, value: &crate::t3_client::Metadata) {
        self.omit_older_settled = value.shell_older_settled;
        self.omit_idle_subagents = value.shell_thread_families;
        // A config stream snapshot may come without the environment; thread links still need
        // the one `server.getConfig` gave, so only a non-empty id replaces it.
        if !value.environment.environment_id.is_empty() {
            self.environment_id = Some(value.environment.environment_id.clone());
        }
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
    /// Applies one shell stream item: a baseline `snapshot` or a delta.
    pub(crate) fn apply(&mut self, item: ShellItem) -> Result<()> {
        match item.kind.as_str() {
            "snapshot" => {
                let Some(snapshot) = item.snapshot else { bail!("missing T3 schema version") };
                // Enrichment snapshots update project identity only; their empty thread array is not authoritative.
                if item.resolved_repository_identity_roots.is_some() {
                    for project in snapshot.projects {
                        self.projects.insert(project.id, project.title);
                    }
                    return Ok(());
                }
                (self.threads, self.details) = snapshot.threads;
                self.sequence = snapshot.snapshot_sequence;
                self.projects.clear();
                for project in snapshot.projects {
                    self.projects.insert(project.id, project.title);
                }
                self.transitions.clear(); // A new/reconnected snapshot is a baseline, never historic completion notices.
            }
            "synchronized" => {}
            kind @ ("thread.updated" | "thread.removed" | "project.updated" | "project.removed") => {
                let sequence = item.sequence.ok_or_else(|| anyhow::anyhow!("missing T3 sequence"))?;
                if sequence <= self.sequence {
                    return Ok(());
                }
                // Filtered summary streams legitimately skip global sequence numbers.
                self.sequence = sequence;
                match kind {
                    "thread.updated" => {
                        let (thread, detail) = item.thread.ok_or_else(|| anyhow::anyhow!("a T3 thread update without its thread"))?.split();
                        self.details.insert(thread.id.clone(), detail);
                        let previous = self.threads.iter().position(|t| t.id == thread.id);
                        if item.location.as_deref() == Some("archive") {
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
                            ensure!(self.threads.len() < MAX_THREADS, TOO_MANY);
                            self.threads.push(thread);
                        }
                    }
                    "thread.removed" => {
                        self.threads.retain(|t| Some(t.id.as_str()) != item.thread_id.as_deref());
                        if let Some(id) = &item.thread_id {
                            self.details.remove(id);
                        }
                    }
                    "project.updated" => {
                        if let Some(project) = item.project {
                            self.projects.insert(project.id, project.title);
                        }
                    }
                    "project.removed" => {
                        if let Some(id) = &item.project_id {
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

    /// Counts live heap bytes so a test can read the peak of one decode.
    struct Counting;
    static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    unsafe impl std::alloc::GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            use std::sync::atomic::Ordering::Relaxed;
            let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(live, Relaxed);
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
            LIVE.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
            unsafe { std::alloc::System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOC: Counting = Counting;

    /// A shell baseline frame shaped like T3's: `threads` summaries with the preview bodies and
    /// repository fields the adapter skips.
    fn baseline(threads: usize) -> String {
        let thread = |i: usize| json!({"id":format!("thread-{i:08}"),"projectId":format!("project-{}", i % 12),"title":format!("Thread number {i} with a title"),
            "providerInstanceId":"codex","status":if i % 7 == 0 {"running"} else {"completed"},"latestRunId":format!("run-{i:08}"),"activeRunId":null,
            "pendingRuntimeRequest":null,"updatedAt":"2026-11-05T21:00:00.000Z","modelSelection":{"model":"gpt-6.1-sol","effort":"xhigh"},
            "lineage":{"parentThreadId":if i % 5 == 0 {Some(format!("thread-{:08}", i - 1))} else {None},"relationshipToParent":if i % 5 == 0 {"subagent"} else {"root"}},
            "pendingBackgroundTasks":[],"preview":{"text":"x".repeat(420),"role":"assistant"},"repository":{"root":"D:/Coding/projects/x","branch":"main"}});
        let projects: Vec<Value> = (0..12).map(|i| json!({"id":format!("project-{i}"),"title":format!("Project {i}")})).collect();
        json!({"_tag":"Chunk","requestId":"1","values":[{"kind":"snapshot","snapshot":{"schemaVersion":2,"snapshotSequence":41,
            "threads":(0..threads).map(thread).collect::<Vec<_>>(),"projects":projects}}]})
        .to_string()
    }

    /// What `component.rs` does with one shell frame.
    fn decode(state: &mut Snapshot, frame: &str) {
        let envelope: ShellEnvelope = serde_json::from_str(frame).unwrap();
        for item in envelope.values {
            state.apply(serde_json::from_str(item.get()).unwrap()).unwrap();
        }
    }

    #[test]
    fn baseline_then_delta_keeps_the_faces_fields() {
        let mut state = Snapshot::default();
        decode(&mut state, &baseline(10));
        assert_eq!((state.threads.len(), state.details.len(), state.projects.len(), state.sequence), (10, 10, 12, 41));
        assert_eq!(state.details["thread-00000005"], ThreadDetail { model: "gpt-6.1-sol".into(), parent_id: Some("thread-00000004".into()), agent: true, background_tasks: 0 });
        assert_eq!(state.threads[7].status, Status::Running);
        let update = json!({"_tag":"Chunk","requestId":"1","values":[{"kind":"thread.updated","sequence":42,"thread":{"id":"thread-00000007","projectId":"project-7",
            "title":"Done","providerInstanceId":"codex","status":"completed","latestRunId":"run-00000007","activeRunId":null,"pendingRuntimeRequest":null,
            "updatedAt":"2026-11-05T21:01:00.000Z"}},{"kind":"thread.removed","sequence":43,"threadId":"thread-00000001"}]});
        decode(&mut state, &update.to_string());
        assert_eq!(state.threads.len(), 9);
        assert_eq!(state.transitions, vec![Transition { thread_id: "thread-00000007".into(), run_id: "run-00000007".into(), status: Status::Completed, attention: false }]);
        assert_eq!(state.details["thread-00000007"].model, "");
    }

    /// `cargo test --release -- --ignored --nocapture --test-threads=1 baseline_decode_peak` (the
    /// counter is process-wide, so run it alone): the heap peak of
    /// decoding a ~2.4 MiB baseline on top of the frame itself (the guest's linear memory keeps
    /// its peak, so this is roughly what T3 costs for good).
    #[test]
    #[ignore]
    fn baseline_decode_peak() {
        use std::sync::atomic::Ordering::Relaxed;
        let frame = baseline(3000);
        let mut state = Snapshot::default();
        let before = LIVE.load(Relaxed);
        PEAK.store(before, Relaxed);
        decode(&mut state, &frame);
        let (peak, kept) = (PEAK.load(Relaxed) - before, LIVE.load(Relaxed) - before);
        println!("frame {:.2} MiB, decode peak +{:.2} MiB, kept {:.2} MiB", frame.len() as f64 / 1048576.0, peak as f64 / 1048576.0, kept as f64 / 1048576.0);
    }

    /// `cargo test --release -- --ignored --nocapture redraw_cost`: what one click costs, which is
    /// a redraw of every face (wasm runs this ~1.5-3x slower than native).
    #[test]
    #[ignore]
    fn redraw_cost() {
        let mut state = Snapshot::default();
        decode(&mut state, &baseline(3000));
        let snapshot = crate::Snapshot { availability: crate::Availability::Live, data: Some(crate::Data::T3(state)) };
        let started = std::time::Instant::now();
        for face in ["running", "attention", "limits", "status"] {
            for size in [mypaper_plugin_sdk::SizeClass::Small, mypaper_plugin_sdk::SizeClass::Large] {
                std::hint::black_box(crate::faces::face(face, size, &snapshot, &Default::default(), 0));
            }
        }
        println!("8 face draws over 3000 threads: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
    }

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
            let text = json!({"_tag":"Chunk","requestId":"config","values":[event]}).to_string();
            let frame: ShellEnvelope = serde_json::from_str(&text).unwrap();
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
