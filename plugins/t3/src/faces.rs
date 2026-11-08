//! The T3 faces as declarative views: Running now, Needs you, Limits, Usage, New thread.
//! Drawn in Glaze's own calm style with the kit (no copied T3 chrome): monochrome rows and
//! meters, colour only for state, totals in the footer so rows that don't fit are still counted.

use crate::t3::{Limit, ModelChoice, Pill, Snapshot as State, Thread};
use crate::{Availability, Data, Snapshot};
use glaze_plugin_sdk::view::SelectOption;
use glaze_plugin_sdk::{Face, Role, Series, SizeClass, Tone, View};

/// `face` at `size` with the default options (fixtures and benches).
pub fn face(face: &str, size: SizeClass, snapshot: &Snapshot, pick: &Pick, now_ms: i64) -> Face {
    face_with(face, size, snapshot, pick, &Options::default(), now_ms)
}

/// A Running now / Needs you instance's options (`options` in plugin.json; the widget's
/// right-click Options… and Studio edit them, and they arrive in `face.open` / `face.options`).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The subagent rows under each thread (the thread's line still counts them).
    pub show_subagents: bool,
    /// Running now keeps a finished thread, tagged Done, until it's read in T3.
    pub show_finished: bool,
    /// Rows listed at most; the footer still counts every thread.
    pub max_items: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { show_subagents: true, show_finished: true, max_items: 20 }
    }
}

impl Options {
    pub fn from_settings(settings: &serde_json::Value) -> Self {
        let d = Self::default();
        Self {
            show_subagents: settings["show-subagents"].as_bool().unwrap_or(d.show_subagents),
            show_finished: settings["show-finished"].as_bool().unwrap_or(d.show_finished),
            max_items: settings["max-items"].as_f64().map_or(d.max_items, |n| n.clamp(1.0, 200.0) as usize),
        }
    }
}

/// `face` at `size` from the provider's snapshot; `pick` is the New thread instance's choices.
pub fn face_with(face: &str, size: SizeClass, snapshot: &Snapshot, pick: &Pick, options: &Options, now_ms: i64) -> Face {
    let mut summary = None;
    let body = match &snapshot.data {
        Some(Data::T3(state)) => match face {
            // The top-bar module: the bar draws its text.
            // The top-bar module counts what the lists show (never settled or snoozed threads).
            "status" => {
                let groups = groups(state, now_ms);
                let need = groups.iter().filter(|g| rank(g.1) == 0).count();
                let running = groups.iter().filter(|g| rank(g.1) == 1).count();
                View::text(format!("{running} running · {need} need input"), Role::Caption)
            }
            "limits" => limits(state, size, now_ms),
            "usage" => usage(state, size),
            "new" => composer(state, size, pick),
            "attention" => threads(state, size, true, options, now_ms, &mut summary),
            _ => threads(state, size, false, options, now_ms, &mut summary),
        },
        None if face == "status" => View::text("T3", Role::Caption),
        None => match &snapshot.availability {
            Availability::Unavailable(why) => View::Stack {
                children: vec![View::empty("terminal", "Connect T3 Code", why.clone()), View::button("Retry", "retry")],
                gap: None,
            },
            _ => View::empty("terminal", "Connecting…", "Waiting for your local T3 Code"),
        },
    };
    // Without data the body already says why; the footer doesn't repeat it.
    let footer = snapshot.data.is_some().then(|| footer(face, snapshot, summary));
    Face { body, footer, refresh: true }
}

/// The chrome's caption: what the face counts (when live), or how stale it is.
fn footer(face: &str, snapshot: &Snapshot, summary: Option<String>) -> String {
    match &snapshot.availability {
        Availability::Live | Availability::Cached => match (face, &snapshot.data) {
            ("usage", Some(Data::T3(state))) => state.usage_day.as_ref().map_or("Last 30 days".into(), |day| format!("30 days to {day} · UTC")),
            _ => summary.unwrap_or_else(|| "Live from T3 Code".into()),
        },
        Availability::Stale(_) => "Disconnected · showing last state".into(),
        Availability::Connecting => "Connecting…".into(),
        Availability::Unavailable(why) => why.clone(),
    }
}

