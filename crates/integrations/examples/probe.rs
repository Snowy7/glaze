//! Headless qualification harness. It never changes app settings, requests consent, or invokes controls.
use mypaper_integrations::{Availability, Hub, Provider, config::Config};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::{GetCurrentProcess, GetProcessTimes},
    },
};

fn usage() -> anyhow::Result<(f64, f64)> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
    memory.cb = std::mem::size_of_val(&memory) as u32;
    // SAFETY: current-process pseudo-handle and correctly sized, owned output structures.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )?;
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            memory.cb,
        )
        .ok()?;
    }
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Ok((
        (ticks(kernel) + ticks(user)) as f64 / 10_000_000.0,
        memory.PrivateUsage as f64 / 1_048_576.0,
    ))
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let provider = match args.first().map(String::as_str) {
        Some("notifications") => Provider::Notifications,
        Some("media") => Provider::Media,
        Some("tailscale") => Provider::Tailscale,
        Some("t3") => Provider::T3,
        _ => anyhow::bail!("expected notifications, media, tailscale or t3"),
    };
    let live = args.iter().any(|s| s == "--live");
    let seconds = args
        .iter()
        .find_map(|s| s.strip_prefix("--seconds="))
        .unwrap_or("60")
        .parse::<u64>()?;
    let runs = args
        .iter()
        .find_map(|s| s.strip_prefix("--runs="))
        .unwrap_or("3")
        .parse::<usize>()?;
    let (tx, rx) = mpsc::channel();
    let hub = Hub::new(move |_| {
        let _ = tx.send(());
    });
    let mut config = Config::default();
    config.notifications.enabled = true;
    config.notifications.live_verified = live;
    config.tailscale.enabled = true;
    config.tailscale.live_verified = live;
    config.t3.enabled = true;
    config.t3.endpoint = "ws://127.0.0.1:3773/ws".into();
    config.t3.live_verified = live;
    for run in 0..runs {
        let (cpu, base) = usage()?;
        let began = Instant::now();
        std::thread::sleep(Duration::from_secs(seconds));
        let (after, _) = usage()?;
        println!(
            "{}",
            serde_json::json!({"provider":format!("{provider:?}"),"run":run+1,"stage":"disabled","cpu_core_pct":(after-cpu)/began.elapsed().as_secs_f64()*100.0,"private_mib":base})
        );
        let lease = hub.acquire(provider, &config)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while lease.snapshot().availability == Availability::Connecting && Instant::now() < deadline
        {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        if args.iter().any(|arg| arg == "--retry") {
            let before = lease.activity().attempts;
            lease.retry();
            let deadline = Instant::now() + Duration::from_secs(15);
            while (lease.activity().attempts == before
                || lease.snapshot().availability == Availability::Connecting)
                && Instant::now() < deadline
            {
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
            anyhow::ensure!(
                lease.activity().attempts > before,
                "explicit retry did not restart the provider"
            );
            println!(
                "{}",
                serde_json::json!({"provider":format!("{provider:?}"),"stage":"reconnect","attempts":lease.activity().attempts-before,"availability":format!("{:?}",lease.snapshot().availability)})
            );
        }
        let status = format!("{:?}", lease.snapshot().availability);
        if matches!(
            lease.snapshot().availability,
            Availability::Connecting | Availability::Unavailable(_)
        ) {
            println!(
                "{}",
                serde_json::json!({"provider":format!("{provider:?}"),"stage":"activation","availability":status})
            );
            drop(lease);
            break;
        }
        while rx.try_recv().is_ok() {}
        // A finite activation grace period excludes initial State/Prefs/Status deliveries from idle.
        std::thread::sleep(Duration::from_secs(2));
        while rx.try_recv().is_ok() {}
        let (cpu, private) = usage()?;
        let activity = lease.activity();
        let began = Instant::now();
        std::thread::sleep(Duration::from_secs(seconds));
        let (after, end_private) = usage()?;
        println!(
            "{}",
            serde_json::json!({"provider":format!("{provider:?}"),"run":run+1,"stage":"enabled","availability":status,"cpu_core_pct":(after-cpu)/began.elapsed().as_secs_f64()*100.0,"private_mib":end_private,"added_private_mib":private-base,"deliveries":lease.activity().deliveries-activity.deliveries,"attempts":lease.activity().attempts-activity.attempts,"snapshot_changes":rx.try_iter().count()})
        );
        drop(lease);
        println!(
            "{}",
            serde_json::json!({"stage":"stopped","private_mib":usage()?.1})
        );
    }
    Ok(())
}
