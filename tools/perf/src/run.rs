//! Running one scenario: start core in the scenario's environment, time every component's
//! `READY` marker from core's creation, then sample CPU and memory of the whole process tree
//! (WebView2 included) while running and while paused.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::Threading::GetProcessTimes;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};
use windows::core::{PCWSTR, w};

use crate::cpu::{self, CpuUse, Times};
use crate::markers::{self, Marker};
use crate::procs::{self, Handle, Memory};
use crate::scenario::{Bars, Env, Scenario};
use crate::session::{self, Session};

/// How long a start may take before its missing markers count as "not measured".
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long core gets to shut everything down after `WM_QUIT`.
const QUIT_TIMEOUT: Duration = Duration::from_secs(15);

/// How a scenario is measured.
#[derive(Debug, Clone, Copy)]
pub struct Plan {
    /// Starts per scenario; the first is cold (fresh profile and caches), the rest warm.
    pub runs: usize,
    /// After the last start, before sampling: theme load, working-set trim and page loads
    /// settle.
    pub settle: Duration,
    /// Length of each CPU window.
    pub window: Duration,
    /// After pausing, before sampling.
    pub pause_settle: Duration,
}

impl Plan {
    /// A rough upper bound of one scenario's duration, for splitting sessions.
    pub fn estimate(&self) -> Duration {
        Duration::from_secs(6) * self.runs as u32
            + self.settle
            + self.pause_settle
            + self.window * 2
    }
}

/// One process in a sample.
#[derive(Debug, Clone, Serialize)]
pub struct ProcRow {
    pub pid: u32,
    pub name: String,
    /// `wallpaper`, `widgets.webview`, `webview.shared`, `widgets.plugins`, ...
    pub group: String,
    pub private_mb: f64,
    pub working_set_mb: f64,
}

/// One start: when each marker came, from core's creation and from its own process's.
#[derive(Debug, Clone, Serialize)]
pub struct Start {
    pub cold: bool,
    /// `component.metric` → (ms from core's creation, ms from its own process's creation).
    pub markers: BTreeMap<String, (f64, f64)>,
    /// When the native taskbar disappeared, ms from core's creation (hide scenarios).
    pub taskbar_hidden_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub scenario: String,
    pub about: String,
    /// Components that ran; budget limits for other components don't apply.
    pub components: BTreeSet<String>,
    /// `component.measure` → value (see perf/budget.toml for the names).
    pub metrics: BTreeMap<String, f64>,
    pub starts: Vec<Start>,
    /// The steady-state sample, per process.
    pub processes: Vec<ProcRow>,
    /// Anything a reader should know (covered screens, timeouts, errors).
    pub notes: Vec<String>,
}