/// A thread's T3 state as a short tag in the theme's palette: neutral while working or
/// waiting on its agents, amber when it waits on you, red or green once it ends unread.
fn status(pill: Pill) -> (&'static str, Tone) {
    match pill {
        Pill::Approval => ("Approval needed", Tone::Warning),
        Pill::Input => ("Needs input", Tone::Warning),
        Pill::Working => ("Working", Tone::Secondary),
        Pill::Connecting => ("Starting", Tone::Secondary),
        Pill::Waiting => ("Waiting", Tone::Tertiary),
        Pill::Failed => ("Failed", Tone::Destructive),
        Pill::Done => ("Done", Tone::Positive),
    }
}

/// Needs you first, then what's working, then what finished unread; newest first in each.
fn rank(pill: Pill) -> u8 {
    match pill {
        Pill::Approval | Pill::Input => 0,
        Pill::Working | Pill::Connecting | Pill::Waiting => 1,
        Pill::Failed | Pill::Done => 2,
    }
}

/// `12m`, `3h`, `2d`: how long ago `at` was.
fn ago(now_ms: i64, at: i64) -> String {
    let mins = (now_ms - at).max(0) / 60_000;
    match mins {
        0 => "<1m".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 48 * 60 => format!("{}h", m / 60),
        m => format!("{}d", m / (24 * 60)),
    }
}

fn provider_name(provider: &str) -> &str {
    match provider {
        p if p.to_lowercase().contains("claude") => "Claude Code",
        p if p.to_lowercase().contains("codex") => "Codex",
        p => p,
    }
}

/// A model slug as people say it: `claude-opus-5-5` → `Opus 5.5`, `gpt-6.1-sol` → `GPT-6.1 Sol`.
/// Names that are already readable (spaces, capitals) pass through.
fn model_name(slug: &str) -> String {
    if slug.contains(' ') || slug.chars().any(char::is_uppercase) {
        return slug.into();
    }
    let rest = slug.strip_prefix("claude-").unwrap_or(slug);
    let mut words: Vec<String> = Vec::new();
    for part in rest.split('-').filter(|p| !p.is_empty() && !(p.len() >= 8 && p.chars().all(|c| c.is_ascii_digit()))) {
        match words.last_mut() {
            // Version digits join with dots: 5-5 → 5.5.
            Some(last) if part.chars().all(|c| c.is_ascii_digit()) && last.chars().last().is_some_and(|c| c.is_ascii_digit()) => {
                last.push('.');
                last.push_str(part);
            }
            _ if part == "gpt" => words.push("GPT".into()),
            _ => {
                let mut chars = part.chars();
                words.push(chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default());
            }
        }
    }
    // `GPT` and its version read as one word.
    match words.as_slice() {
        [first, version, ..] if first == "GPT" && version.starts_with(|c: char| c.is_ascii_digit()) => {
            format!("GPT-{}", words[1..].join(" "))
        }
        _ => words.join(" "),
    }
}

