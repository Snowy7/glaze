//! The T3 faces as declarative views: Running now, Needs you, Limits, Usage, New thread.
//! Same structure as the shell's native faces; the host draws them with the kit.

use crate::t3::{Limit, ModelChoice, Snapshot as State, Thread};
use crate::{Availability, Data, Snapshot};
use mypaper_plugin_sdk::view::SelectOption;
use mypaper_plugin_sdk::{Face, Role, Series, SizeClass, Tone, View};

/// `face` at `size` from the provider's snapshot; `pick` is the New thread instance's choices.
pub fn face(face: &str, size: SizeClass, snapshot: &Snapshot, pick: &Pick, now_ms: i64) -> Face {
    let footer = footer(face, snapshot);
    let body = match &snapshot.data {
        Some(Data::T3(state)) => match face {
            // The top-bar module: the bar draws its text.
            "status" => View::text(
                format!("{} running · {} need input", state.threads.iter().filter(|t| t.running()).count(), state.threads.iter().filter(|t| t.attention()).count()),
                Role::Caption,
            ),
            "limits" => limits(state, size, now_ms),
            "usage" => usage(state, size),
            "new" => composer(state, size, pick),
            "attention" => threads(state, size, true),
            _ => threads(state, size, false),
        },
        None if face == "status" => View::text("T3", Role::Caption),
        None => match &snapshot.availability {
            Availability::Unavailable(why) => View::Stack {
                children: vec![
                    View::empty("terminal", "Connect T3 Code", why.clone()),
                    View::button("Retry", "retry"),
                ],
                gap: None,
            },
            _ => View::empty("terminal", "Connecting…", "Waiting for your local T3 Code"),
        },
    };
    Face { body, footer: Some(footer), refresh: true }
}

fn status(thread: &Thread) -> &'static str {
    use crate::t3::Status;
    if let Some(request) = &thread.pending_runtime_request {
        return if matches!(request.kind.as_str(), "user_input" | "user-input") { "Question for you" } else { "Approval needed" };
    }
    match thread.status {
        Status::Idle => "Idle",
        Status::Queued => "Queued",
        Status::Preparing => "Preparing",
        Status::Starting => "Starting",
        Status::Running => "Working",
        Status::Waiting => "Waiting",
        Status::Completed => "Completed",
        Status::Failed => "Failed",
        Status::Cancelled => "Cancelled",
        Status::Interrupted => "Interrupted",
        Status::RolledBack => "Rolled back",
    }
}

fn footer(face: &str, snapshot: &Snapshot) -> String {
    match &snapshot.availability {
        Availability::Live | Availability::Cached => match (face, &snapshot.data) {
            ("usage", Some(Data::T3(state))) => state
                .usage_day
                .as_ref()
                .map_or("Last 30 days".into(), |day| format!("30 days to {day} · UTC")),
            _ => "Live from your local T3 Code".into(),
        },
        Availability::Stale(_) => "Disconnected · showing last state".into(),
        Availability::Connecting => "Connecting…".into(),
        Availability::Unavailable(why) => why.clone(),
    }
}

