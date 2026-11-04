//! N0.1, the shell surface spike (docs/ui.md, "Measured"): a 3840x36 floating bar drawn with
//! software Direct2D into a layered window, measured from inside the process.
//!
//! ```text
//! cargo run --release -p mypaper-ui --example mypaper-bar-spike -- [--seconds 60] [--redraws 200] [--x X --y Y]
//! ```
//!
//! It reports private memory at each step (WARP's share is the step that creates the first
//! render target), the time from process creation to the first paint, full and partial redraw
//! times, the bar sliding in through position and alpha only, CPU over `--seconds` of idle (the
//! loop sleeps in `MsgWaitForMultipleObjectsEx`), and every DLL loaded (no GPU driver may be).
//! It shows a topmost bar across the top of the primary monitor: run it under the desktop lock.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use mypaper_ui::{
    DisplayList, Kind, Op, Rect, Rgba, Role, Spring, Surface, TextStyle, Ticker, Tween, Ui, icons,
    modules,
};
use windows::Win32::Foundation::{FILETIME, WAIT_OBJECT_0};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage,
};

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 36;

fn main() -> Result<()> {
    let mut seconds = 60u64;
    let mut redraws = 200usize;
    let (mut x, mut y) = (0i32, 0i32);
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--seconds" => seconds = args.next().unwrap_or_default().parse()?,
            "--redraws" => redraws = args.next().unwrap_or_default().parse()?,
            "--x" => x = args.next().unwrap_or_default().parse()?,
            "--y" => y = args.next().unwrap_or_default().parse()?,
            other => bail!("unknown flag {other}"),
        }
    }
    let mut memory = vec![("start", private_mb())];
    let mut ui = Ui::new()?;
    memory.push(("factories (D2D, DirectWrite, WIC)", private_mb()));
    let mut surface = Surface::new(Kind::Bar, "mypaper bar spike", Role::Toolbar)?;
    memory.push(("window", private_mb()));

    let icons_family = icons::system_family(&ui.text);
    let mut clock = 0u32;
    let list = bar(&mut ui, &icons_family, clock, 0.0);
    let first = Instant::now();
    surface.present(&mut ui, (x, y - HEIGHT as i32), (WIDTH, HEIGHT), list)?;
    surface.set_alpha(0)?;
    surface.show();
    let first_paint = first.elapsed();
    let first_paint_age = process_age();
    memory.push((
        "first paint (render target, WIC + DIB bitmaps)",
        private_mb(),
    ));

    // The bar slides down and fades in: position and alpha only, one tick per display frame.
    let mut ticker = Ticker::new()?;
    let start = Instant::now();
    let slide = Tween::new(-(HEIGHT as f32), 0.0, Spring::DEFAULT, start);
    let fade = Tween::new(0.0, 255.0, Spring::DEFAULT, start);
    let mut frames = 0;
    while !slide.done(Instant::now()) {
        ticker.schedule(true);
        wait(&[ticker.handle()], INFINITE);
        let now = Instant::now();
        surface.move_to((x, y + slide.value(now).round() as i32))?;
        surface.set_alpha(fade.value(now).clamp(0.0, 255.0) as u8)?;
        frames += 1;
    }
    ticker.schedule(false);
    let slid = start.elapsed();

    let time = |ui: &mut Ui, surface: &mut Surface, change: &mut dyn FnMut(usize) -> (u32, f32)| {
        let mut samples: Vec<Duration> = (0..redraws)
            .map(|i| {
                let (clock, shade) = change(i);
                let list = bar(ui, &icons_family, clock, shade);
                let t = Instant::now();
                let drawn = surface.present(ui, (x, y), (WIDTH, HEIGHT), list);
                assert!(drawn.is_ok_and(|d| d), "every sample redraws");
                t.elapsed()
            })
            .collect();
        samples.sort();
        (
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100],
            samples[samples.len() - 1],
        )
    };
    // A partial redraw: only the clock changes.
    let partial = time(&mut ui, &mut surface, &mut |i| (i as u32 + 1, 0.0));
    clock = redraws as u32;
    // A full redraw: the whole bar's tint changes.
    let full = time(&mut ui, &mut surface, &mut |i| {
        (clock, (i % 2) as f32 * 0.02 + 0.01)
    });
    // An identical frame: nothing to draw.
    let same = bar(
        &mut ui,
        &icons_family,
        clock,
        0.01 + ((redraws - 1) % 2) as f32 * 0.02,
    );
    let t = Instant::now();
    let redrew = surface.present(&mut ui, (x, y), (WIDTH, HEIGHT), same)?;
    let identical = t.elapsed();
    memory.push(("after redraws", private_mb()));

    // Idle: the loop sleeps until a message comes; nothing moves, so the ticker stays off.
    let cpu_before = cpu_time();
    let idle_start = Instant::now();
    let deadline = idle_start + Duration::from_secs(seconds);
    let mut wakeups = 0;
    while Instant::now() < deadline {
        let left = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        wait(&[ticker.handle()], left);
        wakeups += 1;
    }
    let cpu = cpu_time() - cpu_before;
    let idle = idle_start.elapsed();
    memory.push(("after idle", private_mb()));

    let loaded = modules::loaded();
    let drivers: Vec<&String> = loaded
        .iter()
        .filter(|m| modules::is_gpu_driver(m))
        .collect();
    let has = |name: &str| loaded.iter().any(|m| m == name);

    println!("== N0.1 bar spike: {WIDTH}x{HEIGHT}, software Direct2D on WIC ==");
    println!(
        "first paint: {first_paint:.2?} to draw and show; {first_paint_age:.1?} after process creation"
    );
    println!("slide-in: {frames} frames in {slid:.0?} (position and alpha only)");
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "redraw, clock only: median {:.3} ms, p95 {:.3} ms, max {:.3} ms ({redraws} samples)",
        ms(partial.0),
        ms(partial.1),
        ms(partial.2)
    );
    println!(
        "redraw, whole bar:  median {:.3} ms, p95 {:.3} ms, max {:.3} ms ({redraws} samples)",
        ms(full.0),
        ms(full.1),
        ms(full.2)
    );
    println!("identical frame: redrawn {redrew}, {:.3} ms", ms(identical));
    println!(
        "idle {idle:.1?}: CPU {cpu:.1?} = {:.4}% of one core, {wakeups} wakeups, ticker armed: {}",
        cpu.as_secs_f64() / idle.as_secs_f64() * 100.0,
        ticker.armed()
    );
    for (step, mb) in &memory {
        println!("private {mb:6.2} MB  {step}");
    }
    println!(
        "WARP: d3d10warp.dll {}, d3d11.dll {}, dxgi.dll {}",
        loaded_word(has("d3d10warp.dll")),
        loaded_word(has("d3d11.dll")),
        loaded_word(has("dxgi.dll"))
    );
    println!("GPU drivers loaded: {drivers:?}");
    println!("{} modules: {}", loaded.len(), loaded.join(" "));
    if !drivers.is_empty() {
        bail!("a GPU driver loaded into the spike");
    }
    Ok(())
}

