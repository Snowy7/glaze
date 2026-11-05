#![allow(dead_code)]
#[path = "../.source/caps.rs"]
mod caps;
mod runtime { include!(concat!(env!("OUT_DIR"), "/runtime.rs")); }

use std::{path::PathBuf, time::Instant};
use anyhow::{Result, ensure};
use runtime::{Effect, Manifest, Runtime, exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen}};
use runtime::mypaper::plugin::types::SizeClass;
use windows::Win32::{Foundation::FILETIME, System::{ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX}, Threading::{GetCurrentProcess, GetCurrentThread, GetThreadTimes}}};

fn process_memory() -> (usize, usize) {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    // SAFETY: correctly sized output structure and the current process pseudo handle.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb).unwrap(); }
    (counters.WorkingSetSize, counters.PrivateUsage)
}
fn private_bytes() -> usize { process_memory().1 }

fn thread_cpu_ns() -> u64 {
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: all output pointers are valid FILETIME structures; handle is this thread.
    unsafe { GetThreadTimes(GetCurrentThread(), &mut created, &mut exited, &mut kernel, &mut user).unwrap(); }
    let ticks = |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    (ticks(kernel) + ticks(user)) * 100
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--warm-memory") {
        let dir = PathBuf::from(&args[3]);
        let manifest = Manifest::load(&dir)?;
        let rt = Runtime::new(PathBuf::from(&args[2]), PathBuf::from(&args[2]).join("data"))?;
        let before = process_memory();
        let (component, warm) = rt.component(&manifest, &dir)?;
        ensure!(warm, "memory measurement must use an existing AOT cache");
        let mut instance = rt.instantiate(&manifest, &component, None)?;
        instance.event(&Event::Open(FaceOpen { instance: "warm-probe".into(), face: "main".into(), size: SizeClass::Medium, settings: "{}".into() }))?;
        let after = process_memory();
        println!("fresh_warm_component_ws_delta_bytes={}", after.0.saturating_sub(before.0));
        println!("fresh_warm_component_private_delta_bytes={}", after.1.saturating_sub(before.1));
        return Ok(());
    }
    let dir = PathBuf::from(std::env::args().nth(1).expect("plugin directory"));
    let manifest = Manifest::load(&dir)?;
    let scratch = std::env::temp_dir().join(format!("mypaper-ts-probe-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let result = measure(dir, manifest, &scratch);
    // Scratch is a uniquely named directory created by this process; no user data lives here.
    let _ = std::fs::remove_dir_all(scratch);
    result
}

fn measure(dir: PathBuf, manifest: Manifest, scratch: &std::path::Path) -> Result<()> {
    let rt = Runtime::new(scratch.join("cache"), scratch.join("data"))?;
    let started = Instant::now();
    let (component, warm) = rt.component(&manifest, &dir)?;
    ensure!(!warm);
    println!("component_bytes={}", std::fs::metadata(dir.join(&manifest.component))?.len());
    let cache_bytes: u64 = std::fs::read_dir(scratch.join("cache"))?.map(|e| e.unwrap().metadata().unwrap().len()).sum();
    println!("aot_cache_bytes={cache_bytes}");
    let mut instance = rt.instantiate(&manifest, &component, None)?;
    println!("cold_load_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    println!("init_linear_bytes={}", instance.memory());
    drop(instance);
    drop(component);
    let started = Instant::now();
    let (component, warm) = rt.component(&manifest, &dir)?;
    ensure!(warm);
    instance = rt.instantiate(&manifest, &component, None)?;
    println!("warm_load_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    let open = Event::Open(FaceOpen { instance: "probe-1".into(), face: "main".into(), size: SizeClass::Medium, settings: "{}".into() });
    let effects = instance.event(&open)?;
    ensure!(effects.iter().any(|e| matches!(e, Effect::View(..))), "open must draw a real host-decoded view");
    println!("open_linear_bytes={}", instance.memory());
    let before = private_bytes();
    let mut second = rt.instantiate(&manifest, &component, None)?;
    second.event(&open)?;
    println!("second_instance_private_delta_bytes={}", private_bytes().saturating_sub(before));
    println!("second_instance_linear_bytes={}", second.memory());
    drop(second);
    let action = Event::Action(Action { instance: "probe-1".into(), action: "click".into(), value: None });
    let cpu_before = instance.cpu;
    let mut samples = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let started = Instant::now();
        let effects = instance.event(&action)?;
        ensure!(effects.iter().any(|e| matches!(e, Effect::View(..))), "each click must redraw");
        samples.push(started.elapsed().as_secs_f64() * 1e6);
    }
    samples.sort_by(f64::total_cmp);
    println!("event_p50_us={:.3}", samples[500]);
    println!("event_p99_us={:.3}", samples[990]);
    println!("event_mean_runtime_us={:.3}", (instance.cpu - cpu_before).as_secs_f64() * 1e6 / 1000.0);
    println!("after_1000_events_linear_bytes={}", instance.memory());
    let before = thread_cpu_ns();
    for _ in 0..10_000 { drop(instance.event(&action)?); }
    println!("event_mean_thread_cpu_us={:.3}", (thread_cpu_ns() - before) as f64 / 1000.0 / 10_000.0);
    println!("after_11000_events_linear_bytes={}", instance.memory());
    let saved = instance.save()?;
    ensure!(!saved.is_empty(), "hello saves its counter");
    drop(instance);
    let mut restored = rt.instantiate(&manifest, &component, Some(saved.clone()))?;
    ensure!(restored.save()? == saved, "state must survive unload/reload");
    let started = Instant::now();
    let spin = restored.event(&Event::Action(Action { instance: "probe-1".into(), action: "spin".into(), value: None }));
    ensure!(spin.is_err(), "infinite loop must trap");
    println!("spin_trap_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    // A separate process avoids attributing retained install-time compiler allocations to a
    // warm active plugin. This includes the unique component mapping, rather than just a
    // second instance of an already mapped component.
    let output = std::process::Command::new(std::env::current_exe()?)
        .arg("--warm-memory").arg(scratch.join("cache")).arg(&dir).output()?;
    ensure!(output.status.success(), "warm memory probe: {}", String::from_utf8_lossy(&output.stderr));
    print!("{}", String::from_utf8(output.stdout)?);
    Ok(())
}