/// Runs `scenario` with the executables in `stage`, in a fresh environment under `temp`.
pub fn scenario(
    scenario: &Scenario,
    session: &Session,
    stage: &Path,
    temp: &Path,
    content: &Path,
    plan: &Plan,
) -> Result<Outcome> {
    let env = Env::fresh(&temp.join(scenario.name), scenario, content)?;
    session.guard_journal(env.taskbar_journal());
    let mut outcome = Outcome {
        scenario: scenario.name.to_owned(),
        about: scenario.about.to_owned(),
        components: ["core", "wallpaper", "total"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        metrics: BTreeMap::new(),
        starts: Vec::new(),
        processes: Vec::new(),
        notes: Vec::new(),
    };
    if scenario.widgets {
        outcome.components.insert("widgets".into());
    }
    if scenario.bars != Bars::Off {
        outcome.components.insert("bars".into());
    }
    if scenario.bars == Bars::HideTaskbar {
        outcome.components.insert("taskbar".into());
    }

    let core_exe = stage.join("mypaper-core.exe");
    for run in 0..plan.runs {
        let last = run + 1 == plan.runs;
        let watch = (scenario.bars == Bars::HideTaskbar).then(TaskbarWatch::start);
        let mut core = Launched::start(&core_exe, &env)?;
        let found = core.wait_ready(&scenario.expected(), READY_TIMEOUT);
        let mut start = Start {
            cold: run == 0,
            markers: BTreeMap::new(),
            taskbar_hidden_ms: None,
        };
        for marker in &found {
            let from_core = (marker.epoch_us as f64 - core.created_us as f64) / 1000.0;
            start.markers.insert(
                format!("{}.{}", marker.component, marker.metric),
                (from_core, marker.own_ms),
            );
        }
        if let Some(watch) = watch {
            start.taskbar_hidden_ms = watch
                .finish(Duration::from_secs(5))
                .map(|at| (at as f64 - core.created_us as f64) / 1000.0);
        }
        for (component, metric) in scenario.expected() {
            if !found
                .iter()
                .any(|m| m.component == component && m.metric == metric)
            {
                outcome.notes.push(format!(
                    "run {run}: no READY component={component} {metric}_ms within {READY_TIMEOUT:?}"
                ));
            }
        }
        outcome.starts.push(start);

        if last {
            steady_and_held(&mut core, &env, plan, &mut outcome)?;
        }
        if let Some(note) = core.quit(stage) {
            outcome.notes.push(format!("run {run}: {note}"));
        }
    }
    startup_metrics(&mut outcome);
    Ok(outcome)
}

/// Samples the settled tree while running, then pauses and samples again.
fn steady_and_held(
    core: &mut Launched,
    env: &Env,
    plan: &Plan,
    outcome: &mut Outcome,
) -> Result<()> {
    std::thread::sleep(plan.settle);
    let (steady, rows) = sample_window(core.pid, plan.window);
    outcome.processes = rows;
    record(&mut outcome.metrics, "steady", &steady);

    let log = core.log_since_start();
    let screens = mypaper_desktop::monitors().len();
    let paused = screens_paused(&log);
    outcome.metrics.insert(
        "screens.visible".into(),
        screens.saturating_sub(paused) as f64,
    );
    if paused > 0 {
        outcome.notes.push(format!(
            "{paused} of {screens} screens were covered by windows during the steady sample \
             (the wallpaper and widgets pause there)"
        ));
    }

    env.set_paused(true)?;
    std::thread::sleep(plan.pause_settle);
    let (held, _) = sample_window(core.pid, plan.window);
    record(&mut outcome.metrics, "held", &held);
    Ok(())
}

/// What one CPU window measured, per group of processes.
struct Window {
    cpu: BTreeMap<String, CpuUse>,
    memory: BTreeMap<String, Memory>,
}

/// Puts a window's per-component CPU and memory into `metrics` under `phase`.
fn record(metrics: &mut BTreeMap<String, f64>, phase: &str, window: &Window) {
    let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    for (component, used) in &window.cpu {
        metrics.insert(format!("{component}.cpu_{phase}_pct"), used.machine);
        metrics.insert(format!("{component}.cpu_{phase}_core_pct"), used.one_core);
    }
    for (component, memory) in &window.memory {
        let suffix = if phase == "steady" {
            String::new()
        } else {
            format!("_{phase}")
        };
        metrics.insert(
            format!("{component}.private{suffix}_mb"),
            mb(memory.private),
        );
        metrics.insert(
            format!("{component}.working_set{suffix}_mb"),
            mb(memory.working_set),
        );
    }
}

/// Median startup times: run 0 is `cold`, the median of the others `warm`.
fn startup_metrics(outcome: &mut Outcome) {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for start in &outcome.starts {
        keys.extend(start.markers.keys().cloned());
    }
    let mut put = |name: String, values: Vec<f64>| {
        if let Some(value) = cpu::median(&values) {
            outcome.metrics.insert(name, value);
        }
    };
    for key in keys {
        for (temperature, cold) in [("cold", true), ("warm", false)] {
            let pick = |own: bool| {
                outcome
                    .starts
                    .iter()
                    .filter(|s| s.cold == cold)
                    .filter_map(|s| s.markers.get(&key))
                    .map(|&(from_core, own_ms)| if own { own_ms } else { from_core })
                    .collect::<Vec<_>>()
            };
            put(format!("{key}_ms.{temperature}"), pick(false));
            put(format!("{key}_own_ms.{temperature}"), pick(true));
        }
    }
    for (temperature, cold) in [("cold", true), ("warm", false)] {
        let hidden: Vec<f64> = outcome
            .starts
            .iter()
            .filter(|s| s.cold == cold)
            .filter_map(|s| s.taskbar_hidden_ms)
            .collect();
        put(format!("taskbar.hidden_ms.{temperature}"), hidden);
    }
}

/// Net number of wallpaper screens paused for being covered, from the run's log.
fn screens_paused(log: &str) -> usize {
    let count = |needle: &str| log.lines().filter(|l| l.contains(needle)).count();
    let paused = count("mypaper_wallpaper::engine] pausing");
    let resumed = count("mypaper_wallpaper::engine] resuming");
    paused.saturating_sub(resumed)
}

/// Samples the tree under `root` twice, `window` apart: CPU per component and the memory of
/// the second sample.
fn sample_window(root: u32, window: Duration) -> (Window, Vec<ProcRow>) {
    let before = sample(root);
    let started = Instant::now();
    std::thread::sleep(window);
    let after = sample(root);
    let wall = started.elapsed();
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);

    let mut cpu = BTreeMap::new();
    let mut memory: BTreeMap<String, Memory> = BTreeMap::new();
    let components: BTreeSet<&str> = after.iter().map(|s| component(&s.group)).collect();
    for name in components.iter().copied().chain(["total"]) {
        let pick = |samples: &[Sampled]| -> HashMap<u32, Times> {
            samples
                .iter()
                .filter(|s| name == "total" || component(&s.group) == name)
                .map(|s| (s.pid, s.times))
                .collect()
        };
        let busy = cpu::busy_between(&pick(&before), &pick(&after));
        cpu.insert(name.to_owned(), cpu::cpu_use(busy, wall, cpus));
        let total = memory.entry(name.to_owned()).or_default();
        for s in after
            .iter()
            .filter(|s| name == "total" || component(&s.group) == name)
        {
            total.private += s.memory.private;
            total.working_set += s.memory.working_set;
        }
    }
    let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    let rows = after
        .iter()
        .map(|s| ProcRow {
            pid: s.pid,
            name: s.name.clone(),
            group: s.group.clone(),
            private_mb: mb(s.memory.private),
            working_set_mb: mb(s.memory.working_set),
        })
        .collect();
    (Window { cpu, memory }, rows)
}