fn loaded_word(yes: bool) -> &'static str {
    if yes { "loaded" } else { "not loaded" }
}

/// A representative top bar: the hit fill, a tinted pill per side, app labels, tray icons and
/// a clock. `shade` nudges the tint (a whole-bar change); `clock` changes the clock only.
fn bar(ui: &mut Ui, icon_family: &str, clock: u32, shade: f32) -> DisplayList {
    let white = Rgba::new(0.95, 0.95, 0.97, 1.0);
    let label = TextStyle::new("Segoe UI Variable Text", 13.0);
    let glyph = TextStyle::new(icon_family, 14.0);
    let mut ops = vec![
        Op::Fill {
            rect: Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
            radius: 0.0,
            color: Rgba::HIT,
        },
        Op::Fill {
            rect: Rect::new(8.0, 4.0, WIDTH as f32 - 16.0, 28.0),
            radius: 14.0,
            color: Rgba::new(0.08 + shade, 0.09, 0.11, 0.82),
        },
    ];
    let mut text = |ops: &mut Vec<Op>, x: f32, s: &str, style: &TextStyle| {
        let (w, h) = ui.text.measure(s, style);
        ops.push(Op::Text {
            rect: Rect::new(x, (HEIGHT as f32 - h) / 2.0, w, h),
            text: s.into(),
            style: style.clone(),
            color: white,
        });
        x + w
    };
    let mut x = 24.0;
    for app in [
        "Files", "Edit", "View", "Window", "Help", "Terminal", "Browser", "Mail",
    ] {
        x = text(&mut ops, x, app, &label) + 20.0;
    }
    let mut x = WIDTH as f32 - 420.0;
    for glyph_code in [
        '\u{E701}', '\u{E767}', '\u{E83F}', '\u{E7E7}', '\u{E713}', '\u{E8BE}',
    ] {
        x = text(&mut ops, x, &glyph_code.to_string(), &glyph) + 16.0;
    }
    let time = format!("Wed 4 Nov  {:02}:{:02}", 9 + clock / 60 % 12, clock % 60);
    text(&mut ops, WIDTH as f32 - 160.0, &time, &label);
    DisplayList {
        ops,
        ..Default::default()
    }
}

/// Sleeps until a handle is signalled, a message arrives or `ms` pass; pumps the messages.
fn wait(handles: &[windows::Win32::Foundation::HANDLE], ms: u32) -> bool {
    // SAFETY: the handles outlive the wait; a standard message pump on this thread.
    unsafe {
        let woke = MsgWaitForMultipleObjectsEx(Some(handles), ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        woke == WAIT_OBJECT_0
    }
}

fn private_mb() -> f64 {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the struct and its size match; our own pseudo process handle.
    let _ = unsafe {
        GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb)
    };
    counters.PrivateUsage as f64 / (1024.0 * 1024.0)
}

fn ticks(t: FILETIME) -> u64 {
    u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)
}

/// User plus kernel time of this process.
fn cpu_time() -> Duration {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: our own pseudo handle; every out pointer is a valid local.
    let _ = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// How long ago this process was created.
fn process_age() -> Duration {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: as above.
    let _ = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    // SAFETY: no preconditions.
    let now = ticks(unsafe { GetSystemTimePreciseAsFileTime() });
    Duration::from_nanos(now.saturating_sub(ticks(created)) * 100)
}
