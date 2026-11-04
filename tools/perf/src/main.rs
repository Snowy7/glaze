//! mypaper-perf: measures a mypaper build and checks it against `perf/budget.toml`
//! (docs/performance.md).
//!
//! ```text
//! mypaper-perf [--build DIR] [--content DIR] [--runs N] [--seconds N] [--label TEXT]
//!              [--restart EXE] [--hold SECONDS] [--no-write] [SCENARIO...]
//! mypaper-perf --check perf/results/<report>.json     # a saved run against today's budget
//! mypaper-perf --list
//! ```
//!
//! It stops the user's mypaper under the desktop lock, runs each scenario in a temporary
//! `%APPDATA%` / `%LOCALAPPDATA%`, restarts the user's mypaper (always), writes
//! `perf/results/<date>.{md,json}` and `perf/latest.{md,json}`, and exits with 1 when a limit is
//! broken (listing every one), 2 when it couldn't run.
//! Native N0.3 measurement mode saves raw CPU/RAM samples and optional GPU JSONL instead;
//! its exit status validates the sample, not the native plan's GPU/lag gates.

mod budget;
mod cpu;
mod markers;
mod native;
mod procs;
mod report;
mod run;
mod scenario;
mod session;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation};
use windows::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::budget::{Budget, Measured};
use crate::report::Report;
use crate::run::Plan;
use crate::scenario::Scenario;
use crate::session::Session;

const USAGE: &str = "usage: mypaper-perf [--build DIR] [--content DIR] [--runs N] [--seconds N] \
                     [--label TEXT] [--restart EXE] [--hold SECONDS] [--no-write] [SCENARIO...] \
                     | --check REPORT.json | --list \
                     | --native --build DIR --content DIR [--timestamps] [--composition] NATIVE_SCENARIO";
/// Sessions stop the user's mypaper; each stays under this (the lock protocol's 5 minutes,
/// with room for stopping and restarting).
const SESSION_BUDGET: Duration = Duration::from_secs(4 * 60);

struct Args {
    build: PathBuf,
    content: PathBuf,
    runs: usize,
    seconds: u64,
    label: Option<String>,
    restart: Option<PathBuf>,
    hold: u64,
    write: bool,
    /// `--check REPORT.json`: evaluate a saved report against the budget instead of running.
    check: Option<PathBuf>,
    scenarios: Vec<&'static Scenario>,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// `Ok(true)` when every limit holds.
fn real_main() -> Result<bool> {
    if std::env::args().any(|a| a == "--native") {
        return native::run();
    }
    let Some(args) = parse_args()? else {
        return Ok(true);
    };
    let budget_path = args.content.join("perf").join("budget.toml");
    let budget = Budget::parse(
        &std::fs::read_to_string(&budget_path)
            .with_context(|| format!("reading {}", budget_path.display()))?,
    )?;
    for name in budget.scenario.keys() {
        if !scenario::ALL.iter().any(|s| s.name == name) {
            bail!("perf/budget.toml names an unknown scenario {name:?}");
        }
    }
    if let Some(report) = &args.check {
        return check_report(&budget, report);
    }
    for page in [
        "widgets/shell/dist/index.html",
        "bars/shell/dist/index.html",
    ] {
        if !args.content.join(page).is_file() {
            bail!(
                "{} is missing; build the shell pages first (README, Quick start)",
                args.content.join(page).display()
            );
        }
    }

    let stage = args.content.join("target").join("perf-stage");
    let temp = std::env::temp_dir().join("mypaper-perf");
    let lock = lock_path()?;
    let plan = Plan {
        runs: args.runs,
        settle: Duration::from_secs(5),
        window: Duration::from_secs(args.seconds),
        pause_settle: Duration::from_secs(3),
        hold: Duration::from_secs(args.hold),
    };
    // Fresh every time: a leftover folder could pass for content (see `stage_dir`).
    if let Err(e) = std::fs::remove_dir_all(&stage)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        bail!(
            "clearing {}: {e} (is a test process still running?)",
            stage.display()
        );
    }
    for scenario in &args.scenarios {
        stage_exes(&args.build, &stage_dir(&stage, scenario), scenario)?;
    }
    keep_std_handles_private();

