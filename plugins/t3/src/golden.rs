//! View goldens: every face at every size class, for the main fixture (`golden/faces.json`) and
//! for the edge cases (`golden/edges.json`). The shell renders both into size sheets
//! (`plugin-t3-sizes.png`, `plugin-t3-edges.png`; crates/shell, `views::plugin_tests`).
//! `GLAZE_BLESS=1` rewrites.
use crate::t3::{self, History, Limit, ProviderHistory, Status, Thread, ThreadDetail};
use crate::{Availability, Data, Snapshot};
use glaze_plugin_sdk::SizeClass;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// 2026-11-05 08:00 UTC, the native fixture's clock.
const NOW: i64 = 1_793_875_200_000;

/// `mins` before [`NOW`], as T3 writes it.
fn before(mins: i64) -> String {
    chrono::DateTime::from_timestamp_millis(NOW - mins * 60_000).unwrap().to_rfc3339()
}

fn history() -> History {
    let days: Vec<String> = (7..=31)
        .map(|d| format!("2026-10-{d:02}"))
        .chain((1..=5).map(|d| format!("2026-11-{d:02}")))
        .collect();
    let wave = |phase: f64, scale: f64| -> Vec<u64> {
        (0..30).map(|i| ((f64::from(i) * 0.7 + phase).sin().abs() * scale) as u64).collect()
    };
    let provider = |name: &str, phase, cost, tokens, sessions| ProviderHistory {
        provider: name.into(),
        cost: wave(phase, cost),
        tokens: wave(phase, tokens),
        sessions,
        sessions_distinct: true,
    };
    History {
        days,
        providers: vec![
            provider("claudeAgent", 0.0, 600_000_000.0, 2_000_000_000.0, 386),
            provider("codex", 1.3, 250_000_000.0, 600_000_000.0, 636),
        ],
        uncached: 261_000_000,
        cached: 72_300_000_000,
        cache_write: 685_000_000,
        output: 201_000_000,
        savings_microusd: 271_003_400_000,
        cost_by_type: [0; 4],
    }
}

fn thread(id: &str, title: &str, status: Status, mins_ago: Option<i64>) -> Thread {
    Thread {
        id: id.into(),
        project_id: "paper".into(),
        title: title.into(),
        provider_instance_id: "codex".into(),
        status,
        latest_run_id: Some(format!("run-{id}")),
        active_run_id: Some(format!("run-{id}")),
        pending_runtime_request: None,
        updated_at: mins_ago.map(before).unwrap_or_else(|| "2026-11-05".into()),
        blocked_on_thread_wait: false,
        held: false,
        latest_run_completed_at: None,
        last_visited_at: None,
        settled: false,
        archived: false,
        snoozed_until: None,
    }
}

/// A thread that finished `mins_ago`, last opened in T3 `visited` minutes ago.
fn finished(id: &str, title: &str, status: Status, mins_ago: i64, visited: Option<i64>) -> Thread {
    Thread { latest_run_completed_at: Some(before(mins_ago)), last_visited_at: visited.map(before), ..thread(id, title, status, Some(mins_ago)) }
}

fn limit(driver: &str, provider: &str, account: &str, window: &str, used_permille: u32, resets_in_mins: Option<i64>) -> Limit {
    Limit {
        driver: driver.into(),
        provider: provider.into(),
        instance: format!("{driver}-{account}"),
        account: account.into(),
        window: window.into(),
        used_permille,
        resets_at: resets_in_mins.map(|m| before(-m)),
        duration_mins: Some(7 * 24 * 60),
    }
}

fn snapshot(availability: Availability, threads: Vec<Thread>, details: Vec<(&str, ThreadDetail)>, limits: Vec<Limit>, history: Option<History>) -> Snapshot {
    Snapshot {
        availability,
        data: Some(Data::T3(t3::Snapshot {
            threads,
            details: details.into_iter().map(|(id, d)| (id.into(), d)).collect(),
            projects: BTreeMap::from([("paper".into(), "Glaze".into())]),
            environment_id: Some("fixture".into()),
            models: vec![t3::ModelChoice { provider: "codex".into(), provider_name: "Codex".into(), model: "sol".into(), name: "GPT-6.1 Sol".into() }],
            permissions: vec!["orchestration:read".into(), "orchestration:operate".into(), "diagnostics:read".into()],
            usage_day: Some("2026-11-05".into()),
            history,
            limits,
            ..Default::default()
        })),
    }
}

