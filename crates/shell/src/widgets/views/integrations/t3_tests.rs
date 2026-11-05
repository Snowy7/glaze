//! Production renderer size sheets and dense-size interaction bounds for the T3/Grok faces.
use crate::widgets::{model::Model, time::Stamp, views};
use mypaper_integrations::{Availability, Data, Snapshot, config, grok, launcher, t3};
use mypaper_ui::{DisplayList, Image, Op, Rect, Rgba, TextStyle, Tokens, Ui, golden, render};
use mypaper_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

const NOW: i64 = 1_793_875_200_000;
fn fixture(id: &str, variant: &str) -> Model {
    let placed = Placed {
        instance: format!("{id}-{variant}"),
        widget: id.into(),
        cell: [0, 0],
        size: [8, 5],
        settings: Map::from_iter([("variant".into(), json!(variant))]),
        appearance: Map::new(),
        own: Map::new(),
    };
    let mut model = Model::new(
        placed,
        &mut Storage::new(std::env::temp_dir().join("mypaper-t3-grok-fixture")),
        Stamp::at(NOW),
    )
    .unwrap();
    model.visible = true;
    let threads: Vec<_> = (0..20)
        .map(|i| t3::Thread {
            id: format!("thread-{i}"),
            project_id: "paper".into(),
            title: if i == 0 {
                "Review desktop permissions".into()
            } else {
                format!("Build native widget {}", i + 1)
            },
            provider_instance_id: "codex".into(),
            status: if i == 0 {
                t3::Status::Waiting
            } else {
                t3::Status::Running
            },
            latest_run_id: Some(format!("run-{i}")),
            active_run_id: Some(format!("run-{i}")),
            pending_runtime_request: if i == 0 {
                Some(t3::Request {
                    id: "approval".into(),
                    kind: "command".into(),
                })
            } else {
                None
            },
            updated_at: "2026-11-05".into(),
        })
        .collect();
    let details = threads
        .iter()
        .map(|thread| {
            (
                thread.id.clone(),
                t3::ThreadDetail {
                    model: "GPT-6.1 Sol".into(),
                    agent: thread.id != "thread-0",
                    ..Default::default()
                },
            )
        })
        .collect();
    model.integration.snapshot = Snapshot {
        availability: Availability::Live,
        data: if id == "t3" {
            Some(Data::T3(t3::Snapshot {
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
                permissions: vec![
                    "orchestration:read".into(),
                    "orchestration:operate".into(),
                    "diagnostics:read".into(),
                ],
                usage: Some(vec![
                    t3::Usage {
                        provider: "Codex".into(),
                        model: "GPT-6.1 Sol".into(),
                        tokens: 1_240_000,
                        cost_microusd: 4_560_000,
                        unpriced: false,
                    },
                    t3::Usage {
                        provider: "Claude".into(),
                        model: "Opus".into(),
                        tokens: 820_000,
                        cost_microusd: 7_200_000,
                        unpriced: false,
                    },
                ]),
                usage_day: Some("2026-11-05".into()),
                history: Some(history()),
                limits: std::iter::once(t3::Limit {
                    driver: "codex".into(),
                    provider: "Codex".into(),
                    instance: "codex".into(),
                    account: "alex@example.test".into(),
                    window: "Weekly".into(),
                    used_permille: 660,
                    resets_at: Some("2026-11-10T12:00:00.000Z".into()),
                    duration_mins: Some(7 * 24 * 60),
                })
                .chain(
                    [("one@example.test", 430), ("two@example.test", 290)]
                        .into_iter()
                        .flat_map(|(account, used)| {
                            [("Weekly", used), ("Weekly · Fable", 0)].into_iter().map(
                                move |(window, used_permille)| t3::Limit {
                                    driver: "claudeAgent".into(),
                                    provider: "Claude".into(),
                                    instance: account.into(),
                                    account: account.into(),
                                    window: window.into(),
                                    used_permille,
                                    resets_at: Some("2026-11-09T12:00:00.000Z".into()),
                                    duration_mins: Some(7 * 24 * 60),
                                },
                            )
                        }),
                )
                .collect(),
                ..Default::default()
            }))
        } else {
            None
        },
        ..Default::default()
    };
    if id == "grok" {
        model.integration.grok.discovery = grok::Discovery {
            installed: true,
            running: true,
            signed_in: true,
        };
        model.integration.routines = ["Morning brief", "Review my calendar", "Research watchlist"]
            .iter()
            .enumerate()
            .map(|(i, title)| config::Routine {
                id: format!("routine-{i}"),
                title: (*title).into(),
                url: "https://example.test/routine".into(),
                input: json!({}),
            })
            .collect();
        model.integration.runs = vec![
            launcher::RoutineRun {
                id: "routine-0".into(),
                title: "Morning brief".into(),
                state: launcher::RoutineState::Accepted,
                at_ms: NOW,
            },
            launcher::RoutineRun {
                id: "routine-1".into(),
                title: "Review my calendar".into(),
                state: launcher::RoutineState::Sending,
                at_ms: NOW,
            },
            launcher::RoutineRun {
                id: "routine-2".into(),
                title: "Research watchlist".into(),
                state: launcher::RoutineState::Failed,
                at_ms: NOW,
            },
        ];
    }
    model
}

