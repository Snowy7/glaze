//! View goldens: every face at every size class from the same fixture the shell's native T3
//! faces were checked with. `golden/faces.json` is what the plugin sends; the shell renders it
//! into `plugin-t3-sizes.png` (crates/shell, `views::plugin_tests`). `MYPAPER_BLESS=1` rewrites.
use crate::t3::{self, History, Limit, ProviderHistory, Status, Thread, ThreadDetail};
use crate::{Availability, Data, Snapshot};
use mypaper_plugin_sdk::SizeClass;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// 2026-11-05 08:00 UTC, the native fixture's clock.
const NOW: i64 = 1_793_875_200_000;

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

pub fn fixture() -> Snapshot {
    let threads: Vec<Thread> = (0..20)
        .map(|i| Thread {
            id: format!("thread-{i}"),
            project_id: "paper".into(),
            title: if i == 0 { "Review desktop permissions".into() } else { format!("Build native widget {}", i + 1) },
            provider_instance_id: "codex".into(),
            status: if i == 0 { Status::Waiting } else { Status::Running },
            latest_run_id: Some(format!("run-{i}")),
            active_run_id: Some(format!("run-{i}")),
            pending_runtime_request: (i == 0).then(|| t3::Request { id: "approval".into(), kind: "command".into() }),
            updated_at: "2026-11-05".into(),
        })
        .collect();
    let details = threads
        .iter()
        .map(|t| (t.id.clone(), ThreadDetail { model: "GPT-6.1 Sol".into(), agent: t.id != "thread-0", ..Default::default() }))
        .collect();
    let limit = |driver: &str, provider: &str, account: &str, window: &str, used_permille, resets: &str| Limit {
        driver: driver.into(),
        provider: provider.into(),
        instance: account.into(),
        account: account.into(),
        window: window.into(),
        used_permille,
        resets_at: Some(resets.into()),
        duration_mins: Some(7 * 24 * 60),
    };
    let mut limits = vec![limit("codex", "Codex", "alex@example.test", "Weekly", 660, "2026-11-10T12:00:00.000Z")];
    for (account, used) in [("one@example.test", 430), ("two@example.test", 290)] {
        for (window, used) in [("Weekly", used), ("Weekly · Fable", 0)] {
            limits.push(limit("claudeAgent", "Claude", account, window, used, "2026-11-09T12:00:00.000Z"));
        }
    }
    Snapshot {
        revision: 1,
        availability: Availability::Live,
        data: Some(Data::T3(t3::Snapshot {
            threads,
            details,
            projects: BTreeMap::from([("paper".into(), "mypaper".into())]),
            environment_id: Some("fixture".into()),
            models: vec![t3::ModelChoice {
                provider: "codex".into(),
                provider_name: "Codex".into(),
                model: "sol".into(),
                name: "GPT-6.1 Sol".into(),
            }],
            usage_day: Some("2026-11-05".into()),
            history: Some(history()),
            limits,
            ..Default::default()
        })),
    }
}

/// `{ face: { size: Face } }` for every face and size class.
pub fn faces() -> Value {
    let snapshot = fixture();
    let mut out = Map::new();
    for face in ["running", "usage", "limits", "attention", "new"] {
        let sizes: Map<String, Value> = [("small", SizeClass::Small), ("medium", SizeClass::Medium), ("large", SizeClass::Large)]
            .into_iter()
            .map(|(name, size)| (name.into(), json!(crate::faces::face(face, size, &snapshot, NOW))))
            .collect();
        out.insert(face.into(), sizes.into());
    }
    out.into()
}

#[test]
fn faces_match_the_view_golden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
    let actual = serde_json::to_string_pretty(&faces()).unwrap() + "\n";
    if std::env::var_os("MYPAPER_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let golden = std::fs::read_to_string(&path).expect("run with MYPAPER_BLESS=1 to write the golden");
    assert_eq!(golden.replace("\r\n", "\n"), actual, "T3 faces changed; check plugin-t3-sizes.png and bless");
}