    let mut outcomes = Vec::new();
    let mut failures = Vec::new();
    let mut queue = args.scenarios.iter().peekable();
    while queue.peek().is_some() {
        let session = Session::begin(&lock, &stage, args.restart.clone())?;
        while let Some(scenario) =
            queue.next_if(|_| session.started.elapsed() + plan.estimate() <= SESSION_BUDGET)
        {
            eprintln!("running {} ({})", scenario.name, scenario.about);
            match run::scenario(
                scenario,
                &session,
                &stage_dir(&stage, scenario),
                &temp,
                &args.content,
                &plan,
            ) {
                Ok(outcome) => outcomes.push(outcome),
                Err(e) => {
                    eprintln!("error: {}: {e:#}", scenario.name);
                    failures.push(format!("{}: {e:#}", scenario.name));
                    session::stop_test_processes(&stage);
                }
            }
        }
        drop(session); // restarts the user's mypaper, releases the lock
    }

    let measured: Vec<Measured> = outcomes
        .iter()
        .map(|o| Measured {
            scenario: &o.scenario,
            components: &o.components,
            metrics: &o.metrics,
        })
        .collect();
    let violations = budget::evaluate(&budget, &measured);
    let checked = budget::applicable(&budget, &measured);
    let (date, when) = now();
    let report = Report {
        when,
        label: args.label.clone(),
        machine: machine(),
        build: args.build.display().to_string(),
        plan: format!(
            "{} starts per scenario (1 cold, then warm), CPU windows of {} s after {} s settling",
            plan.runs,
            plan.window.as_secs(),
            plan.settle.as_secs()
        ),
        scenarios: outcomes,
        failures,
        violations,
        checked,
    };
    let markdown = report.markdown();
    println!("{markdown}");
    if args.write {
        write_reports(
            &args.content.join("perf"),
            &date,
            args.label.as_deref(),
            &report,
            &markdown,
        )?;
    }
    if !report.violations.is_empty() || !report.failures.is_empty() {
        eprintln!("budget exceeded:");
        for violation in &report.violations {
            eprintln!("  {violation}");
        }
        for failure in &report.failures {
            eprintln!("  failed: {failure}");
        }
        return Ok(false);
    }
    Ok(true)
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        build: std::env::current_exe()?
            .parent()
            .context("no exe folder")?
            .to_owned(),
        content: std::env::current_dir()?,
        runs: 4,
        seconds: 10,
        label: None,
        restart: None,
        hold: 0,
        write: true,
        check: None,
        scenarios: Vec::new(),
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = || {
            iter.next()
                .with_context(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--build" => args.build = PathBuf::from(value()?),
            "--content" => args.content = PathBuf::from(value()?),
            "--runs" => args.runs = value()?.parse::<usize>()?.max(1),
            "--seconds" => args.seconds = value()?.parse::<u64>()?.max(1),
            "--label" => args.label = Some(value()?),
            "--restart" => args.restart = Some(PathBuf::from(value()?)),
            "--hold" => args.hold = value()?.parse()?,
            "--no-write" => args.write = false,
            "--check" => args.check = Some(PathBuf::from(value()?)),
            "--list" => {
                for s in scenario::ALL {
                    println!("{:<10} {}", s.name, s.about);
                }
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            flag if flag.starts_with('-') => bail!("unknown flag {flag}\n{USAGE}"),
            name => args.scenarios.push(
                scenario::ALL
                    .iter()
                    .find(|s| s.name == name)
                    .with_context(|| format!("no scenario {name:?} (see --list)"))?,
            ),
        }
    }
    if args.scenarios.is_empty() {
        args.scenarios = scenario::ALL.iter().collect();
    }
    args.content = std::path::absolute(&args.content)?;
    args.build = std::path::absolute(&args.build)?;
    Ok(Some(args))
}

/// `--check`: evaluates a saved report (`perf/results/*.json`) against the budget, without
/// running anything: what a tightened budget says about an earlier run.
fn check_report(budget: &Budget, path: &Path) -> Result<bool> {
    #[derive(serde::Deserialize)]
    struct Saved {
        scenarios: Vec<SavedScenario>,
    }
    #[derive(serde::Deserialize)]
    struct SavedScenario {
        scenario: String,
        components: BTreeSet<String>,
        metrics: BTreeMap<String, f64>,
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let saved: Saved =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let measured: Vec<Measured> = saved
        .scenarios
        .iter()
        .map(|s| Measured {
            scenario: &s.scenario,
            components: &s.components,
            metrics: &s.metrics,
        })
        .collect();
    let violations = budget::evaluate(budget, &measured);
    let checked = budget::applicable(budget, &measured);
    if violations.is_empty() {
        println!("all {checked} limits are met by {}", path.display());
        return Ok(true);
    }
    println!("{} of {checked} limits broken:", violations.len());
    for violation in &violations {
        println!("  {violation}");
    }
    Ok(false)
}

/// Makes the harness's own stdin, stdout and stderr non-inheritable. Children are spawned with
/// handle inheritance on (for their own null std handles), so the user's restarted mypaper,
/// which outlives the harness, would otherwise keep the harness's output pipe open, and a shell
/// reading that pipe would wait for mypaper to exit.
fn keep_std_handles_private() {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: our own standard handles; clearing the inherit flag changes nothing else.
        unsafe {
            if let Ok(handle) = GetStdHandle(which)
                && !handle.is_invalid()
            {
                let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
            }
        }
    }
}

/// A scenario's folder under the stage. Prefixed: the executables look for `widgets\`,
/// `themes\`, `bars\` and `plugins\` in every folder above them, so a folder named after the
/// `widgets` scenario would pass for the bundled widgets.
fn stage_dir(stage: &Path, scenario: &Scenario) -> PathBuf {
    stage.join(format!("run-{}", scenario.name))
}

/// Copies the scenario's executables into its own folder under the repo, so they find the
/// bundled content (themes, widgets, shell pages) by walking up, and so a missing widget host
/// or bars binary means "off" to core.
///
/// Each copy is run once with an argument that only prints and exits: the antivirus scans a new
/// executable on its first launch (1 to 7 s here), which would otherwise land in the first
/// "cold" start. Cold means a fresh profile and caches, not an unscanned binary.
fn stage_exes(build: &Path, stage: &Path, scenario: &Scenario) -> Result<()> {
    let _ = std::fs::remove_dir_all(stage);
    std::fs::create_dir_all(stage)?;
    for exe in scenario.exes() {
        let from = build.join(exe);
        let to = stage.join(exe);
        std::fs::copy(&from, &to)
            .with_context(|| format!("copying {} (build it first)", from.display()))?;
        // Core's `--autostart` without a command only prints its usage.
        let probe = if exe == "mypaper-core.exe" {
            "--autostart"
        } else {
            "--help"
        };
        let _ = std::process::Command::new(&to)
            .arg(probe)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}

/// `%LOCALAPPDATA%\Temp\mypaper-tools\desktop.lock`, shared by every tool that stops the user's
/// mypaper.
fn lock_path() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local)
        .join("Temp")
        .join("mypaper-tools")
        .join("desktop.lock"))
}

