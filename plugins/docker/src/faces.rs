//! One Docker face, responsive to the host's three size classes.
use crate::{Availability, Data, Snapshot, docker};
use mypaper_plugin_sdk::SizeClass;
use mypaper_plugin_sdk::{Face, Role, Tone, View};

fn metric(value: String, label: &str, color: Option<Tone>, width: Option<f32>) -> View {
    View::Metric {
        value,
        label: label.into(),
        color,
        width,
    }
}

fn rows(state: &docker::Snapshot, enabled: bool) -> View {
    View::List {
        gap: Some(0.0),
        children: state
            .containers
            .iter()
            .map(|container| {
                let running = container.state == "running";
                let detail = if container.failed {
                    "Needs attention".into()
                } else if let Some(usage) = state.usage.get(&container.id) {
                    let cpu = usage.cpu_hundredths.map_or_else(
                        || "CPU —".into(),
                        |cpu| format!("{:.0}%", cpu as f64 / 100.0),
                    );
                    format!("{cpu} · {:.0} MiB", usage.memory_bytes as f64 / 1_048_576.0)
                } else {
                    container.state.clone()
                };
                View::Row {
                    gap: Some(3.0),
                    children: vec![
                        View::ListRow {
                            title: container.name.clone(),
                            detail: Some(detail),
                            icon: Some(
                                if container.failed {
                                    "warning"
                                } else {
                                    "layers"
                                }
                                .into(),
                            ),
                            trailing: None,
                            action: None,
                        },
                        View::IconButton {
                            icon: if running { "pause" } else { "play" }.into(),
                            label: if running {
                                "Stop container"
                            } else {
                                "Start container"
                            }
                            .into(),
                            action: format!(
                                "{}:{}",
                                if running { "stop" } else { "start" },
                                container.id
                            ),
                            enabled,
                        },
                    ],
                }
            })
            .collect(),
    }
}

pub fn face(size: SizeClass, snapshot: &Snapshot) -> Face {
    let footer = snapshot
        .action_error
        .clone()
        .unwrap_or_else(|| match &snapshot.availability {
            Availability::Live => "Refresh for metrics".into(),
            Availability::Connecting => "Connecting…".into(),
            Availability::Stale(_) => "Docker isn't running · last snapshot".into(),
            Availability::Unavailable(error) => error.clone(),
        });
    let Some(Data::Docker(state)) = &snapshot.data else {
        return Face {
            body: if snapshot.availability == Availability::Connecting {
                View::empty(
                    "layers",
                    "Connecting to Docker",
                    "Reading the local engine…",
                )
            } else {
                View::empty(
                    "layers",
                    "Docker isn't running",
                    "Start Docker, then refresh.",
                )
            },
            footer: Some(footer),
            refresh: true,
        };
    };
    let running = state
        .containers
        .iter()
        .filter(|c| c.state == "running")
        .count();
    let containers = if state.containers.is_empty() {
        View::empty(
            "layers",
            "No containers yet",
            "Containers from your local engine appear here.",
        )
    } else {
        rows(state, snapshot.availability == Availability::Live)
    };
    let body = match size {
        SizeClass::Small => View::Stack {
            gap: Some(5.0),
            children: vec![
                View::row([
                    View::text(running.to_string(), Role::Title1),
                    View::tinted("running", Role::Caption, Tone::Secondary),
                ]),
                containers,
            ],
        },
        SizeClass::Medium => View::Row {
            gap: Some(8.0),
            children: vec![
                metric(running.to_string(), "running", None, Some(105.0)),
                containers,
            ],
        },
        SizeClass::Large => {
            let samples: Vec<_> = state
                .containers
                .iter()
                .filter(|c| c.state == "running")
                .filter_map(|c| state.usage.get(&c.id))
                .collect();
            let cpu = if samples.is_empty() || samples.iter().any(|s| s.cpu_hundredths.is_none()) {
                "—".into()
            } else {
                format!(
                    "{:.0}%",
                    samples.iter().filter_map(|s| s.cpu_hundredths).sum::<u64>() as f64 / 100.0
                )
            };
            let ram = if samples.is_empty() {
                "—".into()
            } else {
                format!(
                    "{:.0}",
                    samples.iter().map(|s| s.memory_bytes).sum::<u64>() as f64 / 1_048_576.0
                )
            };
            View::Stack {
                gap: Some(9.0),
                children: vec![
                    View::row([
                        metric(running.to_string(), "running", None, None),
                        metric(cpu, "CPU · sample", Some(Tone::Secondary), None),
                        metric(ram, "MiB · sample", Some(Tone::Secondary), None),
                    ]),
                    containers,
                ],
            }
        }
    };
    Face {
        body,
        footer: Some(footer),
        refresh: true,
    }
}