fn model(m: &str) -> ThreadDetail {
    ThreadDetail { model: m.into(), ..Default::default() }
}

fn agent_of(parent: &str) -> ThreadDetail {
    ThreadDetail { model: "GPT-6.1 Sol".into(), parent_id: Some(parent.into()), agent: true, ..Default::default() }
}

/// The everyday state: an approval waiting, threads with subagents, recent completions, and the
/// user's own limits (Codex, and Claude reported for two accounts plus one duplicate).
pub fn fixture() -> Snapshot {
    let mut approval = thread("thread-0", "Review desktop permissions", Status::Waiting, Some(4));
    approval.pending_runtime_request = Some(t3::Request { id: "approval".into(), kind: "command".into() });
    let mut threads = vec![
        approval,
        thread("thread-1", "Redesign the T3 faces in Glaze's own style", Status::Running, Some(12)),
        thread("thread-2", "Fix half-size plugin layout", Status::Running, Some(25)),
        thread("thread-3", "Decode server.getConfig without a Value tree", Status::Starting, Some(1)),
        finished("thread-4", "Wire the dock magnification spring", Status::Completed, 18, Some(90)),
        finished("thread-5", "Port Discord notifications", Status::Failed, 64, None),
        finished("thread-read", "Already read, so not listed", Status::Completed, 30, Some(5)),
    ];
    threads.extend((0..4).map(|i| thread(&format!("agent-{i}"), &format!("Check size sheet cell {}", i + 1), Status::Running, Some(6))));
    threads.extend((6..14).map(|i| thread(&format!("thread-{i}"), &format!("Build native widget {i}"), Status::Running, Some(90 + i))));
    let mut details: Vec<(&str, ThreadDetail)> = vec![("thread-0", model("Opus 5.5")), ("thread-1", model("Opus 5.5")), ("thread-2", model("GPT-6.1 Sol")), ("thread-4", model("GPT-6.1 Sol"))];
    details.extend(["agent-0", "agent-1", "agent-2", "agent-3"].map(|id| (id, agent_of("thread-1"))));
    let days = |d: i64| Some(d * 24 * 60);
    let limits = vec![
        limit("codex", "Codex", "you@example.com", "Weekly", 730, Some(4 * 24 * 60 + 19 * 60)),
        limit("claudeAgent", "Claude", "you@example.com", "Weekly", 430, days(3)),
        limit("claudeAgent", "Claude", "you@example.com", "Weekly", 860, days(3)),
        limit("claudeAgent", "Claude", "you@example.com", "Weekly", 430, days(3)),
        limit("claudeAgent", "Claude", "you@example.com", "Weekly · Fable", 0, days(3)),
        limit("claudeAgent", "Claude", "you@example.com", "Weekly · Fable", 0, days(3)),
    ];
    snapshot(Availability::Live, threads, details, limits, Some(history()))
}

