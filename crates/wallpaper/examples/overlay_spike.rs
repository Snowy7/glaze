//! N0.2 spike, the engine side: runs `src/overlay` as the engine will (wgpu opened the way
//! gpu.rs opens it, the overlays driven from a message loop) and measures every N0.2 check live
//! against client processes (`overlay_spike_popup.rs`).
//!
//! ```text
//! cargo build --release -p mypaper-wallpaper --example overlay_spike --example overlay_spike_popup
//! target/release/examples/overlay_spike popup [SECONDS]   # popups over app windows
//! target/release/examples/overlay_spike desktop [SECONDS] # a widget over a web wallpaper (D-W)
//! ```
//!
//! Take the desktop lock first (docs/engine-notes.md, "Glass overlays"). Every window it makes
//! is in a child process it kills before exiting. `desktop` expects a web wallpaper already
//! showing on the primary monitor. Pictures go to `%TEMP%\mypaper-overlay-spike\`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmGetCompositionTimingInfo};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThreadId, GetProcessTimes};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GA_ROOT, GetAncestor, GetCursorPos, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT,
    SetCursorPos, TranslateMessage, WM_APP, WindowFromPoint,
};

#[path = "../src/material.rs"]
#[allow(dead_code)]
mod material;
#[path = "../src/overlay/mod.rs"]
#[allow(dead_code)]
mod overlay;

use overlay::{Gpu, Overlays, Stats};

/// Where everything goes, on the primary monitor's still lower-left corner (the wallpaper above
/// it animates).
const BACKDROP: Rect = rect(30, 1100, 560, 280);
const POPUP: Rect = rect(60, 1130, 250, 220);
const RADIUS: i32 = 16;
/// The second popup opens beside the first: x 310..560.
const ELSEWHERE: Rect = rect(1400, 250, 800, 450);
const BEHIND: Rect = rect(40, 1115, 290, 250);
/// The widget over the web wallpaper (`desktop`).
const WIDGET: Rect = rect(1200, 600, 320, 240);
/// A client line arrived.
const WM_LINE: u32 = WM_APP + 7;

const fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Logs to stderr (not the user's mypaper.log).
struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> Result<()> {
    static LOGGER: Stderr = Stderr;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
    mypaper_desktop::enable_dpi_awareness();
    // SAFETY: the engine's apartment (window.rs), kept for the process.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
    let mut args = std::env::args().skip(1);
    let scenario = args.next().unwrap_or_else(|| "popup".into());
    let seconds: u64 = args.next().map_or(Ok(15), |s| s.parse())?;
    let gpu = open_gpu()?;
    println!(
        "adapter: {} (timestamps: {})",
        gpu.adapter.get_info().name,
        gpu.device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    );
    let mut overlays = Overlays::new(gpu)?;
    overlays.set_timing(std::env::var_os("MYPAPER_SPIKE_NO_TIMING").is_none());
    let mut harness = Harness {
        overlays,
        children: Vec::new(),
        ticks: Vec::new(),
        record: false,
        rows: Vec::new(),
        notes: Vec::new(),
        out: std::env::temp_dir().join("mypaper-overlay-spike"),
        // SAFETY: a plain query.
        thread: unsafe { GetCurrentThreadId() },
        phase: Duration::from_secs(seconds),
    };
    std::fs::create_dir_all(&harness.out)?;
    let result = match scenario.as_str() {
        "popup" => harness.popup(),
        "desktop" => harness.desktop(),
        other => Err(anyhow::anyhow!("unknown scenario {other}")),
    };
    // Whatever happened, every window we made goes.
    for child in &mut harness.children {
        let _ = child.process.kill();
    }
    harness.report();
    result
}

/// wgpu as gpu.rs opens it (DX12, FXC, the low-power adapter), plus timestamp queries.
fn open_gpu() -> Result<Gpu> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("overlay spike"),
        required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))?;
    device.on_uncaptured_error(Arc::new(|e| log::error!("GPU: {e}")));
    Ok(Gpu {
        adapter,
        device,
        queue,
    })
}

/// A client process and what it printed.
struct Client {
    process: Child,
    stdin: ChildStdin,
    lines: Arc<Mutex<Vec<(i64, String)>>>,
    hwnd: HWND,
}

