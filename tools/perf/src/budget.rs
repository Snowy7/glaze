//! `perf/budget.toml`: the promised limits, and checking results against them.
//!
//! ```toml
//! [default]                      # every scenario
//! "wallpaper.first_frame_ms.warm" = 120
//!
//! [scenario.full]                # one scenario: adds limits or overrides defaults
//! "bars.first_paint_ms.warm" = 600
//! ```
//!
//! A metric is `<component>.<measure>`. A limit applies to a scenario when that component ran in
//! it, so `bars.*` limits in `[default]` only bind scenarios with the bars.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Limits for every scenario.
    #[serde(default)]
    pub default: BTreeMap<String, f64>,
    /// Extra or different limits per scenario, by name.
    #[serde(default)]
    pub scenario: BTreeMap<String, BTreeMap<String, f64>>,
}

impl Budget {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("perf/budget.toml")
    }

    /// The limits for `scenario`: the defaults, with its own on top.
    pub fn limits(&self, scenario: &str) -> BTreeMap<&str, f64> {
        let mut limits: BTreeMap<&str, f64> =
            self.default.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        if let Some(own) = self.scenario.get(scenario) {
            limits.extend(own.iter().map(|(k, v)| (k.as_str(), *v)));
        }
        limits
    }
}

/// A broken limit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Violation {
    pub scenario: String,
    pub metric: String,
    pub limit: f64,
    /// `None`: the metric applies but wasn't measured (a component never got ready).
    pub actual: Option<f64>,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            scenario,
            metric,
            limit,
            actual,
        } = self;
        match actual {
            Some(actual) => write!(f, "{scenario}: {metric} = {actual:.3} > {limit}"),
            None => write!(f, "{scenario}: {metric} was not measured (limit {limit})"),
        }
    }
}

/// What one scenario produced.
pub struct Measured<'a> {
    pub scenario: &'a str,
    /// Components that ran (`core`, `wallpaper`, `widgets`, `bars`, `taskbar`, `total`, ...).
    pub components: &'a BTreeSet<String>,
    pub metrics: &'a BTreeMap<String, f64>,
}

/// Every limit the scenarios break, in scenario then metric order.
pub fn evaluate(budget: &Budget, measured: &[Measured]) -> Vec<Violation> {
    let mut broken = Vec::new();
    for run in measured {
        for (metric, limit) in budget.limits(run.scenario) {
            let component = metric.split('.').next().unwrap_or(metric);
            if !run.components.contains(component) {
                continue;
            }
            let actual = run.metrics.get(metric).copied();
            if actual.is_none_or(|actual| actual > limit) {
                broken.push(Violation {
                    scenario: run.scenario.to_owned(),
                    metric: metric.to_owned(),
                    limit,
                    actual,
                });
            }
        }
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: &str = r#"
        [default]
        "wallpaper.first_frame_ms.warm" = 120
        "total.cpu_held_pct" = 0.05
        "bars.first_paint_ms.warm" = 600

        [scenario.video]
        "total.cpu_held_pct" = 0.1
    "#;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn metrics(items: &[(&str, f64)]) -> BTreeMap<String, f64> {
        items.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn scenario_limits_override_defaults() {
        let budget = Budget::parse(BUDGET).unwrap();
        assert_eq!(budget.limits("video")["total.cpu_held_pct"], 0.1);
        assert_eq!(budget.limits("shader")["total.cpu_held_pct"], 0.05);
    }

    #[test]
    fn reports_overruns_and_missing_metrics_of_components_that_ran() {
        let budget = Budget::parse(BUDGET).unwrap();
        let base = set(&["wallpaper", "total"]);
        let with_bars = set(&["wallpaper", "total", "bars"]);
        let fast = metrics(&[
            ("wallpaper.first_frame_ms.warm", 90.0),
            ("total.cpu_held_pct", 0.08),
        ]);
        let slow = metrics(&[
            ("wallpaper.first_frame_ms.warm", 130.0),
            ("total.cpu_held_pct", 0.0),
        ]);
        let broken = evaluate(
            &budget,
            &[
                // 0.08 is fine for video; no bars, so no bars limit.
                Measured {
                    scenario: "video",
                    components: &base,
                    metrics: &fast,
                },
                // Slow first frame; bars ran but never painted.
                Measured {
                    scenario: "full",
                    components: &with_bars,
                    metrics: &slow,
                },
            ],
        );
        let lines: Vec<String> = broken.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "full: bars.first_paint_ms.warm was not measured (limit 600)",
                "full: wallpaper.first_frame_ms.warm = 130.000 > 120",
            ]
        );
    }

    #[test]
    fn typos_in_the_budget_file_are_errors() {
        assert!(Budget::parse("[defaults]\n\"x\" = 1").is_err());
    }
}