/// A big number over a caption: the compact tiles.
fn metric(value: String, caption: String, tone: Tone) -> View {
    View::Stack {
        children: vec![
            View::Spacer,
            View::tinted(value, Role::LargeTitle, tone),
            View::tinted(caption, Role::Caption, Tone::Secondary),
            View::Spacer,
        ],
        gap: Some(2.0),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Top-level threads with their subagents rolled up (T3's own grouping).
fn threads(state: &State, size: SizeClass, attention: bool) -> View {
    let wanted = |t: &Thread| if attention { t.attention() } else { t.running() };
    let parent = |t: &Thread| state.details.get(&t.id).and_then(|d| d.parent_id.clone());
    let mut tops: Vec<&Thread> = state.threads.iter().filter(|t| parent(t).is_none()).collect();
    tops.sort_by(|a, b| b.attention().cmp(&a.attention()).then_with(|| b.updated_at.cmp(&a.updated_at)));
    let groups: Vec<(&Thread, Vec<&Thread>)> = tops
        .into_iter()
        .map(|top| {
            let agents = state
                .threads
                .iter()
                .filter(|t| parent(t).as_deref() == Some(top.id.as_str()) && wanted(t))
                .collect::<Vec<_>>();
            (top, agents)
        })
        .filter(|(top, agents)| wanted(top) || !agents.is_empty())
        .collect();
    let agents: usize = groups.iter().map(|g| g.1.len()).sum();
    let count = groups.len();
    if size == SizeClass::Small {
        let caption = if attention {
            "need your input".into()
        } else if agents == 0 {
            plural(count, "thread")
        } else {
            format!("{} · {}", plural(count, "thread"), plural(agents, "agent"))
        };
        let tone = if attention && count > 0 { Tone::Warning } else { Tone::Label };
        return metric(count.to_string(), caption, tone);
    }
    if count == 0 {
        return if attention {
            View::empty("check", "You're all caught up", "Approvals and questions show up here.")
        } else {
            View::empty("terminal", "No work running", "Threads you start in T3 Code show up here.")
        };
    }
    let mut children = Vec::new();
    if size == SizeClass::Large {
        let caption = if attention {
            format!("need your input · {}", plural(count, "thread"))
        } else {
            format!("running · {}", plural(count, "thread"))
        };
        children.push(View::text(count.to_string(), Role::Title1));
        children.push(View::tinted(caption, Role::Caption, Tone::Secondary));
    }
    for (top, agents) in &groups {
        let model = state
            .details
            .get(&top.id)
            .map(|d| d.model.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| top.provider_instance_id.clone());
        let working = agents.iter().filter(|a| a.running()).count();
        let agents_note = match agents.len() {
            0 => String::new(),
            n if attention => format!(" · {} needs you", plural(n, "agent")),
            n if working == n => format!(" · {} working", plural(n, "agent")),
            n => format!(" · {} ({working} working)", plural(n, "agent")),
        };
        children.push(View::ListRow {
            title: top.title.clone(),
            detail: Some(format!("{} · {model}{agents_note}", status(top))),
            icon: Some(if top.attention() { "message" } else { "terminal" }.into()),
            trailing: None,
            action: Some(format!("thread:{}", top.id)),
        });
        if attention
            && state.can_operate()
            && let Some(request) = top.pending_runtime_request.as_ref().filter(|r| r.approvable())
        {
            let target = format!("{}:{}", top.id, request.id);
            // Two equal halves under the row, as the native face.
            children.push(View::Grid {
                children: vec![
                    View::Button { label: "Approve".into(), action: format!("approve:{target}"), icon: Some("check".into()), primary: false },
                    View::Button { label: "Deny".into(), action: format!("deny:{target}"), icon: None, primary: false },
                ],
                columns: 2,
                gap: Some(6.0),
            });
        }
    }
    View::Stack { children, gap: Some(2.0) }
}

fn left(limit: &Limit) -> f32 {
    1.0 - limit.used_permille as f32 / 1000.0
}

fn epoch_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp_millis())
}

/// Seconds to the reset, and the fraction of the window still to run.
fn timing(limit: &Limit, now_ms: i64) -> (Option<i64>, Option<f32>) {
    let reset = limit.resets_at.as_deref().and_then(epoch_ms).map(|t| (t - now_ms).max(0) / 1000);
    let pace = reset
        .zip(limit.duration_mins.filter(|d| *d > 0))
        .map(|(secs, mins)| (secs as f32 / (mins as f32 * 60.0)).clamp(0.0, 1.0));
    (reset, pace)
}

fn countdown(secs: i64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn initials(account: &str) -> String {
    let name = account.split('@').next().unwrap_or(account);
    let mut parts = name.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty());
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => a.chars().take(1).chain(b.chars().take(1)).collect::<String>(),
        (Some(a), None) => a.chars().take(2).collect::<String>(),
        _ => "?".into(),
    }
    .to_uppercase()
}

/// The brand fill for a provider's bars (Claude's ember, Codex's graphite).
fn brand(driver: &str) -> Tone {
    Tone::Hex(
        match driver {
            d if d.contains("claude") => "#542e25",
            d if d.contains("codex") => "#606062",
            _ => "#46464a",
        }
        .into(),
    )
}