/// A group's component: `widgets.webview` counts as `widgets`.
fn component(group: &str) -> &str {
    group.split('.').next().unwrap_or(group)
}

struct Sampled {
    pid: u32,
    name: String,
    group: String,
    times: Times,
    memory: Memory,
}

/// Every process under `root` with its group, CPU times and memory.
fn sample(root: u32) -> Vec<Sampled> {
    let all = procs::list();
    let mut groups: HashMap<u32, String> = HashMap::new();
    let mut out = Vec::new();
    for proc in procs::tree(root, &all, procs::created) {
        let Some(handle) = Handle::query(proc.pid) else {
            continue;
        };
        let (Some(times), Some(memory)) = (handle.times(), handle.memory()) else {
            continue;
        };
        let parent = groups.get(&proc.parent).cloned();
        let group = group_of(
            &proc.name,
            handle.command_line().as_deref(),
            parent.as_deref(),
        );
        groups.insert(proc.pid, group.clone());
        out.push(Sampled {
            pid: proc.pid,
            name: proc.name,
            group,
            times,
            memory,
        });
    }
    out
}

/// Which component a process belongs to. mypaper's own executables are themselves; a WebView2
/// process belongs to whoever owns its user data folder (`webview2-<host>`; a folder used by
/// both hosts is `webview.shared`), or to its parent's group when its command line doesn't
/// say; anything else (plugins, a wallpaper app) is its parent's `.children`.
pub fn group_of(name: &str, command_line: Option<&str>, parent: Option<&str>) -> String {
    let name = name.to_ascii_lowercase();
    if let Some(own) = name
        .strip_prefix("mypaper-")
        .and_then(|n| n.strip_suffix(".exe"))
    {
        return own.to_owned();
    }
    let parent_component = parent.map(component);
    if name == "msedgewebview2.exe" {
        let folder = command_line.and_then(procs::user_data_dir).and_then(|dir| {
            Path::new(&dir)
                .file_name()
                .map(|f| f.to_string_lossy().to_ascii_lowercase())
        });
        return match folder.as_deref() {
            Some("webview2-widgets") => "widgets.webview".into(),
            Some("webview2-bars") => "bars.webview".into(),
            Some("webview2") => "wallpaper.webview".into(),
            Some(_) => "webview.shared".into(),
            None => parent.unwrap_or("webview.shared").to_owned(),
        };
    }
    match parent_component {
        Some("webview") | None => parent.unwrap_or("other").to_owned(),
        Some(owner) if parent.is_some_and(|p| p.contains(".webview")) => {
            format!("{owner}.webview")
        }
        Some(owner) => format!("{owner}.children"),
    }
}

/// A running test core.
struct Launched {
    child: Child,
    pid: u32,
    /// Core's creation time, Unix microseconds.
    created_us: u64,
    log: PathBuf,
    log_start: u64,
}

impl Launched {
    fn start(exe: &Path, env: &Env) -> Result<Self> {
        let log = env.log();
        let log_start = std::fs::metadata(&log).map_or(0, |m| m.len());
        let child = Command::new(exe)
            .current_dir(exe.parent().context("core has no folder")?)
            .env("APPDATA", &env.appdata)
            .env("LOCALAPPDATA", &env.local)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;
        let pid = child.id();
        let created_us = creation_epoch_us(HANDLE(child.as_raw_handle()))
            .context("reading core's creation time")?;
        Ok(Self {
            child,
            pid,
            created_us,
            log,
            log_start,
        })
    }

    fn log_since_start(&self) -> String {
        let bytes = std::fs::read(&self.log).unwrap_or_default();
        let start = (self.log_start as usize).min(bytes.len());
        String::from_utf8_lossy(&bytes[start..]).into_owned()
    }