/// One measured phase.
struct Row {
    name: String,
    seconds: f64,
    cpu: f64,
    stats: Stats,
    private_mb: f64,
}

/// A message-loop tick that did something: when, and the glass passes drawn so far.
#[derive(Clone, Copy)]
struct Tick {
    qpc: i64,
    passes: u64,
}

struct Harness {
    overlays: Overlays,
    children: Vec<Client>,
    ticks: Vec<Tick>,
    record: bool,
    rows: Vec<Row>,
    notes: Vec<String>,
    out: PathBuf,
    thread: u32,
    phase: Duration,
}

impl Harness {
    /// Popups over app windows (the topmost variant).
    fn popup(&mut self) -> Result<()> {
        let still = self.spawn(&["static", &r(on(BACKDROP))])?;
        self.idle(Duration::from_millis(800));
        let base = private_mb();
        self.note(format!("private before any overlay: {base:.1} MB"));
        let yellow_before = yellow_edges(monitor_bounds());

        let popup = self.spawn(&["popup", &r(on(POPUP)), &RADIUS.to_string()])?;
        self.idle(Duration::from_millis(300));
        let opened = Instant::now();
        self.send(popup, "open")?;
        let shown = self.pump_until(Instant::now() + Duration::from_secs(5), |h| {
            h.overlays.stats().passes > 0
        });
        let first_glass = opened.elapsed();
        if !shown {
            bail!("no glass within 5 s");
        }
        self.note(format!(
            "open-to-glass: {:.1} ms to the first glass present (engine side {:?}); fully in {:.0} ms later (the DirectComposition fade)",
            first_glass.as_secs_f64() * 1000.0,
            self.overlays.stats().open_to_glass,
            300.0
        ));
        self.idle(Duration::from_millis(600));
        self.check_exclusion(popup, on(POPUP))?;
        let yellow_open = yellow_edges(monitor_bounds());
        self.note(format!(
            "capture border: yellow edge pixels {yellow_before} before, {yellow_open} with the session open"
        ));
        self.shot("popup-open", grow(on(BACKDROP), 20))?;
        self.idle(Duration::from_millis(1500));
        let one = private_mb();
        self.note(format!(
            "private with one overlay open: {one:.1} MB (+{:.1})",
            one - base
        ));

        self.measure(
            "over a small app window, mypaper's animated wallpaper running around it (DWM reports it dirty under the window)",
        )?;

        // A plain app window over the whole monitor: mypaper's wallpaper pauses there, as under
        // a maximized app, so what changes from here on is only what the phases make change.
        let cover = self.spawn(&["static", &r(monitor_bounds()), "cover"])?;
        self.idle(Duration::from_secs(2));
        self.measure("idle: over a maximized app, nothing changes")?;

        let video = self.spawn(&["animate", &r(on(ELSEWHERE))])?;
        self.measure("a 60 fps video elsewhere on the monitor")?;
        self.kill(video);

        self.send(popup, "hover on")?;
        self.send(popup, "clock on")?;
        self.measure(
            "no feedback: the popup repaints (10 Hz hover, 1 Hz clock), overlay presents",
        )?;
        self.send(popup, "hover off")?;
        self.send(popup, "clock off")?;

        let behind = self.spawn(&["animate", &r(on(BEHIND))])?;
        self.measure("a 60 fps video behind the popup (every frame changes the glass)")?;
        self.kill(behind);
        let behind = self.spawn(&["animate", &r(on(BEHIND)), "30"])?;
        self.measure("a 30 fps video behind the popup")?;
        self.kill(behind);
        self.idle(Duration::from_millis(500));

        self.lag(popup)?;
        self.clicks(popup)?;

        self.send(popup, "second")?;
        let passes = self.overlays.stats().passes;
        self.pump_until(Instant::now() + Duration::from_secs(3), |h| {
            h.overlays.stats().passes > passes
        });
        self.note(format!(
            "open-to-glass with the session already running (the second popup): {:?}",
            self.overlays.stats().open_to_glass
        ));
        self.idle(Duration::from_millis(1500));
        let (open, sessions) = self.overlays.counts();
        let two = private_mb();
        self.note(format!(
            "two popups on one monitor: {open} overlays on {sessions} capture session(s); private {two:.1} MB (+{:.1} over none)",
            two - base
        ));
        self.shot("two-popups", grow(on(BACKDROP), 20))?;

        self.send(popup, "material 0.35 8")?;
        self.idle(Duration::from_millis(800));
        self.shot("tinted-blurred", grow(on(BACKDROP), 20))?;

        self.send(popup, "close")?;
        self.pump_until(Instant::now() + Duration::from_secs(2), |h| {
            h.overlays.counts() == (0, 0)
        });
        self.idle(Duration::from_secs(3));
        let closed = private_mb();
        self.note(format!(
            "after closing both: {:?} open; private {closed:.1} MB (+{:.1} over none)",
            self.overlays.counts(),
            closed - base
        ));
        self.send(popup, "quit")?;
        self.kill(cover);
        self.kill(still);
        Ok(())
    }