/// Per provider, one card per limit window, one bar per account, as T3's limits page.
fn limits(state: &State, size: SizeClass, now_ms: i64) -> View {
    if state.limits.is_empty() {
        return View::empty("gauge", "No limits reported", "T3 shows limits once a provider reports them.");
    }
    if size == SizeClass::Small {
        let tight = state.limits.iter().min_by(|a, b| left(a).total_cmp(&left(b))).expect("non-empty");
        return metric(
            format!("{:.0}%", left(tight) * 100.0),
            format!("{} · {} left", tight.provider, tight.window),
            Tone::Label,
        );
    }
    // Provider → window → accounts, in the order T3 reports them.
    let mut groups: Vec<(&str, &str, Vec<(&str, Vec<&Limit>)>)> = Vec::new();
    for limit in &state.limits {
        let i = match groups.iter().position(|g| g.0 == limit.driver) {
            Some(i) => i,
            None => {
                groups.push((&limit.driver, &limit.provider, Vec::new()));
                groups.len() - 1
            }
        };
        let windows = &mut groups[i].2;
        match windows.iter().position(|w| w.0 == limit.window) {
            Some(w) => windows[w].1.push(limit),
            None => windows.push((&limit.window, vec![limit])),
        }
    }
    let mut children = Vec::new();
    for (driver, provider, windows) in &groups {
        let dot = if driver.contains("claude") { Tone::Hex("#d97757".into()) } else { Tone::Label };
        children.push(View::Row {
            children: vec![
                View::Icon { icon: "dot".into(), size: Some(10.0), color: Some(dot) },
                View::text(*provider, Role::Headline),
            ],
            gap: Some(8.0),
        });
        for (window, accounts) in windows {
            let avg = accounts.iter().map(|a| left(a)).sum::<f32>() / accounts.len() as f32;
            // One account: the card's number says it all; several: a badge and percent each.
            let several = accounts.len() > 1;
            let bars = accounts.iter().map(|a| {
                let (reset, pace) = timing(a, now_ms);
                View::LimitBar {
                    left: left(a),
                    pace,
                    color: Some(brand(&a.driver)),
                    badge: several.then(|| initials(&a.account)),
                    label: several.then(|| format!("{:.0}%", left(a) * 100.0)),
                    reset: reset.map(countdown),
                }
            });
            let stat = View::Stack {
                children: vec![
                    View::tinted(*window, Role::Callout, Tone::Secondary),
                    View::text(format!("{:.0}% left", avg * 100.0), Role::Title1),
                ],
                gap: Some(2.0),
            };
            // Wide tiles: the stat beside the bars, accounts side by side (T3's limits page).
            let bars = View::Row { children: bars.collect(), gap: Some(8.0) };
            let card = if size == SizeClass::Large {
                View::Grid { children: vec![stat, View::Stack { children: vec![View::Spacer, bars, View::Spacer], gap: None }], columns: 2, gap: Some(14.0) }
            } else {
                View::Stack { children: vec![stat, bars], gap: Some(6.0) }
            };
            children.push(View::Card { title: None, child: Box::new(card) });
        }
    }
    View::Stack { children, gap: Some(10.0) }
}

/// `$16,113.46`
fn dollars(micro: u64) -> String {
    let cents = (micro + 5_000) / 10_000;
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("${grouped}.{:02}", cents % 100)
}

fn provider_name(provider: &str) -> &str {
    match provider {
        p if p.contains("claude") => "Claude Code",
        p if p.contains("codex") => "Codex",
        p => p,
    }
}

