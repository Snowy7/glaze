//! `glaze-plugins --bench <plugin dir>...`: the budget table (docs/plugins.md, "Budgets"),
//! measured in this process with throwaway cache and data dirs.
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use glaze_plugin_api::SizeClass;
use glaze_plugin_api::wire::{self, HelperMsg, HostMsg};
use glaze_plugin_sdk::view::Face;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;

use crate::host::Host;
use crate::runtime::{Manifest, Runtime};

/// `(working set, private bytes)` in MiB.
fn memory() -> (f64, f64) {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    // SAFETY: `counters` is a correctly sized out struct for the current process.
    let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb) };
    let mib = |b: usize| b as f64 / (1 << 20) as f64;
    (mib(counters.WorkingSetSize), mib(counters.PrivateUsage))
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub fn run(dirs: &[PathBuf]) -> Result<()> {
    let scratch = std::env::temp_dir().join(format!("glaze-plugins-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let before = memory();
    let started = Instant::now();
    let rt = Runtime::new(scratch.join("cache"), scratch.join("data"))?;
    let engine = started.elapsed();
    let base = memory();
    println!("| measure | value |\n|---|---|");
    println!("| process before the engine (WS / private) | {:.1} / {:.1} MiB |", before.0, before.1);
    println!("| engine + linker + ticker | {:.1} ms, +{:.1} MiB WS / +{:.1} MiB private |", ms(engine), base.0 - before.0, base.1 - before.1);

    let (tx, _rx) = mpsc::channel();
    let mut host = Host::new(rt, Vec::<u8>::new(), tx);
    for dir in dirs {
        let manifest = Manifest::load(dir)?;
        let id = manifest.id.clone();
        let t = Instant::now();
        let (component, warm) = host.rt.component(&manifest, dir)?;
        let compiled = t.elapsed();
        let t = Instant::now();
        drop(host.rt.instantiate(&manifest, &component, None)?);
        let cold_instance = t.elapsed();
        drop(component);
        let t = Instant::now();
        let (component, warm_again) = host.rt.component(&manifest, dir)?;
        let deserialized = t.elapsed();
        let t = Instant::now();
        drop(host.rt.instantiate(&manifest, &component, None)?);
        let warm_instance = t.elapsed();
        assert!(!warm && warm_again, "first load compiles, second is cached");
        println!("| {id}: cold load (AOT compile + instantiate + init) | {:.1} ms |", ms(compiled + cold_instance));
        println!("| {id}: warm load (cached .cwasm + instantiate + init) | {:.2} ms |", ms(deserialized + warm_instance));

        let face = manifest.faces.first().map_or("main".to_owned(), |f| f.id.clone());
        let before = memory();
        let open = HostMsg::Open { plugin: id.clone(), dir: dir.to_string_lossy().into_owned(), instance: format!("{id}-1"), face, size: SizeClass::Medium, settings: "{}".into() };
        host.on_host(open, Instant::now());
        let after = memory();
        println!(
            "| {id}: active (linear memory / process private delta) | {:.2} MiB / +{:.2} MiB |",
            host.memory(&id).unwrap_or(0) as f64 / (1 << 20) as f64,
            after.1 - before.1
        );
    }

    if dirs.iter().any(|d| d.ends_with("hello")) {
        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            host.out().clear();
            let t = Instant::now();
            host.on_host(HostMsg::Action { instance: "hello-1".into(), action: "click".into(), value: None }, Instant::now());
            // What the shell does with the frame before drawing it.
            let bytes = std::mem::take(host.out());
            let mut input = bytes.as_slice();
            while let Some(message) = wire::read::<HelperMsg>(&mut input)? {
                if let HelperMsg::View { face, .. } = message {
                    let _: Face = serde_json::from_slice(&face)?;
                }
            }
            samples.push(t.elapsed());
        }
        samples.sort();
        let p = |q: f64| ms(samples[((samples.len() as f64 * q) as usize).min(samples.len() - 1)]);
        println!("| hello: action → view frame decoded (p50 / p99, 1000 clicks) | {:.3} / {:.3} ms |", p(0.5), p(0.99));
        let t = Instant::now();
        host.on_host(HostMsg::Action { instance: "hello-1".into(), action: "spin".into(), value: None }, Instant::now());
        println!("| hello: infinite loop stopped after | {:.1} ms |", ms(t.elapsed()));
    }
    let end = memory();
    println!("| process at the end (WS / private) | {:.1} / {:.1} MiB |", end.0, end.1);
    drop(host);
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