    /// A widget over a web wallpaper (the desktop-layer variant, D-W).
    fn desktop(&mut self) -> Result<()> {
        self.idle(Duration::from_millis(500));
        let base = private_mb();
        let widget = self.spawn(&["desktop", &r(on(WIDGET)), &RADIUS.to_string()])?;
        self.idle(Duration::from_millis(300));
        let opened = Instant::now();
        self.send(widget, "open")?;
        let shown = self.pump_until(Instant::now() + Duration::from_secs(5), |h| {
            h.overlays.stats().passes > 0
        });
        if !shown {
            bail!("no glass within 5 s");
        }
        self.note(format!(
            "open-to-glass: {:.1} ms",
            opened.elapsed().as_secs_f64() * 1000.0
        ));
        self.idle(Duration::from_millis(600));
        let overlay = self.order(widget);
        self.note(overlay);
        self.check_exclusion(widget, on(WIDGET))?;
        self.shot("widget-over-web", grow(on(WIDGET), 60))?;
        self.measure("a widget over the animated web wallpaper (glass stays live)")?;
        self.note(format!(
            "private with the widget's overlay: +{:.1} MB",
            private_mb() - base
        ));
        self.send(widget, "close")?;
        self.idle(Duration::from_secs(1));
        self.send(widget, "quit")?;
        Ok(())
    }