fn tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// 30-day cost per provider (largest first) with totals.
fn usage(state: &State, size: SizeClass) -> View {
    let Some(history) = &state.history else {
        let why = state.usage_notice.clone().unwrap_or_else(|| "Usage appears after T3 reports it.".into());
        return View::empty("activity", "No usage yet", why);
    };
    let total: u64 = history.providers.iter().flat_map(|p| &p.cost).sum();
    if size == SizeClass::Small {
        let sessions: u64 = history.providers.iter().map(|p| p.sessions).sum();
        return metric(dollars(total), format!("{sessions} sessions · 30 days"), Tone::Label);
    }
    let mut providers: Vec<_> = history.providers.iter().collect();
    providers.sort_by_key(|p| std::cmp::Reverse(p.cost.iter().sum::<u64>()));
    let sessions: u64 = providers.iter().map(|p| p.sessions).sum();
    let mut children = vec![
        View::text(dollars(total), Role::LargeTitle),
        View::tinted(format!("{sessions} sessions · 30 days"), Role::Caption, Tone::Secondary),
    ];
    let shown = if size == SizeClass::Large { providers.len() } else { 1 };
    for p in providers.iter().take(shown) {
        let cost: u64 = p.cost.iter().sum();
        let share = cost as f64 * 100.0 / total.max(1) as f64;
        children.push(View::ListRow {
            title: provider_name(&p.provider).into(),
            detail: Some(format!("{} sessions · {share:.1}% of cost · {} tokens", p.sessions, tokens(p.tokens.iter().sum()))),
            icon: None,
            trailing: Some(dollars(cost)),
            action: None,
        });
    }
    if size != SizeClass::Large {
        return View::Stack { children, gap: Some(4.0) };
    }
    let series = providers
        .iter()
        .map(|p| Series {
            label: p.provider.clone(),
            values: p.cost.iter().map(|c| *c as f32 / 1e6).collect(),
            color: Some(if p.provider.to_lowercase().contains("claude") { brand("claude") } else { brand("codex") }),
        })
        .collect();
    let day = |i: usize| history.days.get(i).map(|d| d.get(5..).unwrap_or(d).to_owned()).unwrap_or_default();
    let n = history.days.len();
    children.push(View::tinted("Daily cost", Role::Subheadline, Tone::Label));
    children.push(View::AreaChart { series, x_labels: [day(0), day(n / 2), day(n.saturating_sub(1))], unit: None });
    View::Stack { children, gap: Some(4.0) }
}

/// A New thread instance's picks; `None` is T3's first project / model.
#[derive(Clone, Debug, Default)]
pub struct Pick {
    pub project: Option<String>,
    pub model: Option<String>,
}

/// A model's `select` value.
pub fn model_key(model: &ModelChoice) -> String {
    format!("{}/{}", model.provider, model.model)
}

impl Pick {
    /// The project id and model a send would use: the picks if T3 still has them, else its first.
    pub fn resolve<'a>(&self, state: &'a State) -> (Option<&'a String>, Option<&'a ModelChoice>) {
        let project = self
            .project
            .as_ref()
            .and_then(|id| state.projects.get_key_value(id).map(|(k, _)| k))
            .or_else(|| state.projects.keys().next());
        let model = self
            .model
            .as_ref()
            .and_then(|key| state.models.iter().find(|m| model_key(m) == *key))
            .or_else(|| state.models.first());
        (project, model)
    }
}

/// Start a thread: project and model menus over the prompt field (the native composer).
fn composer(state: &State, size: SizeClass, pick: &Pick) -> View {
    if size == SizeClass::Small {
        return View::Stack {
            children: vec![
                View::text("Ready to build", Role::Headline),
                View::Field { placeholder: "Write a prompt".into(), action: "send".into() },
                View::Spacer,
            ],
            gap: Some(10.0),
        };
    }
    let (project, model) = pick.resolve(state);
    let projects = state
        .projects
        .iter()
        .map(|(id, name)| SelectOption { label: name.clone(), value: id.clone() })
        .collect();
    let models = state
        .models
        .iter()
        .map(|m| SelectOption { label: m.name.clone(), value: model_key(m) })
        .collect();
    let mut children = vec![
        View::Select { label: "No project".into(), action: "project".into(), options: projects, selected: project.cloned() },
        View::Select { label: "Default model".into(), action: "model".into(), options: models, selected: model.map(model_key) },
        View::Field { placeholder: "What should we build?".into(), action: "send".into() },
    ];
    if size == SizeClass::Large {
        children.push(View::tinted(
            "Starts in the selected project. T3 asks before edits or commands.",
            Role::Caption,
            Tone::Secondary,
        ));
    }
    children.push(View::Spacer);
    View::Stack { children, gap: Some(8.0) }
}
