//! Size sheets render the production dispatch, including disconnected and setup states.
use super::*;
use crate::widgets::{time::Stamp, views};
use mypaper_integrations::{
    Data, Snapshot, config, launcher, notifications, obs, t3,
};
use mypaper_ui::{DisplayList, Image, Op, TextStyle, Tokens, Ui, golden, render};
use mypaper_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

const NOW: i64 = 1_793_875_200_000;
const IDS: [&str; 5] = ["discord", "t3", "obs", "grok"];

fn model(id: &str, size: [u32; 2]) -> Model {
    let placed = Placed {
        instance: format!("{id}-fixture"),
        widget: id.into(),
        cell: [0; 2],
        size,
        settings: Map::new(),
        appearance: Map::new(),
        own: Map::new(),
    };
    let root = std::env::temp_dir().join("mypaper-integration-widget-fixture");
    let mut model = Model::new(placed, &mut Storage::new(root), Stamp::at(NOW)).unwrap();
    model.visible = true;
    model.integration.observed_at_ms = NOW;
    model.integration.snapshot = Snapshot {
        availability: Availability::Live,
        data: match id {
            "t3" => Some(Data::T3(t3::Snapshot {
                threads: vec![
                    thread(
                        "approval",
                        "Review desktop permissions",
                        t3::Status::Waiting,
                        Some("approval"),
                    ),
                    thread(
                        "working",
                        "Build the native widget kit",
                        t3::Status::Running,
                        None,
                    ),
                    thread(
                        "done",
                        "Simplify the shared providers",
                        t3::Status::Completed,
                        None,
                    ),
                    thread("idle", "Explore a new wallpaper", t3::Status::Idle, None),
                ],
                projects: BTreeMap::from([("mypaper".into(), "mypaper".into())]),
                ..Default::default()
            })),
            "obs" => Some(Data::Obs(obs::Snapshot {
                recording: true,
                streaming: true,
                scene: "Desktop + camera".into(),
                scenes: vec![
                    "Desktop + camera".into(),
                    "Camera".into(),
                    "Be right back".into(),
                ],
                microphones: vec![obs::Microphone {
                    name: "Desk microphone".into(),
                    muted: false,
                }],
                record_clock: obs::Clock {
                    elapsed_ms: 3_965_000,
                    at_ms: NOW,
                    advancing: true,
                },
                stream_clock: obs::Clock {
                    elapsed_ms: 825_000,
                    at_ms: NOW,
                    advancing: true,
                },
                ..Default::default()
            })),
            _ => None,
        },
        ..Default::default()
    };
    for id in ["approval", "working", "done", "idle"] {
        model
            .integration
            .thread_links
            .insert(id.into(), format!("https://example.test/threads/{id}"));
    }
    if id == "grok" {
        model.integration.routines = ["Morning brief", "Review my calendar", "Research watchlist"]
            .iter()
            .enumerate()
            .map(|(index, title)| config::Routine {
                id: format!("routine-{index}"),
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
                at_ms: NOW - 3_600_000,
            },
            launcher::RoutineRun {
                id: "routine-2".into(),
                title: "Research watchlist".into(),
                state: launcher::RoutineState::Failed,
                at_ms: NOW - 7_200_000,
            },
        ];
    }
    model
}