    /// Waits until every `expected` marker is in the log, or `timeout`. Returns what came.
    fn wait_ready(&mut self, expected: &[(&str, &str)], timeout: Duration) -> Vec<Marker> {
        let until = Instant::now() + timeout;
        loop {
            let found = markers::parse_all(&self.log_since_start());
            let complete = expected.iter().all(|(component, metric)| {
                found
                    .iter()
                    .any(|m| m.component == *component && m.metric == *metric)
            });
            let exited = self.child.try_wait().ok().flatten().is_some();
            if complete || exited || Instant::now() >= until {
                return found;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Asks core to quit and waits for its whole tree. Returns a note if it had to be killed.
    fn quit(&mut self, stage: &Path) -> Option<String> {
        let tree: Vec<u32> = procs::tree(self.pid, &procs::list(), procs::created)
            .iter()
            .map(|p| p.pid)
            .collect();
        procs::post_quit(self.pid);
        let exited = procs::wait_until(QUIT_TIMEOUT, || {
            self.child.try_wait().ok().flatten().is_some()
        });
        let mut note = None;
        if !exited {
            let _ = self.child.kill();
            let _ = self.child.wait();
            session::stop_test_processes(stage);
            note = Some(format!("core didn't quit within {QUIT_TIMEOUT:?}; killed"));
        }
        // Children (WebView2 included) follow their hosts out.
        let gone = procs::wait_until(Duration::from_secs(5), || {
            let alive: Vec<u32> = procs::list().iter().map(|p| p.pid).collect();
            !tree.iter().any(|pid| alive.contains(pid))
        });
        if !gone {
            note.get_or_insert_with(|| "some child processes outlived core by 5 s".into());
        }
        note
    }
}

fn creation_epoch_us(process: HANDLE) -> Option<u64> {
    let mut created = FILETIME::default();
    let (mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: the child's live process handle; the out-pointers outlive the call.
    unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    Some(procs::ticks_to_epoch_us(procs::ticks(created)))
}

/// Watches the native taskbar from just before core starts: when it stops being visible.
struct TaskbarWatch {
    done: Arc<AtomicBool>,
    hidden_at: Arc<AtomicU64>,
    thread: std::thread::JoinHandle<()>,
}

impl TaskbarWatch {
    fn start() -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let hidden_at = Arc::new(AtomicU64::new(0));
        let thread = {
            let (done, hidden_at) = (done.clone(), hidden_at.clone());
            std::thread::spawn(move || {
                let visible = || {
                    // SAFETY: read-only window queries.
                    unsafe {
                        FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())
                            .is_ok_and(|tray| IsWindowVisible(tray).as_bool())
                    }
                };
                if !visible() {
                    return; // already hidden: nothing to time
                }
                while !done.load(Ordering::Relaxed) {
                    if !visible() {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_micros() as u64);
                        hidden_at.store(now, Ordering::Relaxed);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        Self {
            done,
            hidden_at,
            thread,
        }
    }

    /// When the taskbar went away (Unix microseconds), waiting up to `grace` more.
    fn finish(self, grace: Duration) -> Option<u64> {
        procs::wait_until(grace, || self.hidden_at.load(Ordering::Relaxed) != 0);
        self.done.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
        Some(self.hidden_at.load(Ordering::Relaxed)).filter(|&at| at != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_group_by_component_and_webview_folder() {
        let webview =
            |dir: &str| format!("msedgewebview2.exe --user-data-dir=\"{dir}\\EBWebView\"");
        assert_eq!(group_of("mypaper-bars.exe", None, Some("core")), "bars");
        assert_eq!(
            group_of(
                "msedgewebview2.exe",
                Some(&webview(r"C:\t\mypaper\webview2-widgets")),
                Some("widgets")
            ),
            "widgets.webview"
        );
        assert_eq!(
            group_of(
                "msedgewebview2.exe",
                Some(&webview(r"C:\t\mypaper\webview2-shell")),
                Some("bars")
            ),
            "webview.shared"
        );
        // A renderer whose command line names no folder follows its browser process.
        assert_eq!(
            group_of(
                "msedgewebview2.exe",
                Some("--type=renderer"),
                Some("bars.webview")
            ),
            "bars.webview"
        );
        assert_eq!(
            group_of("weather.exe", None, Some("widgets")),
            "widgets.children"
        );
        assert_eq!(
            group_of("crashpad.exe", None, Some("widgets.webview")),
            "widgets.webview"
        );
    }

    #[test]
    fn covered_screens_net_out_resumes() {
        let log = "x mypaper_wallpaper::engine] pausing 2560x1440 screen\n\
                   x mypaper_wallpaper::engine] pausing 2560x1440 screen\n\
                   x mypaper_wallpaper::engine] resuming 2560x1440 screen\n";
        assert_eq!(screens_paused(log), 1);
    }
}
