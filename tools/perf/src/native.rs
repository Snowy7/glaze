//! N0.3/N2 engine-only scenarios. Same desktop lock/restore protocol as the product harness,
//! with a native plate writer instead of the deleted bars/widgets. Refuses covered screens.
use crate::{cpu, procs, session::Session};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::BOOL;

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        procs::post_quit(self.0.id());
        if !procs::wait_until(Duration::from_secs(2), || {
            self.0.try_wait().ok().flatten().is_some()
        }) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Save placements before minimizing, then restore exactly those windows on every exit.
struct DesktopView(Vec<(HWND, WINDOWPLACEMENT)>);
impl DesktopView {
    fn uncover() -> Self {
        unsafe extern "system" fn each(hwnd: HWND, param: LPARAM) -> BOOL {
            // SAFETY: enumeration is synchronous; param points at the live vector below.
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    return BOOL(1);
                }
                let style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
                if style.0 & (WS_CAPTION.0 | WS_THICKFRAME.0) == 0 {
                    return BOOL(1);
                }
                let mut placement = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                if GetWindowPlacement(hwnd, &mut placement).is_ok() {
                    (&mut *(param.0 as *mut Vec<(HWND, WINDOWPLACEMENT)>)).push((hwnd, placement));
                    let _ = ShowWindowAsync(hwnd, SW_MINIMIZE);
                }
            }
            BOOL(1)
        }
        let mut saved = Vec::new();
        // SAFETY: callback receives the vector only for the duration of enumeration.
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut saved as isize)) };
        std::thread::sleep(Duration::from_millis(500));
        Self(saved)
    }
}
impl Drop for DesktopView {
    fn drop(&mut self) {
        for (hwnd, placement) in &self.0 {
            // SAFETY: only restores windows this harness minimized; stale HWNDs are ignored.
            let _ = unsafe { SetWindowPlacement(*hwnd, placement) };
        }
    }
}

#[derive(Serialize)]
struct Sample {
    scenario: String,
    run: usize,
    seconds: f64,
    private_mb: f64,
    cpu_core_pct: f64,
    cpu_machine_pct: f64,
    visible_screens: usize,
    timing_file: Option<PathBuf>,
    sample_started_qpc: u64,
    sample_ended_qpc: u64,
}

