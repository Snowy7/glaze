//! The results as Markdown tables (`perf/latest.md`) and JSON (`perf/latest.json`).

use std::collections::BTreeMap;
use std::fmt::Write;

use serde::Serialize;

use crate::budget::Violation;
use crate::run::Outcome;

#[derive(Debug, Serialize)]
pub struct Report {
    /// Local time of the run, `YYYY-MM-DD HH:MM`.
    pub when: String,
    pub label: Option<String>,
    pub machine: String,
    pub build: String,
    pub plan: String,
    pub scenarios: Vec<Outcome>,
    /// Scenarios that failed to run at all.
    pub failures: Vec<String>,
    pub violations: Vec<Violation>,
    /// How many limits applied.
    pub checked: usize,
}

impl Report {
    pub fn markdown(&self) -> String {
        let mut md = String::new();
        let title = match &self.label {
            Some(label) => format!("{} ({label})", self.when),
            None => self.when.clone(),
        };
        let _ = writeln!(md, "# mypaper performance: {title}\n");
        let _ = writeln!(md, "- Machine: {}", self.machine);
        let _ = writeln!(md, "- Build: `{}`", self.build);
        let _ = writeln!(md, "- {}\n", self.plan);

        md.push_str("## Budget\n\n");
        if self.violations.is_empty() && self.failures.is_empty() {
            let _ = writeln!(
                md,
                "All {} limits in `perf/budget.toml` are met.\n",
                self.checked
            );
        } else {
            let _ = writeln!(
                md,
                "**{} of {} limits broken.**\n",
                self.violations.len(),
                self.checked
            );
            for violation in &self.violations {
                let _ = writeln!(md, "- {violation}");
            }
            for failure in &self.failures {
                let _ = writeln!(md, "- failed to run: {failure}");
            }
            md.push('\n');
        }

        md.push_str("## Startup\n\n");
        md.push_str(
            "Milliseconds from core's process creation to each component being ready, \
             cold (fresh profile) / warm (median).\n\n",
        );
        md.push_str(
            "| Scenario | core spawned | wallpaper first frame | widgets first paint \
             | bars native paint | bars first paint | taskbar hidden |\n",
        );
        md.push_str("|---|---|---|---|---|---|---|\n");
        for s in &self.scenarios {
            let t = |key: &str| cold_warm(&s.metrics, key);
            let _ = writeln!(
                md,
                "| `{}` | {} | {} | {} | {} | {} | {} |",
                s.scenario,
                t("core.spawn_ms"),
                t("wallpaper.first_frame_ms"),
                t("widgets.first_paint_ms"),
                t("bars.native_paint_ms"),
                t("bars.first_paint_ms"),
                t("taskbar.hidden_ms"),
            );
        }

        md.push_str("\n## CPU\n\n");
        md.push_str(
            "Percent of the whole machine (of one core in parentheses), WebView2 processes \
             included in their host (or in *shared WebView2*). *Held* is paused from the tray \
             (`paused: true`).\n\n",
        );
        md.push_str(
            "| Scenario | screens visible | total | wallpaper | widgets | bars \
             | shared WebView2 | core | held total |\n",
        );
        md.push_str("|---|---|---|---|---|---|---|---|---|\n");
        for s in &self.scenarios {
            let c = |component: &str, phase: &str| cpu(&s.metrics, component, phase);
            let _ = writeln!(
                md,
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
                s.scenario,
                s.metrics
                    .get("screens.visible")
                    .map_or("-".into(), |v| format!("{v:.0}")),
                c("total", "steady"),
                c("wallpaper", "steady"),
                c("widgets", "steady"),
                c("bars", "steady"),
                c("webview", "steady"),
                c("core", "steady"),
                c("total", "held"),
            );
        }

        md.push_str("\n## Memory\n\n");
        md.push_str(
            "Private / working set in MB, running. A browser process both hosts share is \
             `shared WebView2`.\n\n",
        );
        md.push_str("| Scenario | total | core | wallpaper | widgets | bars | shared WebView2 |\n");
        md.push_str("|---|---|---|---|---|---|---|\n");
        for s in &self.scenarios {
            let m = |component: &str| memory(&s.metrics, component);
            let _ = writeln!(
                md,
                "| `{}` | {} | {} | {} | {} | {} | {} |",
                s.scenario,
                m("total"),
                m("core"),
                m("wallpaper"),
                m("widgets"),
                m("bars"),
                m("webview"),
            );
        }

        let noted: Vec<&Outcome> = self
            .scenarios
            .iter()
            .filter(|s| !s.notes.is_empty())
            .collect();
        if !noted.is_empty() {
            md.push_str("\n## Notes\n\n");
            for s in noted {
                for note in &s.notes {
                    let _ = writeln!(md, "- `{}`: {note}", s.scenario);
                }
            }
        }
        md
    }
}

fn cold_warm(metrics: &BTreeMap<String, f64>, key: &str) -> String {
    let get = |t: &str| metrics.get(&format!("{key}.{t}")).copied();
    match (get("cold"), get("warm")) {
        (None, None) => "-".into(),
        (Some(cold), None) => format!("{cold:.0}"),
        (None, Some(warm)) => format!("- / {warm:.0}"),
        (Some(cold), Some(warm)) => format!("{cold:.0} / {warm:.0}"),
    }
}

fn cpu(metrics: &BTreeMap<String, f64>, component: &str, phase: &str) -> String {
    let machine = metrics.get(&format!("{component}.cpu_{phase}_pct"));
    let core = metrics.get(&format!("{component}.cpu_{phase}_core_pct"));
    match (machine, core) {
        (Some(machine), Some(core)) => format!("{machine:.3}% ({core:.2}%)"),
        _ => "-".into(),
    }
}

fn memory(metrics: &BTreeMap<String, f64>, component: &str) -> String {
    let private = metrics.get(&format!("{component}.private_mb"));
    let working = metrics.get(&format!("{component}.working_set_mb"));
    match (private, working) {
        (Some(private), Some(working)) => format!("{private:.0} / {working:.0}"),
        _ => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_show_what_was_measured() {
        let metrics: BTreeMap<String, f64> = [
            ("wallpaper.first_frame_ms.cold", 201.4),
            ("wallpaper.first_frame_ms.warm", 98.6),
            ("bars.first_paint_ms.warm", 410.0),
            ("total.cpu_steady_pct", 0.171),
            ("total.cpu_steady_core_pct", 2.74),
            ("bars.private_mb", 40.2),
            ("bars.working_set_mb", 61.7),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        assert_eq!(cold_warm(&metrics, "wallpaper.first_frame_ms"), "201 / 99");
        assert_eq!(cold_warm(&metrics, "bars.first_paint_ms"), "- / 410");
        assert_eq!(cold_warm(&metrics, "widgets.first_paint_ms"), "-");
        assert_eq!(cpu(&metrics, "total", "steady"), "0.171% (2.74%)");
        assert_eq!(memory(&metrics, "bars"), "40 / 62");
        assert_eq!(memory(&metrics, "core"), "-");
    }
}
