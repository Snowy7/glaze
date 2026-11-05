//! One filtered Engine event stream; bounded list reconciliation only on actual lifecycle events.
use crate::{Availability, Data, Publish, transport};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub state: String,
    pub failed: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub containers: Vec<Container>,
    pub usage: std::collections::BTreeMap<String, Usage>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Hundredths of a percent, including all assigned CPUs.
    pub cpu_hundredths: Option<u64>,
    pub memory_bytes: u64,
    pub memory_limit: u64,
}
pub fn usage(value: &Value) -> Usage {
    let cpu = &value["cpu_stats"];
    let old = &value["precpu_stats"];
    let total = cpu["cpu_usage"]["total_usage"].as_u64().unwrap_or(0);
    let previous = old["cpu_usage"]["total_usage"].as_u64().unwrap_or(0);
    let system = cpu["system_cpu_usage"].as_u64().unwrap_or(0);
    let before = old["system_cpu_usage"].as_u64().unwrap_or(0);
    let cores = cpu["online_cpus"]
        .as_u64()
        .or_else(|| {
            cpu["cpu_usage"]["percpu_usage"]
                .as_array()
                .map(|a| a.len() as u64)
        })
        .unwrap_or(1);
    let memory = &value["memory_stats"];
    Usage {
        cpu_hundredths: (before > 0 && system > before && total >= previous).then(|| {
            ((total - previous) as u128 * cores as u128 * 10_000 / (system - before) as u128)
                .min(u64::MAX as u128) as u64
        }),
        memory_bytes: memory["usage"].as_u64().unwrap_or(0).saturating_sub(
            memory["stats"]["inactive_file"]
                .as_u64()
                .or_else(|| memory["stats"]["total_inactive_file"].as_u64())
                .unwrap_or(0),
        ),
        memory_limit: memory["limit"].as_u64().unwrap_or(0),
    }
}

fn filter(config: &crate::config::Docker, events: bool) -> String {
    let mut filters = json!({});
    if events {
        filters["type"] = json!(["container"]);
        filters["event"] = json!([
            "create",
            "destroy",
            "start",
            "stop",
            "die",
            "oom",
            "pause",
            "unpause",
            "health_status"
        ]);
        if !config.containers.is_empty() {
            filters["container"] = json!(config.containers);
        }
    } else if !config.containers.is_empty() {
        filters["id"] = json!(config.containers);
    }
    crate::launcher::encode(&filters.to_string())
}
async fn read(config: &crate::config::Docker) -> Result<Snapshot> {
    let list = transport::pipe_json(
        &config.pipe,
        "GET",
        &format!(
            "/containers/json?all=1&limit=128&filters={}",
            filter(config, false)
        ),
        None,
    )
    .await?;
    snapshot(&list)
}
pub fn snapshot(list: &Value) -> Result<Snapshot> {
    let entries = list
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid Docker container list"))?;
    ensure!(
        entries.len() <= 128,
        "Docker list exceeds 128 containers; select containers in settings"
    );
    Ok(Snapshot {
        usage: Default::default(),
        containers: entries
            .iter()
            .map(|c| -> Result<Container> {
                let state = c["State"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Docker container state is missing"))?
                    .to_owned();
                Ok(Container {
                    id: c["Id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Docker container ID is missing"))?
                        .into(),
                    name: c["Names"][0]
                        .as_str()
                        .unwrap_or("Container")
                        .trim_start_matches('/')
                        .into(),
                    failed: c["Status"].as_str().is_some_and(|s| {
                        s.contains("unhealthy")
                            || (s.starts_with("Exited (") && !s.starts_with("Exited (0)"))
                    }),
                    state,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}
pub(crate) async fn run(
    config: &crate::config::Docker,
    publish: &Publish,
    commands: &mut mpsc::Receiver<crate::Action>,
) -> Result<()> {
    ensure!(
        !config.pipe.is_empty(),
        "select Docker's existing local context pipe in settings"
    );
    // Subscribe before the seed. An event is an invalidation, never a counter delta: no lost-update race.
    // The server flushes headers before registering its subscriber. A bounded replay cut covers that gap.
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut events = transport::pipe_request(
        &config.pipe,
        "GET",
        &format!("/events?since={since}&filters={}", filter(config, true)),
        None,
    )
    .await?;
    let mut state = read(config).await?;
    publish.set(Availability::Live, Some(Data::Docker(state.clone())));
    loop {
        tokio::select! {
            Some(action) = commands.recv() => if let crate::Action::Docker { id, start } = action {
                if !state.containers.iter().any(|c| c.id == id) { publish.action_result(Err(anyhow::anyhow!("container is no longer in the snapshot"))); continue; }
                let result = async {
                    let path = format!("/containers/{}/{}", crate::launcher::encode(&id), if start { "start" } else { "stop" });
                    transport::pipe_request(&config.pipe, "POST", &path, None).await?.finish().await
                }.await;
                publish.action_result(result);
            } else if matches!(action, crate::Action::DockerMetrics) {
                // Bound the request fanout. Idle containers and hidden widgets create no stats work.
                let mut failed = None;
                for container in state.containers.iter().filter(|c| c.state == "running").take(16) {
                    let path = format!("/containers/{}/stats?stream=false&one-shot=true", crate::launcher::encode(&container.id));
                    match transport::pipe_json(&config.pipe, "GET", &path, None).await {
                        Ok(value) => { state.usage.insert(container.id.clone(), usage(&value)); }
                        Err(error) => { failed = Some(error); break; }
                    }
                }
                publish.action_result(failed.map_or(Ok(()), Err));
                publish.set(Availability::Live, Some(Data::Docker(state.clone())));
            },
            event = events.next() => {
                ensure!(event?.is_some(), "Docker event stream closed");
                publish.delivered();
                state = read(config).await?;
                publish.set(Availability::Live, Some(Data::Docker(state.clone())));
            }
        }
    }
}