/// 30 days of two providers' cost and tokens, shaped like a real usage summary.
fn history() -> t3::History {
    let days: Vec<String> = (7..=31)
        .map(|d| format!("2026-10-{d:02}"))
        .chain((1..=5).map(|d| format!("2026-11-{d:02}")))
        .collect();
    let wave = |phase: f64, scale: f64| -> Vec<u64> {
        (0..30)
            .map(|i| ((f64::from(i) * 0.7 + phase).sin().abs() * scale) as u64)
            .collect()
    };
    t3::History {
        days,
        providers: vec![
            t3::ProviderHistory {
                provider: "claudeAgent".into(),
                cost: wave(0.0, 600_000_000.0),
                tokens: wave(0.0, 2_000_000_000.0),
                sessions: 386,
                sessions_distinct: true,
            },
            t3::ProviderHistory {
                provider: "codex".into(),
                cost: wave(1.3, 250_000_000.0),
                tokens: wave(1.3, 600_000_000.0),
                sessions: 636,
                sessions_distinct: true,
            },
        ],
        uncached: 261_000_000,
        cached: 72_300_000_000,
        cache_write: 685_000_000,
        output: 201_000_000,
        savings_microusd: 271_003_400_000,
        cost_by_type: [0; 4],
    }
}

fn draw(ui: &mut Ui, model: &Model, size: (u32, u32), scale: f32) -> DisplayList {
    views::view(
        model,
        &Tokens::default(),
        views::Frame {
            size,
            scale,
            now: Stamp::at(NOW),
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    )
}

fn label(sheet: &mut DisplayList, x: f32, y: f32, text: &str) {
    sheet.ops.push(Op::Text {
        rect: Rect::new(x, y, 1000.0, 24.0),
        text: text.into(),
        style: TextStyle::new("Segoe UI Variable", 14.0),
        color: Rgba::rgb8([220, 228, 240], 1.0),
    });
}
fn add(ui: &mut Ui, sheet: &mut DisplayList, model: &Model, size: (u32, u32), x: f32, y: f32) {
    let list = draw(ui, model, size, 1.0);
    let image = render(ui, &list, size.0, size.1).unwrap();
    let rect = Rect::new(x, y, size.0 as f32, size.1 as f32);
    sheet.ops.push(Op::Fill {
        rect,
        radius: 22.0,
        color: Rgba::rgb8([44, 52, 64], 1.0),
    });
    sheet.ops.push(Op::Image {
        rect,
        image: Arc::new(image),
        opacity: 1.0,
        source: None,
    });
}
fn sheet(ui: &mut Ui, id: &str, variants: &[&str]) -> Image {
    let height = variants.len() as u32 * 380 + 285;
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, 1176.0, height as f32),
        radius: 0.0,
        color: Rgba::rgb8([20, 25, 33], 1.0),
    });
    for (row, variant) in variants.iter().enumerate() {
        let y = 20.0 + row as f32 * 380.0;
        label(
            &mut sheet,
            24.0,
            y,
            &format!("{id} / {variant} — compact · regular · large"),
        );
        let model = fixture(id, variant);
        for (x, size) in [(24.0, (180, 132)), (228.0, (360, 220)), (612.0, (540, 330))] {
            add(ui, &mut sheet, &model, size, x, y + 30.0);
        }
    }
    let y = variants.len() as f32 * 380.0 + 20.0;
    for (index, (x, title)) in [
        (24.0, "SETUP"),
        (412.0, "DISCONNECTED"),
        (800.0, "PERMISSION / NO ROUTINES"),
    ]
    .into_iter()
    .enumerate()
    {
        label(&mut sheet, x, y, title);
        let mut m = fixture(id, if id == "t3" { "usage" } else { "routines" });
        if index == 0 {
            m.integration.snapshot = Snapshot {
                availability: Availability::Unavailable("opaque protocol error".into()),
                ..Default::default()
            };
            m.integration.routines.clear();
            m.integration.grok.discovery = Default::default();
        } else if index == 1 {
            m.integration.snapshot.availability = Availability::Stale("raw socket error".into());
        } else {
            if let Some(Data::T3(state)) = &mut m.integration.snapshot.data {
                state.usage = None;
                state.history = None;
                state.permissions.clear();
            }
            m.integration.routines.clear();
        }
        add(ui, &mut sheet, &m, (352, 220), x, y + 30.0);
    }
    render(ui, &sheet, 1176, height).unwrap()
}

