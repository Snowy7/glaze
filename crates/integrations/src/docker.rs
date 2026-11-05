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
        containers: entries
            .iter()
            .filter_map(|c| {
                let state = c["State"].as_str()?.to_owned();
                Some(Container {
                    id: c["Id"].as_str()?.into(),
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
            .collect(),
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
    let mut events = transport::pipe_request(
        &config.pipe,
        "GET",
        &format!("/events?filters={}", filter(config, true)),
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