    /// The top-level windows from the widget down to the desktop (Progman, which holds the
    /// wallpaper and icons): the overlay must be right under the widget.
    fn order(&self, client: usize) -> String {
        use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDNEXT, GetWindow};
        let widget = self.children[client].hwnd;
        let mut names = Vec::new();
        let mut at = Ok(widget);
        while let Ok(window) = at {
            let class = class_of(window);
            let done = class == "Progman";
            names.push(class);
            if done || names.len() > 8 {
                break;
            }
            // SAFETY: a read-only z-order walk.
            at = unsafe { GetWindow(window, GW_HWNDNEXT) };
        }
        format!(
            "desktop-layer z-order, from the widget down: {}",
            names.join(" > ")
        )
    }

    /// The glass's backdrop must not show the client (its magenta marker), while a GDI
    /// screenshot and an independent capture session (Snipping Tool's API) must.
    fn check_exclusion(&mut self, client: usize, at: Rect) -> Result<()> {
        let hwnd = self.children[client].hwnd;
        let (frame, crop) = self
            .overlays
            .backdrop(hwnd)
            .context("no backdrop for the client")?;
        let in_crop = magenta(&crop);
        let lit = crop
            .chunks(4)
            .filter(|p| p[0] > 16 || p[1] > 16 || p[2] > 16)
            .count();
        let screen = magenta(&gdi_capture(at));
        let other = other_session_magenta(at)?;
        self.note(format!(
            "exclusion: magenta marker pixels in the glass's own backdrop {in_crop} (frame {frame:?}, {lit} lit pixels), in a GDI screenshot {screen}, in an independent capture session {other}"
        ));
        Ok(())
    }

    /// Moves the popup in circles for 5 s and pairs each move with the engine's tick that
    /// placed the overlay: the lag in DWM vblanks, and the time to the next glass present.
    fn lag(&mut self, popup: usize) -> Result<()> {
        self.children[popup].lines.lock().unwrap().clear();
        self.ticks.clear();
        self.record = true;
        self.send(popup, "path 5000")?;
        let lines = self.children[popup].lines.clone();
        self.pump_until(Instant::now() + Duration::from_secs(8), |_| {
            lines.lock().unwrap().iter().any(|(_, l)| l == "path done")
        });
        self.idle(Duration::from_millis(300));
        self.record = false;
        // (before sending Place, once the engine took it, once the popup moved), QPC.
        let moves: Vec<[i64; 3]> = lines
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, l)| {
                let mut w = l.strip_prefix("moved ")?.split_whitespace().skip(1);
                let mut next = || w.next()?.parse().ok();
                Some([next()?, next()?, next()?])
            })
            .collect();
        let freq = qpc_frequency();
        let ms = |ticks: i64| ticks as f64 * 1000.0 / freq;
        let period = refresh_period_ms();
        let at_once = self.overlays.stats().moved_at_once;
        // The overlay window moves inside the client's SendMessage: the popup follows `lead` later.
        let mut lead: Vec<f64> = moves.iter().map(|m| ms(m[2] - m[1])).collect();
        let mut round_trip: Vec<f64> = moves.iter().map(|m| ms(m[1] - m[0])).collect();
        let apart = lead.iter().filter(|&&l| l > period).count();
        // The glass inside: the first glass pass after the move was sent.
        let mut glass: Vec<f64> = moves
            .iter()
            .filter_map(|m| {
                let before = self
                    .ticks
                    .iter()
                    .take_while(|t| t.qpc < m[0])
                    .last()?
                    .passes;
                let next = self
                    .ticks
                    .iter()
                    .find(|t| t.qpc >= m[0] && t.passes > before)?;
                Some(ms(next.qpc - m[0]))
            })
            .collect();
        self.note(format!(
            "lag over {} moves, refresh {period:.2} ms: {at_once} moved the overlay window inside the client's SendMessage (round trip {}), the popup followed {} later ({apart} moves more than a refresh apart); the glass inside was redrawn at the new place {} after the move",
            moves.len(),
            stats_ms(&mut round_trip),
            stats_ms(&mut lead),
            stats_ms(&mut glass),
        ));
        Ok(())
    }

    /// Clicks: the popup's glass body (alpha 1/255) must be hit, the overlay's rim outside it
    /// must not be.
    fn clicks(&mut self, popup: usize) -> Result<()> {
        let hwnd = self.children[popup].hwnd;
        let popup_at = on(POPUP);
        let body = POINT {
            x: popup_at.x + popup_at.width / 2,
            y: popup_at.y + popup_at.height - 40,
        };
        let ring = POINT {
            x: popup_at.x - 4,
            y: popup_at.y + popup_at.height / 2,
        };
        // SAFETY: read-only hit tests.
        let (at_body, at_ring) = unsafe {
            (
                GetAncestor(WindowFromPoint(body), GA_ROOT),
                GetAncestor(WindowFromPoint(ring), GA_ROOT),
            )
        };
        let mut was = POINT::default();
        // SAFETY: a real click on our own popup's body, then the cursor goes back.
        unsafe {
            let _ = GetCursorPos(&mut was);
            let _ = SetCursorPos(body.x, body.y);
            let click = |flags| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            SendInput(
                &[click(MOUSEEVENTF_LEFTDOWN), click(MOUSEEVENTF_LEFTUP)],
                size_of::<INPUT>() as i32,
            );
        }
        let lines = self.children[popup].lines.clone();
        let clicked = self.pump_until(Instant::now() + Duration::from_secs(2), |_| {
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|(_, l)| l.starts_with("click"))
        });
        // SAFETY: puts the cursor back.
        let _ = unsafe { SetCursorPos(was.x, was.y) };
        let behind = class_of(at_ring);
        self.note(format!(
            "clicks: the glass body hits the popup: {} (WindowFromPoint), received: {clicked}; the overlay's rim outside the popup hits {behind:?} (click-through: {})",
            at_body == hwnd,
            at_ring != hwnd && behind != "mypaper.wallpaper.glass"
        ));
        Ok(())
    }

    /// Measures one phase of `self.phase`: CPU of the whole process, the overlays' counts.
    fn measure(&mut self, name: &str) -> Result<()> {
        self.idle(Duration::from_millis(500));
        let (cpu0, stats0, t0) = (cpu_ms(), self.overlays.stats(), Instant::now());
        self.idle(self.phase);
        let (cpu1, stats1) = (cpu_ms(), self.overlays.stats());
        let seconds = t0.elapsed().as_secs_f64();
        let stats = Stats {
            frames: stats1.frames.minus(stats0.frames),
            updates: stats1.updates - stats0.updates,
            compares: stats1.compares - stats0.compares,
            unchanged: stats1.unchanged - stats0.unchanged,
            passes: stats1.passes - stats0.passes,
            gpu_ms: stats1.gpu_ms - stats0.gpu_ms,
            timed: stats1.timed - stats0.timed,
            ..stats1
        };
        self.rows.push(Row {
            name: name.into(),
            seconds,
            cpu: (cpu1 - cpu0) / (seconds * 10.0),
            stats,
            private_mb: private_mb(),
        });
        Ok(())
    }

    fn report(&self) {
        println!();
        for note in &self.notes {
            println!("- {note}");
        }
        println!();
        println!(
            "| phase | s | CPU % of a core | frames delivered / stale / missed / taken | updates | compares (unchanged) | glass passes | GPU ms per update | private MB |"
        );
        println!("|---|---:|---:|---|---:|---:|---:|---:|---:|");
        for row in &self.rows {
            let s = &row.stats;
            let gpu = if s.timed > 0 {
                format!("{:.3}", s.gpu_ms / s.timed as f64)
            } else {
                "-".into()
            };
            println!(
                "| {} | {:.0} | {:.2} | {} / {} / {} / {} | {} | {} ({}) | {} | {gpu} | {:.1} |",
                row.name,
                row.seconds,
                row.cpu,
                s.frames.delivered,
                s.frames.stale,
                s.frames.missed,
                s.frames.taken,
                s.updates,
                s.compares,
                s.unchanged,
                s.passes,
                row.private_mb
            );
        }
        println!(
            "\nexclusion by window affinity (fallback): {}",
            self.overlays.stats().excluding_by_affinity
        );
    }

    fn note(&mut self, note: String) {
        println!("{note}");
        self.notes.push(note);
    }

    fn spawn(&mut self, args: &[&str]) -> Result<usize> {
        let exe = std::env::current_exe()?.with_file_name("overlay_spike_popup.exe");
        let mut process = Command::new(exe)
            .args(args.iter().flat_map(|a| a.split(' ')))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("starting overlay_spike_popup")?;
        let stdin = process.stdin.take().context("no stdin")?;
        let stdout = process.stdout.take().context("no stdout")?;
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        let thread = self.thread;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.starts_with("error") {
                    eprintln!("client: {line}");
                }
                sink.lock().unwrap().push((qpc(), line));
                // SAFETY: wakes the harness's thread.
                let _ = unsafe { PostThreadMessageW(thread, WM_LINE, WPARAM(0), LPARAM(0)) };
            }
        });
        self.children.push(Client {
            process,
            stdin,
            lines: lines.clone(),
            hwnd: HWND::default(),
        });
        let index = self.children.len() - 1;
        let ready = self.pump_until(Instant::now() + Duration::from_secs(5), |_| {
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|(_, l)| l.starts_with("ready"))
        });
        if !ready {
            bail!("client {args:?} not ready");
        }
        let hwnd = lines
            .lock()
            .unwrap()
            .iter()
            .find_map(|(_, l)| l.strip_prefix("ready ")?.trim().parse::<usize>().ok())
            .context("no hwnd")?;
        self.children[index].hwnd = HWND(hwnd as *mut _);
        Ok(index)
    }

    fn send(&mut self, client: usize, line: &str) -> Result<()> {
        writeln!(self.children[client].stdin, "{line}")?;
        self.children[client].stdin.flush()?;
        Ok(())
    }

    fn kill(&mut self, client: usize) {
        let _ = self.children[client].process.kill();
        let _ = self.children[client].process.wait();
    }

    fn idle(&mut self, for_: Duration) {
        self.pump_until(Instant::now() + for_, |_| false);
    }

    /// Runs the engine's loop (wait for a message, pump, tick) until `done` or `deadline`.
    fn pump_until(&mut self, deadline: Instant, mut done: impl FnMut(&Self) -> bool) -> bool {
        loop {
            if done(self) {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            // SAFETY: the engine's wait: until a message arrives or the deadline passes.
            unsafe {
                MsgWaitForMultipleObjectsEx(
                    None,
                    left.as_millis().min(u128::from(u32::MAX)) as u32,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            let before = self.overlays.stats().passes;
            let handled = self.overlays.tick();
            if self.record {
                let passes = self.overlays.stats().passes;
                if handled > 0 || passes != before {
                    self.ticks.push(Tick { qpc: qpc(), passes });
                }
            }
        }
    }

    /// Saves a GDI screenshot of `at` as a BMP.
    fn shot(&mut self, name: &str, at: Rect) -> Result<()> {
        let pixels = gdi_capture(at);
        let path = self.out.join(format!("{name}.bmp"));
        std::fs::write(&path, bmp(at.width as u32, at.height as u32, &pixels))?;
        self.note(format!("screenshot: {}", path.display()));
        Ok(())
    }
}

fn r(rect: Rect) -> String {
    format!("{} {} {} {}", rect.x, rect.y, rect.width, rect.height)
}

fn grow(rect: Rect, by: i32) -> Rect {
    Rect {
        x: rect.x - by,
        y: rect.y - by,
        width: rect.width + 2 * by,
        height: rect.height + 2 * by,
    }
}

fn magenta(bgra: &[u8]) -> usize {
    bgra.chunks(4)
        .filter(|p| p[0] > 230 && p[1] < 30 && p[2] > 230)
        .count()
}

/// The monitor the spike runs on: the primary, or with `left` as the last argument the one left of
/// it (the user may be working on the primary).
fn monitor_bounds() -> Rect {
    static BOUNDS: std::sync::OnceLock<Rect> = std::sync::OnceLock::new();
    *BOUNDS.get_or_init(|| {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
        };
        let left = std::env::args().next_back().is_some_and(|a| a == "left");
        let point = if left {
            POINT { x: -10, y: 300 }
        } else {
            POINT { x: 10, y: 300 }
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: plain monitor queries into a local.
        unsafe {
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let _ = GetMonitorInfoW(monitor, &mut info);
        }
        let m = info.rcMonitor;
        rect(m.left, m.top, m.right - m.left, m.bottom - m.top)
    })
}

/// `layout` (laid out on a 2560x1440 monitor at 0, 0) on the spike's monitor.
fn on(layout: Rect) -> Rect {
    let m = monitor_bounds();
    Rect {
        x: m.x + layout.x,
        y: m.y + layout.y,
        ..layout
    }
}

/// Pixels along the monitor's edges (4 px deep) that look like the capture border's yellow.
fn yellow_edges(m: Rect) -> usize {
    let edges = [
        rect(m.x, m.y, m.width, 4),
        rect(m.x, m.y + m.height - 4, m.width, 4),
        rect(m.x, m.y, 4, m.height),
        rect(m.x + m.width - 4, m.y, 4, m.height),
    ];
    edges
        .iter()
        .map(|&e| {
            gdi_capture(e)
                .chunks(4)
                .filter(|p| p[2] > 200 && p[1] > 140 && p[0] < 90)
                .count()
        })
        .sum()
}

/// A GDI screenshot of the screen rect `at` (tightly packed BGRA), layered windows included.
fn gdi_capture(at: Rect) -> Vec<u8> {
    let (w, h) = (at.width, at.height);
    // SAFETY: a DIB section we own, filled by one BitBlt from the screen.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return Vec::new();
        };
        let old = SelectObject(dc, bmp.into());
        let _ = BitBlt(
            dc,
            0,
            0,
            w,
            h,
            Some(screen),
            at.x,
            at.y,
            SRCCOPY | CAPTUREBLT,
        );
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), (w * h * 4) as usize).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        pixels
    }
}

