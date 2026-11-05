//! Stable metric identities and quantized display values shared by invalidation and drawing.
use super::model::Model;

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub id: String,
    pub label: String,
    pub value: String,
    pub detail: String,
    pub percent: f64,
    pub supported: bool,
}

pub fn visible(m: &Model) -> Vec<Metric> {
    let Some(stats) = &m.stats else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut percent = |id: &str, label: &str, value: Option<f64>, detail: String| {
        let pct = value.unwrap_or(0.0).clamp(0.0, 100.0).round();
        out.push(Metric {
            id: id.into(),
            label: label.into(),
            value: value.map_or_else(|| "Unavailable".into(), |_| format!("{pct:.0}%")),
            detail,
            percent: pct,
            supported: value.is_some(),
        });
    };
    if m.flag("cpu", true) {
        percent("cpu", "CPU", Some(stats.cpu), String::new());
    }
    if m.flag("ram", true) {
        percent(
            "ram",
            "Memory",
            Some(stats.memory.used as f64 / stats.memory.total.max(1) as f64 * 100.0),
            format!(
                "{:.1} of {:.0} GB",
                stats.memory.used as f64 / 1_073_741_824.0,
                stats.memory.total as f64 / 1_073_741_824.0
            ),
        );
    }
    if m.flag("gpu", true) {
        percent("gpu", "GPU", stats.gpu, String::new());
    }
    if m.flag("disk", false) {
        let filter = m.str("drives", "").to_uppercase();
        for disk in stats.disks.iter().flatten().filter(|d| {
            filter.is_empty() || d.name.chars().next().is_some_and(|c| filter.contains(c))
        }) {
            percent(
                &format!("disk-{}", disk.name),
                &disk.name,
                Some(disk.used as f64 / disk.total.max(1) as f64 * 100.0),
                format!(
                    "{:.0} / {:.0} GB",
                    disk.used as f64 / 1_073_741_824.0,
                    disk.total as f64 / 1_073_741_824.0
                ),
            );
        }
    }
    if m.flag("battery", true)
        && let Some(battery) = stats.battery
    {
        percent(
            "battery",
            "Battery",
            Some(battery.percent.into()),
            if battery.charging {
                "Charging"
            } else if battery.percent <= 20 {
                "Low"
            } else {
                "On battery"
            }
            .into(),
        );
    }
    if m.flag("net", true) {
        let label = stats
            .network
            .as_ref()
            .and_then(|n| n.name.clone())
            .unwrap_or_else(|| "Network".into());
        let (down, up) = stats.net.map_or((0.0, 0.0), |n| (n.down, n.up));
        out.push(Metric {
            id: "net".into(),
            label,
            value: stats.net.map_or_else(
                || "Unavailable".into(),
                |_| format!("↓{} ↑{}", rate(down), rate(up)),
            ),
            detail: if stats.net.is_some() {
                "per second"
            } else {
                ""
            }
            .into(),
            percent: (down / 262_144.0 * 100.0).clamp(0.0, 100.0).round(),
            supported: stats.net.is_some(),
        });
    }
    out.sort_by_key(|metric| match metric.id.as_str() {
        "cpu" => 0,
        "ram" => 1,
        "gpu" => 2,
        "net" => 3,
        "battery" => 5,
        _ => 4,
    });
    out
}

pub fn rate(bytes: f64) -> String {
    if bytes >= 1_000_000.0 {
        format!("{:.1}M", bytes / 1_000_000.0)
    } else {
        format!("{:.0}K", bytes / 1000.0)
    }
}