/// The edge cases, each a named snapshot.
pub fn edges() -> Vec<(&'static str, Snapshot)> {
    let long = "Investigate why the dock's liquid glass rim flickers on the secondary monitor after a DPI change while a fullscreen app is open";
    let one = snapshot(
        Availability::Live,
        vec![thread("solo", long, Status::Running, None)],
        vec![],
        vec![
            limit("claudeAgent", "Claude", "", "5-hour", 1000, Some(95)),
            limit("claudeAgent", "Claude", "", "Weekly", 880, Some(2 * 24 * 60)),
            limit("codex", "Codex", "", "Weekly", 0, None),
        ],
        None,
    );
    let mut many_limits = Vec::new();
    for (i, account) in ["a.very.long.address.for.work@an-extremely-long-company-domain.example", "alex@home.example", "alex@work.example", "ops@team.example", "ci@team.example", "bot@team.example"].iter().enumerate() {
        many_limits.push(limit("claudeAgent", "Claude", account, "Weekly", 150 + i as u32 * 140, Some(30 * (i as i64 + 1))));
    }
    // The same label twice with different numbers: two orgs behind one login.
    many_limits.push(limit("codex", "Codex", "you@example.com", "5-hour", 950, Some(40)));
    many_limits.push(limit("codex", "Codex", "you@example.com", "5-hour", 200, Some(40)));
    let many = snapshot(
        Availability::Live,
        (0..30).map(|i| thread(&format!("t{i}"), &format!("Thread number {i}"), if i % 7 == 0 { Status::Waiting } else { Status::Running }, Some(i * 3))).map(|mut t| {
            if t.status == Status::Waiting {
                t.pending_runtime_request = Some(t3::Request { id: "q".into(), kind: "user_input".into() });
            }
            t
        }).collect(),
        vec![],
        many_limits,
        Some(history()),
    );
    let mut stale = fixture();
    stale.availability = Availability::Stale("socket closed".into());
    // Every T3 sidebar state once (`Thread::pill`).
    let mut input = thread("s-input", "Asks which icon set to use", Status::Waiting, Some(3));
    input.pending_runtime_request = Some(t3::Request { id: "q".into(), kind: "user_input".into() });
    let mut approval = thread("s-approval", "Wants to run cargo build", Status::Waiting, Some(1));
    approval.pending_runtime_request = Some(t3::Request { id: "a".into(), kind: "command".into() });
    let blocked = Thread { blocked_on_thread_wait: true, ..thread("s-blocked", "Waits on another thread", Status::Running, Some(9)) };
    // Never shown: settled (even asking for input), snoozed, archived, and a settled thread's agent.
    let mut settled = Thread { settled: true, ..thread("x-settled", "HIDDEN settled but asking", Status::Waiting, Some(2)) };
    settled.pending_runtime_request = Some(t3::Request { id: "s".into(), kind: "user_input".into() });
    let snoozed = Thread { snoozed_until: Some(before(-60)), ..thread("x-snoozed", "HIDDEN snoozed", Status::Running, Some(2)) };
    let archived = Thread { archived: true, ..thread("x-archived", "HIDDEN archived", Status::Running, Some(2)) };
    let settled_done = Thread { settled: true, ..finished("x-done", "HIDDEN settled done unread", Status::Completed, 5, Some(60)) };
    let mut settled_agent = thread("x-agent", "HIDDEN agent of a settled thread", Status::Running, Some(1));
    settled_agent.pending_runtime_request = Some(t3::Request { id: "g".into(), kind: "command".into() });
    let states = snapshot(
        Availability::Live,
        vec![
            approval,
            input,
            thread("s-work", "Working on the dock spring", Status::Running, Some(7)),
            thread("s-wait", "Idle, its subagents still running", Status::Idle, Some(15)),
            thread("s-agent-1", "Subagent: render the sheet", Status::Running, Some(2)),
            thread("s-agent-2", "Subagent: check light mode", Status::Running, Some(4)),
            blocked,
            finished("s-done", "Finished, not read yet", Status::Completed, 12, Some(40)),
            finished("s-failed", "Failed, never opened", Status::Failed, 20, None),
            finished("s-read", "Finished and read: hidden", Status::Completed, 30, Some(2)),
            settled,
            snoozed,
            archived,
            settled_done,
            settled_agent,
        ],
        vec![("s-agent-1", agent_of("s-wait")), ("s-agent-2", agent_of("s-wait")), ("s-work", model("claude-opus-5-5")), ("x-agent", agent_of("x-settled"))],
        vec![],
        None,
    );
    vec![
        ("states", states),
        ("empty", snapshot(Availability::Live, vec![], vec![], vec![], None)),
        ("one", one),
        ("many", many),
        ("stale", stale),
        ("connecting", Snapshot { availability: Availability::Connecting, data: None }),
        ("unavailable", Snapshot { availability: Availability::Unavailable("T3 Code isn't running on this PC.".into()), data: None }),
    ]
}

const FACES: [&str; 5] = ["running", "usage", "limits", "attention", "new"];
const SIZES: [(&str, SizeClass); 3] = [("small", SizeClass::Small), ("medium", SizeClass::Medium), ("large", SizeClass::Large)];

