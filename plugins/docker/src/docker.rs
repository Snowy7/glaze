//! Docker Engine decoding; CPU deltas and working-set memory match the native provider.
use anyhow::{Result, ensure};
use serde_json::Value;

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