/// Magenta pixels in `at` as an independent capture session sees it (no exclusion list): the
/// API Snipping Tool and screen sharing use.
fn other_session_magenta(at: Rect) -> Result<usize> {
    use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows_core::Interface;

    let mut device = None;
    let mut context = None;
    // SAFETY: plain device creation; out pointers outlive the call.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device: ID3D11Device = device.context("no device")?;
    let context: ID3D11DeviceContext = context.context("no context")?;
    // SAFETY: a live DXGI device.
    let winrt: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&device.cast::<IDXGIDevice>()?)? }.cast()?;
    let m = monitor_bounds();
    let at = Rect {
        x: at.x - m.x,
        y: at.y - m.y,
        ..at
    };
    // SAFETY: a plain monitor lookup.
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: m.x + 10,
                y: m.y + 10,
            },
            MONITOR_DEFAULTTOPRIMARY,
        )
    };
    let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: a live monitor handle.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        item.Size()?,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsBorderRequired(false);
    session.StartCapture()?;
    let start = Instant::now();
    let frame = loop {
        if let Ok(frame) = pool.TryGetNextFrame() {
            break frame;
        }
        if start.elapsed() > Duration::from_secs(3) {
            bail!("the independent session got no frame");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    // SAFETY: the frame's texture, a staging copy of `at`, mapped and read on this thread.
    let count = unsafe {
        let texture: ID3D11Texture2D = access.GetInterface()?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: at.width as u32,
            Height: at.height as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging))?;
        let staging = staging.context("no staging texture")?;
        let source = D3D11_BOX {
            left: at.x as u32,
            top: at.y as u32,
            front: 0,
            right: (at.x + at.width) as u32,
            bottom: (at.y + at.height) as u32,
            back: 1,
        };
        context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, 0, Some(&source));
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let mut count = 0;
        for y in 0..at.height as usize {
            let row = std::slice::from_raw_parts(
                mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                at.width as usize * 4,
            );
            count += magenta(row);
        }
        context.Unmap(&staging, 0);
        count
    };
    drop(frame);
    session.Close()?;
    pool.Close()?;
    Ok(count)
}