#[test]
fn t3_grok_dense_sizes_and_sheets() {
    let mut ui = Ui::new().unwrap();
    for (id, variants) in [
        (
            "t3",
            &["running", "usage", "limits", "attention", "new"][..],
        ),
        ("grok", &["routines", "recent", "prompt"][..]),
    ] {
        for variant in variants {
            for width in 4..=12 {
                for height in 3..=10 {
                    for scale in [1.0, 1.5] {
                        let size = (
                            (width as f32 * 44.0 * scale) as u32,
                            (height as f32 * 44.0 * scale) as u32,
                        );
                        let m = fixture(id, variant);
                        let list = draw(&mut ui, &m, size, scale);
                        assert!(
                            list.damage(&draw(&mut ui, &m, size, scale)).is_none(),
                            "{id}/{variant} idle damage"
                        );
                        for hit in &list.hits {
                            assert!(
                                hit.rect.left >= 0.0
                                    && hit.rect.top >= 0.0
                                    && hit.rect.right <= size.0 as f32
                                    && hit.rect.bottom <= size.1 as f32,
                                "{id}/{variant}/{width}x{height}: {} {:?}",
                                hit.label,
                                hit.rect
                            );
                        }
                    }
                }
            }
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("src/widgets/views/integrations/{id}-sizes.png"));
        golden::check(&sheet(&mut ui, id, variants), &path).unwrap();
    }
}

#[test]
fn t3_limits_renders_claude_accounts_side_by_side() {
    let mut ui = Ui::new().unwrap();
    let m = fixture("t3", "limits");
    let list = draw(&mut ui, &m, (960, 480), 1.0);
    let labels: Vec<_> = list
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Text { text, .. } => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    for label in ["Claude", "57%", "71%", "Weekly · Fable"] {
        assert!(labels.contains(&label), "missing {label}");
    }
    let mut screenshot = DisplayList::default();
    screenshot.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, 960.0, 480.0),
        radius: 0.0,
        color: Rgba::rgb8([20, 25, 33], 1.0),
    });
    screenshot.ops.extend(list.ops);
    let image = render(&mut ui, &screenshot, 960, 480).unwrap();
    golden::check(
        &image,
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/integrations/golden/t3-limits-claude.png"),
    )
    .unwrap();
}