fn sheet(snapshot: &Snapshot, faces: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for face in faces {
        let sizes: Map<String, Value> = SIZES
            .into_iter()
            .map(|(name, size)| (name.into(), json!(crate::faces::face(face, size, snapshot, &crate::faces::Pick::default(), NOW))))
            .collect();
        out.insert((*face).into(), sizes.into());
    }
    out
}

/// `{ face: { size: Face } }` for every face and size class.
pub fn faces() -> Value {
    sheet(&fixture(), &FACES).into()
}

/// `{ case: { face: { size: Face } } }` for the faces each edge case is about.
pub fn edge_faces() -> Value {
    let faces = ["running", "attention", "limits", "usage"];
    edges().into_iter().map(|(name, s)| (name.to_owned(), Value::from(sheet(&s, &faces)))).collect::<Map<_, _>>().into()
}

fn check(name: &str, actual: Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden").join(name);
    let actual = serde_json::to_string_pretty(&actual).unwrap() + "\n";
    if std::env::var_os("GLAZE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let golden = std::fs::read_to_string(&path).expect("run with GLAZE_BLESS=1 to write the golden");
    assert_eq!(golden.replace("\r\n", "\n"), actual, "T3 faces changed; check the size sheets and bless");
}

#[test]
fn faces_match_the_view_golden() {
    check("faces.json", faces());
    check("edges.json", edge_faces());
}

#[test]
fn thread_states_follow_t3() {
    use crate::t3::Pill;
    let (_, states) = edges().into_iter().find(|e| e.0 == "states").unwrap();
    let Some(Data::T3(state)) = &states.data else { unreachable!() };
    let pill = |id: &str, busy| state.threads.iter().find(|t| t.id == id).unwrap().pill(busy);
    assert_eq!(pill("s-approval", false), Some(Pill::Approval));
    assert_eq!(pill("s-input", false), Some(Pill::Input));
    assert_eq!(pill("s-work", false), Some(Pill::Working));
    assert_eq!(pill("s-wait", true), Some(Pill::Waiting), "idle while its subagents run");
    assert_eq!(pill("s-wait", false), None);
    assert_eq!(pill("s-blocked", false), Some(Pill::Waiting));
    assert_eq!(pill("s-done", false), Some(Pill::Done), "finished after the last visit");
    assert_eq!(pill("s-read", false), None, "read in T3: off the list");
    assert_eq!(pill("s-failed", false), Some(Pill::Failed));
    let running = serde_json::to_string(&edge_faces()["states"]["running"]["large"]).unwrap();
    assert!(running.contains("Finished, not read yet") && !running.contains("Finished and read"), "{running}");
    assert!(running.contains("\"tag\":\"Waiting\""), "{running}");
}

/// Settled, snoozed and archived threads (and a settled thread's agent) appear in no face, size
/// or count, the bar module included.
#[test]
fn settled_snoozed_and_archived_threads_never_show() {
    let (_, states) = edges().into_iter().find(|e| e.0 == "states").unwrap();
    for face in ["running", "attention", "status"] {
        for (_, size) in SIZES {
            let json = serde_json::to_string(&crate::faces::face(face, size, &states, &crate::faces::Pick::default(), NOW)).unwrap();
            assert!(!json.contains("HIDDEN"), "{face}/{size:?}: {json}");
            if face == "status" {
                // 2 need input (approval, input), 3 running (working, blocked, waiting on agents).
                assert!(json.contains("3 running · 2 need input"), "{json}");
            }
        }
    }
    let small = serde_json::to_string(&edge_faces()["states"]["attention"]["small"]).unwrap();
    assert!(small.contains("\"text\":\"2\""), "{small}");
}

#[test]
fn identical_reports_collapse_and_duplicate_labels_stay_apart() {
    let json = serde_json::to_string(&faces()["limits"]["large"]).unwrap();
    // Two Claude accounts by their names (never initials), the duplicate report folded away.
    assert_eq!(json.matches("Weekly · snowy7").count(), 1, "{json}");
    assert_eq!(json.matches("Weekly · sn\"").count(), 1, "{json}");
    let edges = edge_faces();
    let many = serde_json::to_string(&edges["many"]["limits"]["large"]).unwrap();
    assert!(many.contains("snowy7 (2)"), "{many}");
    assert!(many.contains("4 more accounts"), "{many}");
}
