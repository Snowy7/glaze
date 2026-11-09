//! Input-free comparison of Files' former teardown and retained-frame resume paths.
//! Launch at medium integrity under the desktop lock. Only owned no-activate windows change.
#![allow(dead_code)]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Dwm::DwmFlush,
        System::{Com::*, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
#[path = "../src/material.rs"]
mod material;
#[path = "../src/overlay/mod.rs"]
mod overlay;
mod gpu {
    pub struct DeviceSlot<T>(Option<T>);
    impl<T> From<T> for DeviceSlot<T> {
        fn from(value: T) -> Self {
            Self(Some(value))
        }
    }
    impl<T> DeviceSlot<T> {
        pub fn get(&self) -> Option<&T> {
            self.0.as_ref()
        }
        pub fn clear(&mut self) {
            self.0 = None;
        }
        pub fn replace(&mut self, value: T) {
            self.0 = Some(value);
        }
    }
    impl<T> std::ops::Deref for DeviceSlot<T> {
        type Target = T;
        fn deref(&self) -> &T {
            self.0.as_ref().expect("device unavailable")
        }
    }
}
use overlay::{
    Gpu, Overlays,
    request::{Layer, Material, Request},
};

fn pump(overlays: &mut Overlays, duration: Duration) -> u64 {
    let end = Instant::now() + duration;
    let mut wakes = 0;
    while Instant::now() < end {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
        }
        overlays.tick();
        let deadline = overlays.deadline().unwrap_or(end).min(end);
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u32::MAX as u128) as u32;
        unsafe {
            if MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
                != WAIT_TIMEOUT
            {
                wakes += 1;
            }
        }
    }
    wakes
}
fn fresh(overlays: &mut Overlays, passes: u64) -> anyhow::Result<()> {
    let end = Instant::now() + Duration::from_secs(15);
    while overlays.stats().passes == passes && Instant::now() < end {
        pump(overlays, Duration::from_millis(1));
    }
    anyhow::ensure!(
        overlays.stats().passes > passes,
        "no fresh glass in 15 seconds"
    );
    unsafe {
        DwmFlush()?;
    }
    Ok(())
}
fn gpu() -> anyhow::Result<Gpu> {
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
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))?;
    Ok(Gpu {
        adapter,
        device,
        queue,
    })
}
fn run() -> anyhow::Result<()> {
    glaze_desktop::enable_dpi_awareness();
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let output = std::env::args().nth(1).expect("output JSON path");
    let client = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            w!("STATIC"),
            w!("Glaze resume fixture"),
            WS_POPUP,
            80,
            180,
            640,
            420,
            None,
            None,
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?
    };
    unsafe {
        SetLayeredWindowAttributes(client, COLORREF(0), 1, LWA_ALPHA)?;
        let _ = ShowWindow(client, SW_SHOWNOACTIVATE);
    }
    let mut overlays = Overlays::new(gpu()?)?;
    let open = Request::Open {
        client,
        layer: Layer::Desktop,
        rect: glaze_desktop::Rect {
            x: 80,
            y: 180,
            width: 640,
            height: 420,
        },
        radius: 18.0,
        material: Material {
            clarity: 0.35,
            blur: 24.0,
            refraction: 0.6,
            highlight: 0.5,
            tint: [30, 32, 38, 0],
        },
    };
    let mut results = Vec::new();
    for load in [false, true] {
        let stop = Arc::new(AtomicBool::new(false));
        let workers: Vec<_> = (0..if load { 4 } else { 0 })
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut value = 1u64;
                    while !stop.load(Ordering::Relaxed) {
                        value = std::hint::black_box(
                            value.wrapping_mul(1664525).wrapping_add(1013904223),
                        );
                    }
                })
            })
            .collect();
        for retained in [false, true] {
            overlays.handle(open)?;
            let passes = overlays.stats().passes;
            fresh(&mut overlays, passes)?;
            pump(&mut overlays, Duration::from_millis(350));
            for trial in 0..3 {
                let plate = unsafe { GetWindow(client, GW_HWNDNEXT)? };
                unsafe {
                    let _ = ShowWindow(client, SW_HIDE);
                }
                overlays.handle(if retained {
                    Request::Visibility {
                        client,
                        visible: false,
                    }
                } else {
                    Request::Close { client }
                })?;
                pump(&mut overlays, Duration::from_millis(3400));
                let hidden_before = overlays.stats();
                let hidden_wakes = pump(&mut overlays, Duration::from_millis(400));
                let hidden_deadline_none = overlays.deadline().is_none();
                let hidden_after = overlays.stats();
                let hidden_counts = overlays.counts();
                let started = Instant::now();
                unsafe {
                    let _ = ShowWindow(client, SW_SHOWNOACTIVATE);
                }
                let passes = overlays.stats().passes;
                let cached_ms = if retained {
                    // Files shows this verified retained HWND before sending the wake.
                    unsafe {
                        let _ = ShowWindow(plate, SW_SHOWNOACTIVATE);
                        DwmFlush()?;
                    }
                    Some(started.elapsed().as_secs_f64() * 1000.0)
                } else {
                    None
                };
                overlays.handle(if retained {
                    Request::Visibility {
                        client,
                        visible: true,
                    }
                } else {
                    open
                })?;
                fresh(&mut overlays, passes)?;
                let refresh_ms = started.elapsed().as_secs_f64() * 1000.0;
                results.push(serde_json::json!({"load_cores": if load {4} else {0}, "retained": retained,
                    "trial": trial, "cached_ms": cached_ms, "fresh_ms": refresh_ms,
                    "full_opacity_ms": cached_ms.unwrap_or(refresh_ms + 300.0),
                    "hidden_plates": hidden_counts.0, "hidden_sessions": hidden_counts.1,
                    "hidden_wakes": hidden_wakes, "hidden_deadline_none": hidden_deadline_none,
                    "hidden_gpu_passes": hidden_after.passes-hidden_before.passes,
                    "hidden_capture_frames": hidden_after.frames.delivered-hidden_before.frames.delivered}));
                pump(&mut overlays, Duration::from_millis(350));
            }
            overlays.handle(Request::Close { client })?;
        }
        stop.store(true, Ordering::Relaxed);
        for worker in workers {
            let _ = worker.join();
        }
    }
    std::fs::write(output, serde_json::to_vec_pretty(&results)?)?;
    drop(overlays);
    unsafe {
        DestroyWindow(client)?;
        CoUninitialize();
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
