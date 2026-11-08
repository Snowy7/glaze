//! Native shell fixture: bar, dock and six widgets on every physical monitor.
//! Run only under the desktop lock. `plates [--animate] [--seconds N] [--hz N]`.
use anyhow::{Context, Result};
use glaze_glass_ipc::{Material, Plate, Writer};
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    glaze_desktop::enable_dpi_awareness();
    let args: Vec<_> = std::env::args().collect();
    let value = |flag: &str, default: f64| -> Result<f64> {
        args.iter()
            .position(|a| a == flag)
            .map_or(Ok(default), |i| {
                args.get(i + 1)
                    .context("missing option value")?
                    .parse()
                    .context("invalid number")
            })
    };
    let seconds = value("--seconds", 120.0)?;
    let hz = value("--hz", 60.0)?.clamp(1.0, 360.0);
    let animate = args.iter().any(|a| a == "--animate");
    let mut plates = Vec::new();
    for (index, m) in glaze_desktop::monitors().iter().enumerate() {
        let (x, y, w, h) = (m.x as f32, m.y as f32, m.width as f32, m.height as f32);
        let mut add = |rect, radius| {
            plates.push(Plate {
                id: plates.len() as u64,
                rect,
                radius,
                material: Material::default(),
            })
        };
        add([x + 12.0, y + 12.0, w - 24.0, 40.0], 20.0);
        add([x + (w - 600.0) / 2.0, y + h - 96.0, 600.0, 76.0], 30.0);
        for n in 0..6 {
            add(
                [
                    x + 24.0 + (n % 3) as f32 * 328.0,
                    y + 88.0 + (n / 3) as f32 * 244.0,
                    304.0,
                    220.0,
                ],
                24.0,
            );
        }
        eprintln!("fixture monitor {index}: {m:?}, 8 plates");
    }
    let start = Instant::now();
    let mut writer = Writer::open()?;
    writer.publish(&plates)?;
    if !animate {
        std::thread::sleep(Duration::from_secs_f64(seconds));
        return Ok(());
    }
    let origins: Vec<_> = plates.iter().map(|p| p.rect[0]).collect();
    let interval = Duration::from_secs_f64(1.0 / hz);
    let mut deadline = start;
    // A high-resolution waitable timer avoids Sleep's 15.6 ms granularity.
    use windows::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer,
        TIMER_ALL_ACCESS, WaitForSingleObject,
    };
    // SAFETY: creates an owned timer with no callback.
    let timer = unsafe {
        CreateWaitableTimerExW(
            None,
            None,
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS.0,
        )?
    };
    let result = (|| -> Result<()> {
        while start.elapsed().as_secs_f64() < seconds {
            let t = start.elapsed().as_secs_f32();
            for (i, p) in plates.iter_mut().enumerate().filter(|(i, _)| i % 8 == 1) {
                p.rect[0] = origins[i] + 160.0 * (t * 1.5).sin();
            }
            writer.publish(&plates)?;
            deadline += interval;
            let nanos = deadline
                .saturating_duration_since(Instant::now())
                .as_nanos();
            let due = -((nanos / 100) as i64).max(1);
            // SAFETY: valid owned timer, no callbacks or borrowed data retained.
            unsafe {
                SetWaitableTimer(timer, &due, 0, None, None, false)?;
                WaitForSingleObject(timer, INFINITE);
            }
        }
        Ok(())
    })();
    // SAFETY: this example owns the timer and has finished waiting on it.
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(timer) };
    result
}