fn thread(id: &str, title: &str, status: t3::Status, pending: Option<&str>) -> t3::Thread {
    t3::Thread {
        id: id.into(),
        project_id: "mypaper".into(),
        title: title.into(),
        provider_instance_id: "Codex".into(),
        status,
        latest_run_id: Some(id.into()),
        active_run_id: None,
        pending_runtime_request: pending.map(|kind| t3::Request {
            id: "q".into(),
            kind: kind.into(),
        }),
        updated_at: "2026-11-05T10:00:00Z".into(),
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

#[test]
fn size_sheets_have_pure_frames_bounded_hits_and_no_idle_deadlines() {
    let mut ui = Ui::new().unwrap();
    for id in IDS {
        for (grid, pixels) in [
            ([2, 2], (180, 180)),
            ([4, 2], (360, 180)),
            ([4, 4], (360, 360)),
        ] {
            for scale in [1.0, 1.5] {
                let mut m = model(id, grid);
                let pixels = (
                    (pixels.0 as f32 * scale) as u32,
                    (pixels.1 as f32 * scale) as u32,
                );
                let list = draw(&mut ui, &m, pixels, scale);
                let same = draw(&mut ui, &m, pixels, scale);
                assert!(
                    list.damage(&same).is_none(),
                    "{id} redraws with identical input"
                );
                for hit in &list.hits {
                    assert!(
                        hit.rect.left >= 0.0
                            && hit.rect.top >= 0.0
                            && hit.rect.right <= pixels.0 as f32
                            && hit.rect.bottom <= pixels.1 as f32,
                        "{id}: {} {:?}",
                        hit.label,
                        hit.rect
                    );
                }
                assert!(
                    !list
                        .ops
                        .iter()
                        .any(|op| matches!(op, Op::Stroke { color, .. } if color.a > 0.0)),
                    "{id} outlines glass"
                );
                m.schedule(Stamp::at(NOW), Instant::now());
                assert_eq!(m.next.is_some(), id == "obs", "{id} has an idle deadline");
                m.visible = false;
                m.schedule(Stamp::at(NOW), Instant::now());
                assert!(m.next.is_none());
            }
        }
        let sheet = sheet(&mut ui, id);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../docs/media/native/widget-{id}.png"));
        golden::check(&sheet, &path).unwrap();
    }
}

fn sheet(ui: &mut Ui, id: &str) -> Image {
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, 996.0, 740.0),
        radius: 0.0,
        color: Rgba::rgb8([20, 25, 33], 1.0),
    });
    sheet.ops.push(Op::Text {
        rect: Rect::new(24.0, 16.0, 940.0, 28.0),
        text: format!("{}  /  small · medium · large", id.to_uppercase()).into(),
        style: TextStyle::new("Segoe UI Variable", 18.0),
        color: Rgba::rgb8([235, 240, 248], 1.0),
    });
    for (grid, pixels, x) in [
        ([2, 2], (180, 180), 24.0),
        ([4, 2], (360, 180), 228.0),
        ([4, 4], (360, 360), 612.0),
    ] {
        add_widget(ui, &mut sheet, &model(id, grid), pixels, x, 58.0);
    }
    for (x, label) in [(24.0, "SETUP"), (348.0, "DISCONNECTED"), (672.0, "EMPTY")] {
        sheet.ops.push(Op::Text {
            rect: Rect::new(x, 449.0, 300.0, 23.0),
            text: label.into(),
            style: TextStyle::new("Segoe UI Variable", 12.0),
            color: Rgba::rgb8([159, 172, 190], 1.0),
        });
    }
    for (index, x) in [24.0, 348.0, 672.0].into_iter().enumerate() {
        let mut m = model(id, [4, 2]);
        match index {
            0 => {
                m.integration.snapshot = Snapshot {
                    availability: Availability::Unavailable("Finish setup in Settings".into()),
                    ..Default::default()
                };
                m.integration.routines.clear();
            }
            1 => {
                m.integration.snapshot.availability =
                    Availability::Stale("Source disconnected".into());
            }
            _ => {
                m.integration.snapshot.data = match id {
                    "t3" => Some(Data::T3(Default::default())),
                    "discord" => Some(Data::Notifications(Default::default())),
                    "obs" => Some(Data::Obs(Default::default())),
                    _ => None,
                };
                m.integration.runs.clear();
            }
        }
        add_widget(ui, &mut sheet, &m, (300, 210), x, 486.0);
    }
    render(ui, &sheet, 996, 740).unwrap()
}

fn add_widget(
    ui: &mut Ui,
    sheet: &mut DisplayList,
    model: &Model,
    size: (u32, u32),
    x: f32,
    y: f32,
) {
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

#[test]
fn obs_clock_stops_when_paused_or_disconnected_and_discord_filters_exact_app_identity() {
    let mut obs = model("obs", [4, 2]);
    let Some(Data::Obs(state)) = obs.integration.snapshot.data.as_mut() else {
        unreachable!()
    };
    state.paused = true;
    state.streaming = false;
    obs.schedule(Stamp::at(NOW), Instant::now());
    assert!(obs.next.is_none());
    let mut ui = Ui::new().unwrap();
    let list = draw(&mut ui, &model("discord", [4, 4]), (360, 360), 1.0);
    assert!(
        !list
            .ops
            .iter()
            .any(|op| matches!(op, Op::Text { text, .. } if text.contains("Never shown")))
    );
}

/// The shared catalog test uses a plain initial model. Export just our six baselines without
/// blessing another agent's widget images while they are editing the same checkout.
#[test]
#[ignore = "exports only the six integration catalog goldens"]
fn export_integration_catalog_goldens() {
    use mypaper_widgets::{
        layout::{self, Grid, Layout, RectF},
        look, registry,
    };
    let mut ui = Ui::new().unwrap();
    let registry = registry::builtins();
    for id in IDS {
        let manifest = &registry.get(id).unwrap().manifest;
        let mut layout = Layout::default();
        let instance = layout.add(id, "fixture", [0, 0], manifest.size.default);
        let grid = Grid::new(
            &layout.grid,
            [96.0; 2],
            RectF {
                x: 0.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            },
        );
        let placed = layout::place(
            layout.get(&instance).unwrap(),
            manifest,
            &grid,
            &look::defaults(),
        );
        let m = Model::new(
            placed,
            &mut Storage::new(std::env::temp_dir().join("mypaper-widget-catalog")),
            Stamp::at(NOW),
        )
        .unwrap();
        let mut list = views::view(
            &m,
            &crate::widgets::tokens(&m.placed.appearance),
            views::Frame {
                size: (360, 240),
                scale: 1.0,
                now: Stamp::at(NOW),
                instant: Instant::now(),
                editing: false,
            },
            &mut ui.text,
        );
        list.ops.insert(
            0,
            Op::Fill {
                rect: Rect::new(0.0, 0.0, 360.0, 240.0),
                radius: 0.0,
                color: Rgba::rgb8([36, 43, 53], 1.0),
            },
        );
        let image = render(&mut ui, &list, 360, 240).unwrap();
        golden::write(
            &image,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("src/widgets/golden/{id}.png")),
        )
        .unwrap();
    }
}