/// A top-down 32-bit BMP of tightly packed BGRA.
fn bmp(width: u32, height: u32, bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(54 + bgra.len());
    out.extend(b"BM");
    out.extend((54 + bgra.len() as u32).to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend(width.to_le_bytes());
    out.extend((-(height as i32)).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    out.extend(bgra);
    out
}

/// "median X ms, p95 Y ms, max Z ms".
fn stats_ms(values: &mut [f64]) -> String {
    if values.is_empty() {
        return "n/a".into();
    }
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    format!(
        "median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        at(0.5),
        at(0.95),
        values[values.len() - 1]
    )
}

fn qpc() -> i64 {
    let mut value = 0;
    // SAFETY: a plain query.
    let _ = unsafe { QueryPerformanceCounter(&mut value) };
    value
}

fn qpc_frequency() -> f64 {
    let mut value = 0;
    // SAFETY: a plain query.
    let _ = unsafe { QueryPerformanceFrequency(&mut value) };
    value as f64
}

fn refresh_period_ms() -> f64 {
    let mut info = DWM_TIMING_INFO {
        cbSize: size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: a plain query into a local.
    let _ = unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut info) };
    info.qpcRefreshPeriod as f64 * 1000.0 / qpc_frequency()
}

fn private_mb() -> f64 {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: our own process; the struct's size goes with it.
    let _ = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    counters.PrivateUsage as f64 / (1024.0 * 1024.0)
}

/// CPU time (user + kernel) of the whole process so far, in ms.
fn cpu_ms() -> f64 {
    let (mut a, mut b, mut kernel, mut user) = Default::default();
    // SAFETY: our own process; out pointers outlive the call.
    let _ = unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user) };
    let ticks = |t: windows::Win32::Foundation::FILETIME| {
        (u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)) as f64
    };
    (ticks(kernel) + ticks(user)) / 10_000.0
}

/// A window's class name.
fn class_of(hwnd: HWND) -> String {
    let mut class = [0u16; 64];
    // SAFETY: a read-only query into a local buffer.
    let len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut class) };
    String::from_utf16_lossy(&class[..len.max(0) as usize])
}