pub fn run() -> Result<bool> {
    mypaper_desktop::enable_dpi_awareness();
    let args: Vec<_> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let build = PathBuf::from(option("--build").context("--native needs --build DIR")?);
    let content = PathBuf::from(option("--content").context("--native needs --content DIR")?);
    let seconds: u64 = option("--seconds").unwrap_or("60".into()).parse()?;
    let runs: usize = option("--runs").unwrap_or("3".into()).parse()?;
    ensure!(
        runs > 0 && (1..=180).contains(&seconds),
        "runs must be positive and seconds 1..=180"
    );
    let scenario = args.last().context("native scenario is required")?;
    let (default_theme, animate) = match scenario.as_str() {
        "engine-aurora" => ("examples/aurora", false),
        "real-look" => ("rain-city", false),
        "glass-motion" => ("rain-city", true),
        "still-plates" => ("examples/photo", false),
        "still-motion" => ("examples/photo", true),
        _ => bail!(
            "native scenarios: engine-aurora, real-look, glass-motion, still-plates, still-motion"
        ),
    };
    let theme = option("--theme").unwrap_or_else(|| default_theme.into());
    let output = PathBuf::from(
        option("--output")
            .unwrap_or_else(|| build.join(format!("{scenario}.json")).display().to_string()),
    );
    let stage = build.join("native-stage");
    std::fs::create_dir_all(&stage)?;
    std::fs::copy(
        build.join("mypaper-wallpaper.exe"),
        stage.join("mypaper-wallpaper.exe"),
    )?;
    std::fs::copy(
        build.join("examples/plates.exe"),
        stage.join("mypaper-plates.exe"),
    )?;
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA")?);
    let lock = local.join("Temp/mypaper-tools/desktop.lock");
    let refresh_hz: Vec<_> = mypaper_desktop::monitor_infos()
        .iter()
        .map(|monitor| {
            use windows::Win32::Graphics::Gdi::{
                DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
            };
            let mut mode = DEVMODEW {
                dmSize: size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            // SAFETY: initialized DEVMODE output for the enumerated display name.
            let ok = unsafe {
                EnumDisplaySettingsW(
                    &windows::core::HSTRING::from(&monitor.device),
                    ENUM_CURRENT_SETTINGS,
                    &mut mode,
                )
            }
            .as_bool();
            let hz = if ok {
                mode.dmDisplayFrequency.max(1)
            } else {
                60
            };
            eprintln!("{}: {:?}, {hz} Hz", monitor.device, monitor.rect);
            hz
        })
        .collect();
    let hz = option("--hz")
        .unwrap_or_else(|| refresh_hz.iter().copied().max().unwrap_or(60).to_string());
    let mut samples = Vec::new();
    for run in 0..runs {
        let session = Session::begin(&lock, &stage, None)?;
        let desktop = DesktopView::uncover();
        let monitors = mypaper_desktop::monitors();
        ensure!(
            monitors.len() == 2 && monitors.iter().all(|m| (m.width, m.height) == (2560, 1440)),
            "N0.3 requires two 2560x1440 monitors: {monitors:?}"
        );
        ensure!(
            mypaper_desktop::covered_monitors(&monitors)
                .iter()
                .all(|c| !c),
            "screens remain covered"
        );
        let env = stage.join(format!("env-{run}"));
        let settings = env.join("appdata/mypaper");
        std::fs::create_dir_all(&settings)?;
        std::fs::create_dir_all(env.join("local"))?;
        std::fs::write(
            settings.join("config.json"),
            r#"{"theme":"aurora","paused":false,"pause_on_battery":false,"pause_on_fullscreen":false,"static_snapshot":false}"#,
        )?;
        let timing_file = args
            .iter()
            .any(|a| a == "--timestamps")
            .then(|| output.with_extension(format!("run{run}.jsonl")));
        let mut command = Command::new(stage.join("mypaper-wallpaper.exe"));
        command
            .arg(content.join("themes").join(&theme))
            .env("APPDATA", env.join("appdata"))
            .env("LOCALAPPDATA", env.join("local"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = &timing_file {
            command.env("MYPAPER_GPU_TIMINGS", path);
        }
        if args.iter().any(|a| a == "--composition") {
            command.env("MYPAPER_COMPOSITION_TIMING", "1");
        }
        let mut engine = Running(command.spawn()?);
        let mut writer_command = Command::new(stage.join("mypaper-plates.exe"));
        writer_command
            .args([
                "--seconds",
                &(seconds
                    + 20
                    + if args.iter().any(|a| a == "--exercise-settings") {
                        30
                    } else {
                        0
                    })
                .to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if animate {
            writer_command.args(["--animate", "--hz", &hz]);
        }
        let writer = Running(writer_command.spawn()?);
        std::thread::sleep(Duration::from_secs(6));
        if let Some(path) = option("--capture") {
            ensure!(
                Command::new("powershell.exe")
                    .args(["-NoProfile", "-File"])
                    .arg(content.join("tools/perf/capture.ps1"))
                    .args(["-Out", &path])
                    .status()?
                    .success(),
                "desktop capture failed"
            );
        }
        if args.iter().any(|a| a == "--exercise-settings") {
            exercise_settings(
                &mut engine,
                &mut command,
                &settings.join("config.json"),
                &env.join("local/mypaper/mypaper.log"),
                timing_file.as_deref(),
            )?;
        }
        ensure!(
            engine.0.try_wait()?.is_none(),
            "engine exited: inspect {}",
            env.join("local/mypaper/mypaper.log").display()
        );
        let handle = procs::Handle::query(engine.0.id()).context("query engine")?;
        let before = handle.times().context("engine times")?;
        let mut qpc_start = 0;
        // SAFETY: valid QPC output, shared clock with the engine's telemetry.
        unsafe {
            windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc_start)?;
        }
        let started = Instant::now();
        eprintln!(
            "{scenario}: sample {}/{runs}, {seconds}s, engine {}",
            run + 1,
            engine.0.id()
        );
        // Check actual visibility each second; never pass a paused/covered sample as two-screen.
        for _ in 0..seconds {
            std::thread::sleep(Duration::from_secs(1));
            if mypaper_desktop::covered_monitors(&monitors)
                .iter()
                .any(|c| *c)
            {
                unsafe extern "system" fn dump(hwnd: HWND, _: LPARAM) -> BOOL {
                    // SAFETY: read-only queries of enumerated windows.
                    unsafe {
                        if IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
                            let mut title = [0; 256];
                            let n = GetWindowTextW(hwnd, &mut title);
                            let mut class = [0; 128];
                            let c = GetClassNameW(hwnd, &mut class);
                            let mut rect = Default::default();
                            let _ = GetWindowRect(hwnd, &mut rect);
                            eprintln!(
                                "visible {:?} class={} title={} maximized={}",
                                rect,
                                String::from_utf16_lossy(&class[..c as usize]),
                                String::from_utf16_lossy(&title[..n as usize]),
                                IsZoomed(hwnd).as_bool()
                            );
                        }
                    }
                    BOOL(1)
                }
                // SAFETY: read-only synchronous enumeration.
                let _ = unsafe { EnumWindows(Some(dump), LPARAM(0)) };
                bail!("a screen became covered during the sample");
            }
        }
        let elapsed = started.elapsed();
        let mut qpc_end = 0;
        // SAFETY: valid QPC output.
        unsafe {
            windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc_end)?;
        }
        let after = handle.times().context("engine times")?;
        let memory = handle.memory().context("engine memory")?;
        let used = cpu::cpu_use(
            Duration::from_nanos(after.busy.saturating_sub(before.busy) * 100),
            elapsed,
            std::thread::available_parallelism()?.get() as u32,
        );
        samples.push(Sample {
            scenario: scenario.clone(),
            run,
            seconds: elapsed.as_secs_f64(),
            private_mb: memory.private as f64 / 1048576.0,
            cpu_core_pct: used.one_core,
            cpu_machine_pct: used.machine,
            visible_screens: 2,
            timing_file,
            sample_started_qpc: qpc_start as u64,
            sample_ended_qpc: qpc_end as u64,
        });
        drop(writer);
        drop(engine);
        drop(desktop);
        drop(session);
        std::fs::write(&output, serde_json::to_vec_pretty(&samples)?)?;
    }
    let private = cpu::median(&samples.iter().map(|s| s.private_mb).collect::<Vec<_>>()).unwrap();
    let cpu = cpu::median(&samples.iter().map(|s| s.cpu_core_pct).collect::<Vec<_>>()).unwrap();
    println!(
        "{scenario}: median private {private:.2} MiB, CPU {cpu:.3}% of one core; {}",
        output.display()
    );
    Ok(true)
}

/// Verify FPS in process and the GPU-restart contract used by core's existing supervisor.
fn exercise_settings(
    engine: &mut Running,
    command: &mut Command,
    config: &std::path::Path,
    log: &std::path::Path,
    timing: Option<&std::path::Path>,
) -> Result<()> {
    let update = |gpu: &str, fps: Option<u32>| -> Result<()> {
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(config)?)?;
        value["engine"] = serde_json::json!({"gpu":gpu,"fps":fps});
        std::fs::write(config, serde_json::to_vec(&value)?)?;
        Ok(())
    };
    let counter = || -> Result<u64> {
        let mut now = 0; // SAFETY: valid QPC output.
        unsafe {
            windows::Win32::System::Performance::QueryPerformanceCounter(&mut now)?;
        }
        Ok(now as u64)
    };
    update("integrated", Some(12))?;
    std::thread::sleep(Duration::from_secs(1));
    ensure!(
        engine.0.try_wait()?.is_none(),
        "FPS change restarted the engine"
    );
    let start = counter()?;
    std::thread::sleep(Duration::from_secs(2));
    let end = counter()?;
    let text = std::fs::read_to_string(log)?;
    ensure!(
        text.contains("gpu: Integrated, fps: Some(12)"),
        "FPS setting was not applied"
    );
    for gpu in ["high-performance", "auto", "integrated"] {
        update(gpu, None)?;
        let until = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = engine.0.try_wait()? {
                break status;
            }
            ensure!(
                Instant::now() < until,
                "GPU setting did not request a restart"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        ensure!(
            status.code() == Some(i32::from(mypaper_desktop::EXIT_RESTART)),
            "unexpected GPU restart exit: {status}"
        );
        if gpu == "high-performance"
            && let Some(timing) = timing
        {
            let rows: Vec<serde_json::Value> = std::fs::read_to_string(timing)?
                .lines()
                .map(serde_json::from_str)
                .collect::<std::result::Result<_, _>>()?;
            for screen in 0..2 {
                let count = rows
                    .iter()
                    .filter(|r| {
                        r["screen"] == screen
                            && r["wallpaper_drawn"] == true
                            && r["submit_qpc"]
                                .as_u64()
                                .is_some_and(|q| q >= start && q <= end)
                    })
                    .count();
                ensure!(
                    (20..=28).contains(&count),
                    "12 fps cap rendered {count} frames on screen {screen} in 2 s"
                );
            }
        }
        *engine = Running(command.spawn()?);
        std::thread::sleep(Duration::from_secs(3));
        ensure!(
            engine.0.try_wait()?.is_none(),
            "engine failed after selecting {gpu}"
        );
        eprintln!("live renderer setting verified: {gpu}");
    }
    Ok(())
}