#[test]
fn t3_pairing_draft_is_masked_in_the_display_list() {
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("t3", "limits");
    m.integration.snapshot = Snapshot {
        availability: Availability::Unavailable("Connect using a pairing code".into()),
        ..Default::default()
    };
    m.integration.t3.draft = "fixture-private-pairing-code".into();
    let list = draw(&mut ui, &m, (540, 330), 1.0);
    assert!(
        list.ops
            .iter()
            .any(|op| matches!(op, Op::Text {text, ..} if text.contains('•')))
    );
    assert!(
        !list
            .ops
            .iter()
            .any(|op| matches!(op, Op::Text {text, ..} if text.contains(&m.integration.t3.draft)))
    );
    // Exercise an active native input without focusing it or sending desktop input.
    let surface = mypaper_ui::Surface::new(
        mypaper_ui::Kind::Popup { activate: false },
        "T3 pairing test",
        mypaper_ui::Role::TextInput,
    )
    .unwrap();
    let field = mypaper_ui::TextField::new(
        surface.hwnd(),
        Rect::new(0.0, 0.0, 300.0, 30.0),
        false,
        13.0,
        Rgba::rgb8([255, 255, 255], 1.0),
    )
    .unwrap();
    field.set_text("active-private-pairing-code").unwrap();
    m.field = Some(field);
    let list = draw(&mut ui, &m, (540, 330), 1.0);
    assert!(!list.ops.iter().any(|op| matches!(op, Op::TextField { .. })));
    assert!(
        list.ops
            .iter()
            .any(|op| matches!(op, Op::Text {text, ..} if text.contains('•')))
    );
    assert!(
        !list
            .ops
            .iter()
            .any(|op| matches!(op, Op::Text {text, ..} if text.contains("active-private")))
    );
}

#[test]
fn t3_scrolling_reaches_the_last_agent_and_stale_approvals_are_disabled() {
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("t3", "running");
    draw(&mut ui, &m, (360, 360), 1.0);
    m.scroll = m.scroll_max.get();
    let list = draw(&mut ui, &m, (360, 360), 1.0);
    assert!(
        list.hits
            .iter()
            .any(|hit| hit.id == crate::integrations::t3_widgets::THREAD + 19)
    );
    m.placed
        .settings
        .insert("variant".into(), json!("attention"));
    m.integration.snapshot.availability =
        Availability::Stale("socket detail must stay hidden".into());
    let list = draw(&mut ui, &m, (360, 360), 1.0);
    assert!(
        list.hits
            .iter()
            .filter(|hit| (100_000..140_000).contains(&hit.id))
            .all(|hit| !hit.enabled)
    );
    assert!(
        !list
            .ops
            .iter()
            .any(|op| matches!(op,Op::Text{text,..} if text.contains("socket detail")))
    );
}

#[test]
fn rendered_approvals_never_retarget_after_a_thread_or_request_changes() {
    use crate::integrations::t3_widgets::{APPROVE, resolve_approval};
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("t3", "attention");
    draw(&mut ui, &m, (360, 360), 1.0);
    let Some(Data::T3(snapshot)) = &mut m.integration.snapshot.data else {
        unreachable!()
    };
    assert_eq!(
        resolve_approval(&m.integration.t3, snapshot, APPROVE),
        Some(("thread-0".into(), "approval".into()))
    );
    snapshot.threads[0]
        .pending_runtime_request
        .as_mut()
        .unwrap()
        .id = "replacement-request".into();
    assert!(resolve_approval(&m.integration.t3, snapshot, APPROVE).is_none());
    snapshot.threads.remove(0);
    snapshot.threads[0].pending_runtime_request = Some(t3::Request {
        id: "different-thread-request".into(),
        kind: "command".into(),
    });
    assert!(resolve_approval(&m.integration.t3, snapshot, APPROVE).is_none());
}