/// The quiet second line: model · how long, each part only when T3 knows it.
fn detail(state: &State, thread: &Thread, pill: Pill, agents: usize, now_ms: i64) -> Option<String> {
    let model = state
        .details
        .get(&thread.id)
        .map(|d| model_name(&d.model))
        .filter(|m| !m.is_empty())
        .or_else(|| Some(provider_name(&thread.provider_instance_id).to_owned()).filter(|p| !p.is_empty()));
    let time = epoch_ms(&thread.updated_at).map(|at| {
        let span = ago(now_ms, at);
        match pill {
            Pill::Approval | Pill::Input | Pill::Waiting => format!("waiting {span}"),
            Pill::Working | Pill::Connecting => format!("running {span}"),
            Pill::Failed | Pill::Done => format!("{span} ago"),
        }
    });
    let agents = (agents > 0).then(|| plural(agents, "agent"));
    let parts: Vec<String> = [model, time, agents].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// One thread as a whole-row target that opens it in T3.
fn item(thread: &Thread, pill: Pill, detail: Option<String>, nested: bool) -> View {
    let (tag, tone) = status(pill);
    View::Item {
        title: if thread.title.trim().is_empty() { "Untitled thread".into() } else { thread.title.clone() },
        detail,
        tag: Some(tag.into()),
        tone: Some(tone),
        action: Some(format!("thread:{}", thread.id)),
        nested,
    }
}

/// Subagents listed under their thread on large tiles; the rest are counted.
const AGENTS_SHOWN: usize = 3;

/// A big number over a caption: the compact tiles.
fn metric(value: String, caption: String, tone: Tone) -> View {
    View::Stack {
        children: vec![View::Spacer, View::tinted(value, Role::LargeTitle, tone), View::tinted(caption, Role::Caption, Tone::Secondary), View::Spacer],
        gap: Some(2.0),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn parent<'a>(state: &'a State, thread: &Thread) -> Option<&'a str> {
    state.details.get(&thread.id).and_then(|d| d.parent_id.as_deref())
}

/// Running now / Needs you: top-level threads with their subagents grouped under them. Each
/// state is T3's own (`Thread::pill`): Running now keeps a finished thread, tagged Done or
/// Failed, until it's read in T3; Needs you lists approvals and questions.
/// A listed thread, its T3 state, and its live subagents.
type Group<'a> = (&'a Thread, Pill, Vec<(&'a Thread, Pill)>);

/// Every thread a list or count may show, grouped and ordered. Settled, archived and snoozed
/// threads never appear (nor their subagents), whatever they wait for. One pass over the
/// threads: every redraw (a click included) runs this, so it stays linear in the thread count.
pub(crate) fn groups(state: &State, now_ms: i64) -> Vec<Group<'_>> {
    let mut tops: Vec<&Thread> = Vec::new();
    let mut agents_of: std::collections::HashMap<&str, Vec<(&Thread, Pill)>> = std::collections::HashMap::new();
    for thread in state.threads.iter().filter(|t| !t.hidden(now_ms)) {
        match parent(state, thread) {
            None => tops.push(thread),
            Some(parent) => {
                if let Some(pill) = thread.pill(false).filter(|p| rank(*p) < 2) {
                    agents_of.entry(parent).or_default().push((thread, pill));
                }
            }
        }
    }
    let mut groups: Vec<Group<'_>> = tops
        .into_iter()
        .filter_map(|top| {
            let agents = agents_of.remove(top.id.as_str()).unwrap_or_default();
            // A subagent asking for something makes its thread need you, as in T3.
            let asking = agents.iter().map(|a| a.1).find(|p| rank(*p) == 0);
            let pill = asking.filter(|_| top.pill(false).is_none_or(|p| rank(p) > 0)).or_else(|| top.pill(!agents.is_empty()))?;
            Some((top, pill, agents))
        })
        .collect();
    groups.sort_by(|a, b| rank(a.1).cmp(&rank(b.1)).then_with(|| b.0.updated_at.cmp(&a.0.updated_at)));
    groups
}

fn threads(state: &State, size: SizeClass, attention: bool, options: &Options, now_ms: i64, summary: &mut Option<String>) -> View {
    let mut groups = groups(state, now_ms);
    if attention {
        groups.retain(|g| rank(g.1) == 0);
    } else if !options.show_finished {
        groups.retain(|g| g.1 != Pill::Done);
    }
    let agents: usize = groups.iter().map(|g| g.2.len()).sum();
    let count = groups.len();
    let active = groups.iter().filter(|g| rank(g.1) < 2).count();
    let unread = count - active;
    if count > 0 {
        let mut parts = vec![if attention { format!("{} need you", plural(count, "thread")) } else { format!("{active} running") }];
        if agents > 0 && !attention {
            parts.push(plural(agents, "agent"));
        }
        if unread > 0 {
            parts.push(format!("{unread} unread"));
        }
        *summary = Some(parts.join(" · "));
    }
    if size == SizeClass::Small {
        let (value, caption) = if attention {
            (count, "need you".to_owned())
        } else if unread > 0 && active == 0 {
            (unread, "done · unread".to_owned())
        } else {
            (active, if agents > 0 { format!("running · {}", plural(agents, "agent")) } else { "running".to_owned() })
        };
        let tone = if attention && count > 0 { Tone::Warning } else { Tone::Label };
        return metric(value.to_string(), caption, tone);
    }
    if count == 0 {
        return if attention {
            View::empty("check", "All caught up", "Approvals and questions show up here.")
        } else {
            View::empty("terminal", "Nothing running", "Threads you start in T3 Code show up here.")
        };
    }
    let mut rows = Vec::new();
    for (top, pill, agents) in groups.iter().take(options.max_items) {
        rows.push(item(top, *pill, detail(state, top, *pill, agents.len(), now_ms), false));
        // Large tiles list the agents under their thread; the rest are counted in the line above.
        if options.show_subagents && size == SizeClass::Large {
            for (agent, pill) in agents.iter().take(AGENTS_SHOWN) {
                rows.push(item(agent, *pill, None, true));
            }
        }
        if attention
            && state.can_operate()
            && let Some(request) = top.pending_runtime_request.as_ref().filter(|r| r.approvable())
        {
            let target = format!("{}:{}", top.id, request.id);
            rows.push(View::Grid {
                children: vec![
                    View::Button { label: "Approve".into(), action: format!("approve:{target}"), icon: Some("check".into()), primary: false },
                    View::Button { label: "Deny".into(), action: format!("deny:{target}"), icon: None, primary: false },
                ],
                columns: 2,
                gap: Some(6.0),
            });
        }
    }
    // One list scrolling under the fixed chrome (the shell scrolls it; the position is kept
    // per face); the footer keeps the full count.
    let list = View::List { children: rows, gap: Some(2.0) };
    View::Scroll { id: if attention { "needs-you" } else { "running" }.into(), child: Box::new(list), on_reach_end: None }
}

fn left(limit: &Limit) -> f32 {
    (1.0 - limit.used_permille as f32 / 1000.0).clamp(0.0, 1.0)
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
    if d > 0 && h == 0 {
        format!("{d}d")
    } else if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 && m == 0 {
        format!("{h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// Colour only for state: amber under 20 % left, red when spent, neutral otherwise.
fn state_tone(left: f32) -> Option<Tone> {
    if left <= 0.0 {
        Some(Tone::Destructive)
    } else if left < 0.2 {
        Some(Tone::Warning)
    } else {
        None
    }
}

/// A provider's small brand dot (Claude's ember; others neutral).
fn brand_dot(driver: &str) -> Tone {
    if driver.to_lowercase().contains("claude") { Tone::Hex("#d97757".into()) } else { Tone::Secondary }
}

/// Meters listed per provider window before the rest are summarised in one line.
const ACCOUNTS_SHOWN: usize = 3;

/// One meter per limit window: "% left", one clean bar, the reset as plain text.
fn meter(limit: &Limit, label: String, now_ms: i64) -> View {
    let (reset, pace) = timing(limit, now_ms);
    let left = left(limit);
    let note = match reset {
        Some(_) if left >= 1.0 => Some("full".into()),
        Some(secs) => Some(format!("resets in {}", countdown(secs))),
        None => None,
    };
    View::Meter {
        value: format!("{:.0}% left", left * 100.0),
        label,
        note,
        fraction: left,
        color: state_tone(left),
        // Only when it says something: burning faster than an even pace.
        mark: pace.filter(|p| left > 0.0 && left + 0.02 < *p),
    }
}

/// Each account's name as shown: the email's local part when that alone tells them apart,
/// the full address when it doesn't, and a number when even that repeats.
fn account_names(accounts: &[&Limit]) -> Vec<String> {
    let name = |l: &Limit| {
        let a = l.account.trim();
        if a.is_empty() { l.instance.clone() } else { a.to_owned() }
    };
    let full: Vec<String> = accounts.iter().map(|l| name(l)).collect();
    let local: Vec<String> = full.iter().map(|a| a.split('@').next().unwrap_or(a).to_owned()).collect();
    // The short name is enough when no two different addresses share it.
    let short = (0..full.len()).all(|i| (0..i).all(|j| full[i] == full[j] || local[i] != local[j]));
    let names = if short { local } else { full };
    let mut seen: Vec<String> = Vec::new();
    names
        .into_iter()
        .map(|n| {
            let repeat = seen.iter().filter(|s| **s == n).count();
            seen.push(n.clone());
            if repeat == 0 { n } else { format!("{n} ({})", repeat + 1) }
        })
        .collect()
}

/// Per provider: a brand dot and name, then a meter per window (per account when there are
/// several), identical reports collapsed and long account lists summarised.
fn limits(state: &State, size: SizeClass, now_ms: i64) -> View {
    // Identical reports (the same account seen twice) collapse into one.
    let mut all: Vec<&Limit> = Vec::new();
    for limit in &state.limits {
        let same = |l: &&Limit| l.driver == limit.driver && l.account == limit.account && l.window == limit.window && l.used_permille == limit.used_permille && l.resets_at == limit.resets_at;
        if !all.iter().any(same) {
            all.push(limit);
        }
    }
    if all.is_empty() {
        return View::empty("gauge", "No limits reported", "Limits show once a provider reports them.");
    }
    if size == SizeClass::Small {
        let tight = all.iter().min_by(|a, b| left(a).total_cmp(&left(b))).expect("non-empty");
        let left = left(tight);
        return metric(format!("{:.0}%", left * 100.0), format!("{} · {} left", tight.provider, tight.window), state_tone(left).unwrap_or(Tone::Label));
    }
    // Provider → window → accounts, in the order T3 reports them.
    let mut providers: Vec<(&str, &str, Vec<(&str, Vec<&Limit>)>)> = Vec::new();
    for limit in all {
        let i = providers.iter().position(|g| g.0 == limit.driver).unwrap_or_else(|| {
            providers.push((&limit.driver, &limit.provider, Vec::new()));
            providers.len() - 1
        });
        let windows = &mut providers[i].2;
        match windows.iter().position(|w| w.0 == limit.window) {
            Some(w) => windows[w].1.push(limit),
            None => windows.push((&limit.window, vec![limit])),
        }
    }
    // The most urgent provider first, so a short tile shows what's running out.
    let tightest = |windows: &Vec<(&str, Vec<&Limit>)>| windows.iter().flat_map(|w| &w.1).map(|l| left(l)).fold(1.0_f32, f32::min);
    providers.sort_by(|a, b| tightest(&a.2).total_cmp(&tightest(&b.2)));
    let mut rows = Vec::new();
    for (driver, provider, windows) in &providers {
        let name = if provider.trim().is_empty() { provider_name(driver) } else { provider };
        let header = View::Row {
            children: vec![View::Icon { icon: "dot".into(), size: Some(8.0), color: Some(brand_dot(driver)) }, View::tinted(name, Role::Subheadline, Tone::Secondary)],
            gap: Some(6.0),
        };
        let first = rows.len();
        for (window, accounts) in windows {
            let window = if window.trim().is_empty() { "Limit" } else { window };
            // Every account at the same reading: one row says it for all of them.
            if accounts.len() > 1 && accounts.iter().all(|a| a.used_permille == accounts[0].used_permille && a.resets_at == accounts[0].resets_at) {
                rows.push(meter(accounts[0], format!("{window} · {} accounts", accounts.len()), now_ms));
                continue;
            }
            let names = account_names(accounts);
            let mut shown: Vec<(&Limit, String)> = accounts.iter().copied().zip(names).collect();
            // Tightest first when there are several, so a summary hides only the roomy ones.
            shown.sort_by(|a, b| left(a.0).total_cmp(&left(b.0)));
            let several = shown.len() > 1;
            let rest = shown.split_off(shown.len().min(if shown.len() > ACCOUNTS_SHOWN { ACCOUNTS_SHOWN - 1 } else { ACCOUNTS_SHOWN }));
            for (limit, account) in shown {
                let label = if several { format!("{window} · {account}") } else { window.to_owned() };
                rows.push(meter(limit, label, now_ms));
            }
            if let Some(low) = rest.iter().map(|r| left(r.0)).min_by(f32::total_cmp) {
                let more = match rest.len() {
                    1 => format!("1 more account, {:.0}% left", low * 100.0),
                    n => format!("{n} more accounts, all {:.0}% left or more", low * 100.0),
                };
                rows.push(View::tinted(more, Role::Footnote, Tone::Tertiary));
            }
        }
        // The header travels with its first row: a short tile never ends on a bare name.
        let lead = rows.remove(first);
        rows.insert(first, View::Stack { children: vec![header, lead], gap: Some(6.0) });
    }
    // Large tiles spread the meters over their height (macOS's large widgets) instead of
    // leaving the bottom empty; the spacers shrink to nothing when the rows fill it.
    if size == SizeClass::Large && rows.len() > 2 {
        rows = rows.into_iter().flat_map(|row| [row, View::Spacer]).collect();
        rows.pop();
    }
    View::List { children: rows, gap: Some(10.0) }
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

fn tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// 30-day cost: the total, the daily chart taking the free height, then a row per provider.
fn usage(state: &State, size: SizeClass) -> View {
    let Some(history) = &state.history else {
        let why = state.usage_notice.clone().unwrap_or_else(|| "Usage appears after T3 reports it.".into());
        return View::empty("activity", "No usage yet", why);
    };
    let total: u64 = history.providers.iter().flat_map(|p| &p.cost).sum();
    let sessions: u64 = history.providers.iter().map(|p| p.sessions).sum();
    if size == SizeClass::Small {
        return metric(dollars(total), format!("{sessions} sessions · 30 days"), Tone::Label);
    }
    let mut providers: Vec<_> = history.providers.iter().collect();
    providers.sort_by_key(|p| std::cmp::Reverse(p.cost.iter().sum::<u64>()));
    let mut children = vec![View::Stack {
        children: vec![View::text(dollars(total), Role::LargeTitle), View::tinted(format!("{sessions} sessions · 30 days"), Role::Footnote, Tone::Secondary)],
        gap: Some(0.0),
    }];
    if size == SizeClass::Large && history.days.len() > 1 {
        let series = providers
            .iter()
            .map(|p| Series {
                label: provider_name(&p.provider).into(),
                values: p.cost.iter().map(|c| *c as f32 / 1e6).collect(),
                color: Some(if p.provider.to_lowercase().contains("claude") { Tone::Hex("#d97757".into()) } else { Tone::Label }),
            })
            .collect();
        let day = |i: usize| history.days.get(i).map(|d| d.get(5..).unwrap_or(d).to_owned()).unwrap_or_default();
        let n = history.days.len();
        children.push(View::AreaChart { series, x_labels: [day(0), day(n / 2), day(n - 1)], unit: None });
    }
    let shown = if size == SizeClass::Large { 3 } else { 2 };
    for p in providers.iter().take(shown) {
        let cost: u64 = p.cost.iter().sum();
        children.push(View::ListRow {
            title: provider_name(&p.provider).into(),
            detail: Some(format!("{} sessions · {} tokens", p.sessions, tokens(p.tokens.iter().sum()))),
            icon: None,
            trailing: Some(dollars(cost)),
            action: None,
        });
    }
    if size != SizeClass::Large {
        children.push(View::Spacer);
    }
    View::Stack { children, gap: Some(6.0) }
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
            children: vec![View::text("Ready to build", Role::Headline), View::Field { placeholder: "Write a prompt".into(), action: "send".into() }, View::Spacer],
            gap: Some(10.0),
        };
    }
    let (project, model) = pick.resolve(state);
    let projects = state.projects.iter().map(|(id, name)| SelectOption { label: name.clone(), value: id.clone() }).collect();
    let models = state.models.iter().map(|m| SelectOption { label: m.name.clone(), value: model_key(m) }).collect();
    let mut children = vec![
        View::Select { label: "No project".into(), action: "project".into(), options: projects, selected: project.cloned() },
        View::Select { label: "Default model".into(), action: "model".into(), options: models, selected: model.map(model_key) },
        View::Field { placeholder: "What should we build?".into(), action: "send".into() },
    ];
    if size == SizeClass::Large {
        children.push(View::tinted("Starts in the selected project. T3 asks before edits or commands.", Role::Caption, Tone::Secondary));
    }
    children.push(View::Spacer);
    View::Stack { children, gap: Some(8.0) }
}

#[test]
fn model_slugs_read_as_names() {
    for (slug, name) in [
        ("claude-opus-5-5", "Opus 5.5"),
        ("claude-sonnet-5-5-20260101", "Sonnet 5.5"),
        ("gpt-6.1-sol", "GPT-6.1 Sol"),
        ("GPT-6.1 Sol", "GPT-6.1 Sol"),
        ("o3", "O3"),
    ] {
        assert_eq!(model_name(slug), name);
    }
}