/// (`YYYY-MM-DD-HHMM` for file names, `YYYY-MM-DD HH:MM` for people), local time.
fn now() -> (String, String) {
    // SAFETY: no preconditions.
    let t = unsafe { GetLocalTime() };
    let date = format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay);
    (
        format!("{date}-{:02}{:02}", t.wHour, t.wMinute),
        format!("{date} {:02}:{:02}", t.wHour, t.wMinute),
    )
}

fn machine() -> String {
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let monitors = mypaper_desktop::monitor_infos();
    let screens: Vec<String> = monitors
        .iter()
        .map(|m| format!("{}x{}", m.rect.width, m.rect.height))
        .collect();
    format!("{cpus} logical CPUs, screens {}", screens.join(" + "))
}

fn write_reports(
    dir: &Path,
    date: &str,
    label: Option<&str>,
    report: &Report,
    markdown: &str,
) -> Result<()> {
    let results = dir.join("results");
    std::fs::create_dir_all(&results)?;
    let stem = match label {
        Some(label) => format!(
            "{date}-{}",
            label.replace(|c: char| !c.is_alphanumeric(), "-")
        ),
        None => date.to_owned(),
    };
    let json = serde_json::to_string_pretty(report)? + "\n";
    for (path, text) in [
        (results.join(format!("{stem}.md")), markdown),
        (results.join(format!("{stem}.json")), json.as_str()),
        (dir.join("latest.md"), markdown),
        (dir.join("latest.json"), json.as_str()),
    ] {
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}
