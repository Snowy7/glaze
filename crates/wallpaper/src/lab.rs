//! Offline frame lab (dev tool, `#[ignore]`): one screen of the engine's real per-frame path
//! (wallpaper into the kept frame, restore into a 2560x1440 target, the fixture's eight glass
//! plates) on the default integrated GPU, without touching the desktop. It times every pass
//! with GPU timestamps and dumps raw frames, so a change can be measured and pixel-diffed
//! (`tools/perf/diff-frames.py`) before anyone takes the desktop lock.
//!
//! ```text
//! set MYPAPER_LAB=themes\rain-city;%TEMP%\mypaper-lab\before;3,12,30;150
//! cargo test -p mypaper-wallpaper lab::lab -- --ignored --exact --nocapture
//! ```
//!
//! Fields: theme folder (from the repo root); output folder; capture times in seconds; timed
//! frames. Each capture writes `<t>s.bgra` (the final 2560x1440 frame) and `<t>s-kept.bgra`
//! (the wallpaper at its render scale), tightly packed BGRA, with sizes in `frames.json`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use mypaper_desktop::Rect;
use mypaper_glass_ipc::{Material, Plate};

use crate::glass_pass::GlassPass;
use crate::gpu::{Gpu, KeptFrame};
use crate::sources::{self, Frame, Source};

const SIZE: (u32, u32) = (2560, 1440);
/// Timestamps the glass renderer may write per frame (its passes plus one).
const PASS_STAMPS: u32 = 24;

/// The plates fixture (examples/plates.rs) on one monitor at the origin.
fn fixture(dock_x: f32) -> Vec<Plate> {
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let mut plates = Vec::new();
    let mut add = |rect, radius| {
        plates.push(Plate {
            id: plates.len() as u64,
            rect,
            radius,
            material: Material::default(),
        })
    };
    add([12.0, 12.0, w - 24.0, 40.0], 20.0);
    add([(w - 600.0) / 2.0 + dock_x, h - 96.0, 600.0, 76.0], 30.0);
    for n in 0..6 {
        let (column, row) = ((n % 3) as f32, (n / 3) as f32);
        add(
            [24.0 + column * 328.0, 88.0 + row * 244.0, 304.0, 220.0],
            24.0,
        );
    }
    plates
}

struct Lab {
    gpu: Gpu,
    source: Box<dyn Source>,
    kept: KeptFrame,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    glass: GlassPass,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    pass_resolve: wgpu::Buffer,
    pass_read: wgpu::Buffer,
    period: f64,
    /// Wallpapers drawn so far (the kept frame's `drawn`).
    drawn: u32,
}

impl Lab {
    fn new(theme: &Path) -> Self {
        let gpu = Gpu::headless().expect("a DX12 GPU");
        let theme = mypaper_theme::LoadedTheme::load(theme).unwrap();
        let source = sources::load(&gpu, &theme, &[]).unwrap();
        let kept = KeptFrame::new(&gpu, source.render_size(SIZE));
        // `texture` adds COPY_DST, as the swapchain has, so a full-scale restore copies.
        let target = gpu.texture(
            "lab target",
            SIZE,
            gpu.format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            None,
        );
        let view = target.create_view(&Default::default());
        let monitor = Rect {
            x: 0,
            y: 0,
            width: SIZE.0 as i32,
            height: SIZE.1 as i32,
        };
        let glass = GlassPass::new(Some(&[monitor])).unwrap();
        let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("lab"),
            ty: wgpu::QueryType::Timestamp,
            count: 4,
        });
        let buffer = |size, usage| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lab timestamps"),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let resolving = wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC;
        let reading = wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ;
        let resolve = buffer(32, resolving);
        let read = buffer(32, reading);
        let pass_resolve = buffer(u64::from(PASS_STAMPS) * 8, resolving);
        let pass_read = buffer(u64::from(PASS_STAMPS) * 8, reading);
        let period = f64::from(gpu.queue.get_timestamp_period());
        Self {
            gpu,
            source,
            kept,
            target,
            view,
            glass,
            queries,
            resolve,
            read,
            pass_resolve,
            pass_read,
            period,
            drawn: 0,
        }
    }

    /// Records one frame like `Engine::render`; `wallpaper` false is a glass-only frame.
    fn encode(&mut self, clock: Duration, frame: u32, wallpaper: bool) -> wgpu::CommandEncoder {
        let gpu = &self.gpu;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.write_timestamp(&self.queries, 0);
        if wallpaper {
            self.source.update(gpu, clock);
            self.source.render(
                gpu,
                Frame {
                    screen: 0,
                    size: (self.kept.texture.width(), self.kept.texture.height()),
                    mouse: [0.0, 0.0],
                    clock,
                    frame,
                    view: &self.kept.view,
                    encoder: &mut encoder,
                },
            );
            // A new wallpaper in the kept frame, as the engine marks it.
            self.drawn += 1;
            self.kept.drawn.set(Some(self.drawn));
        }
        encoder.write_timestamp(&self.queries, 1);
        self.kept
            .restore(gpu, &self.target, &self.view, &mut encoder);
        encoder.write_timestamp(&self.queries, 2);
        self.glass
            .encode(gpu, 0, &self.kept, &self.target, &mut encoder);
        encoder.write_timestamp(&self.queries, 3);
        encoder
    }

    /// Submits one timed frame, waits for it and returns [wallpaper, restore, glass] in ms, and
    /// the glass renderer's passes (label, ms) once [`Lab::profile`] is on.
    fn timed(
        &mut self,
        clock: Duration,
        frame: u32,
        wallpaper: bool,
    ) -> ([f64; 3], Vec<(&'static str, f64)>) {
        let mut encoder = self.encode(clock, frame, wallpaper);
        encoder.resolve_query_set(&self.queries, 0..4, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 32);
        let labels = self
            .glass
            .renderer(0)
            .and_then(|r| r.profiler.as_ref())
            .map(|p| {
                let labels = p.labels();
                let n = labels.len() as u32 + 1;
                encoder.resolve_query_set(p.queries(), 0..n, &self.pass_resolve, 0);
                encoder.copy_buffer_to_buffer(&self.pass_resolve, 0, &self.pass_read, 0, u64::from(n) * 8);
                labels
            })
            .unwrap_or_default();
        self.gpu.queue.submit([encoder.finish()]);
        let ticks = |bytes: &[u8]| -> Vec<u64> {
            bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| u64::from_ne_bytes(*b))
                .collect()
        };
        let stamps = self.map(&self.read, ticks);
        let ms = |s: &[u64], a: usize, b: usize| s[b].saturating_sub(s[a]) as f64 * self.period / 1e6;
        let passes = if labels.is_empty() {
            Vec::new()
        } else {
            let s = self.map(&self.pass_read, ticks);
            labels.iter().enumerate().map(|(i, l)| (*l, ms(&s, i, i + 1))).collect()
        };
        ([ms(&stamps, 0, 1), ms(&stamps, 1, 2), ms(&stamps, 2, 3)], passes)
    }

    /// Times the glass renderer's passes from the next frame on.
    fn profile(&mut self) {
        let queries = self.gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("lab passes"),
            ty: wgpu::QueryType::Timestamp,
            count: PASS_STAMPS,
        });
        if let Some(r) = self.glass.renderer(0) {
            r.profiler = Some(liquid_rust::Profiler::new(queries));
        }
    }

    fn map<T>(&self, buffer: &wgpu::Buffer, read: impl FnOnce(&[u8]) -> T) -> T {
        let (sender, receiver) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = sender.send(r);
        });
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receiver.recv().unwrap().unwrap();
        let value = read(&buffer.get_mapped_range(..).unwrap());
        buffer.unmap();
        value
    }

    /// Reads a whole texture back as tightly packed BGRA rows.
    fn pixels(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let (width, height) = (texture.width(), texture.height());
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lab readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.gpu.queue.submit([encoder.finish()]);
        self.map(&buffer, |bytes| {
            bytes
                .chunks_exact(row as usize)
                .flat_map(|r| &r[..width as usize * 4])
                .copied()
                .collect()
        })
    }
}

/// GPU ms per renderer pass across frames; a label seen again in one frame gets a number.
#[derive(Default)]
struct Passes(Vec<(String, Vec<f64>)>);

impl Passes {
    fn add(&mut self, frame: &[(&'static str, f64)]) {
        let mut seen: Vec<&str> = Vec::new();
        for (label, ms) in frame {
            let n = seen.iter().filter(|l| *l == label).count();
            seen.push(label);
            let name = if n == 0 {
                (*label).to_string()
            } else {
                format!("{label} #{}", n + 1)
            };
            match self.0.iter_mut().find(|(l, _)| *l == name) {
                Some((_, values)) => values.push(*ms),
                None => self.0.push((name, vec![*ms])),
            }
        }
    }
}

/// Median, p95 and maximum of `values`.
fn spread(values: &mut [f64]) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    [at(0.5), at(0.95), at(1.0)]
}

#[test]
#[ignore = "dev tool: MYPAPER_LAB=<theme>;<out dir>;<t1,t2,..>;<frames>"]
fn lab() {
    let spec = std::env::var("MYPAPER_LAB").expect("set MYPAPER_LAB");
    let parts: Vec<&str> = spec.split(';').collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = PathBuf::from(parts[1]);
    std::fs::create_dir_all(&out).unwrap();
    let times: Vec<f32> = parts.get(2).map_or(vec![12.0], |s| {
        s.split(',').map(|t| t.parse().unwrap()).collect()
    });
    let frames: u32 = parts.get(3).map_or(150, |s| s.parse().unwrap());
    let _apartment = mypaper_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let mut lab = Lab::new(&root.join(parts[0]));
    lab.glass.set_plates(fixture(0.0));
    lab.glass.settle();

    let mut files = Vec::new();
    for (i, &t) in times.iter().enumerate() {
        let encoder = lab.encode(Duration::from_secs_f32(t), i as u32, true);
        lab.gpu.queue.submit([encoder.finish()]);
        for (texture, suffix) in [(&lab.target, ""), (&lab.kept.texture, "-kept")] {
            let name = format!("{t}s{suffix}.bgra");
            std::fs::write(out.join(&name), lab.pixels(texture)).unwrap();
            let (w, h) = (texture.width(), texture.height());
            files.push(format!(
                "{{\"file\":\"{name}\",\"width\":{w},\"height\":{h}}}"
            ));
        }
    }
    std::fs::write(out.join("frames.json"), format!("[{}]", files.join(","))).unwrap();

    // Wallpaper frames at 30 fps from 5 s, then glass-only frames with the dock moving.
    lab.profile();
    let mut wallpaper = [vec![], vec![], vec![], vec![]];
    let mut wallpaper_passes = Passes::default();
    for f in 0..frames {
        let clock = Duration::from_secs_f64(5.0 + f64::from(f) / 30.0);
        let (ms, passes) = lab.timed(clock, f, true);
        for (k, v) in ms.iter().enumerate() {
            wallpaper[k].push(*v);
        }
        wallpaper[3].push(ms.iter().sum());
        wallpaper_passes.add(&passes);
    }
    let mut glass = [vec![], vec![], vec![]];
    let mut glass_passes = Passes::default();
    for f in 0..frames {
        lab.glass
            .set_plates(fixture(160.0 * (f as f32 * 0.05).sin()));
        let (ms, passes) = lab.timed(Duration::from_secs(5), f, false);
        glass[0].push(ms[1]);
        glass[1].push(ms[2]);
        glass[2].push(ms[1] + ms[2]);
        glass_passes.add(&passes);
    }
    let names = ["wallpaper", "restore", "glass", "frame"];
    let mut report = Vec::new();
    for (name, values) in names.iter().zip(&mut wallpaper) {
        report.push((format!("wallpaper-frame {name}"), spread(values)));
    }
    for (name, values) in &mut wallpaper_passes.0 {
        report.push((format!("wallpaper-frame | {name}"), spread(values)));
    }
    for (name, values) in ["restore", "glass", "frame"].iter().zip(&mut glass) {
        report.push((format!("glass-only {name}"), spread(values)));
    }
    for (name, values) in &mut glass_passes.0 {
        report.push((format!("glass-only | {name}"), spread(values)));
    }
    let mut json = Vec::new();
    for (name, [median, p95, max]) in &report {
        eprintln!("{name:>30}: median {median:.3} / p95 {p95:.3} / max {max:.3} ms");
        json.push(format!(
            "\"{name}\":{{\"median\":{median},\"p95\":{p95},\"max\":{max}}}"
        ));
    }
    std::fs::write(out.join("timing.json"), format!("{{{}}}", json.join(","))).unwrap();
    eprintln!("wrote {}", out.display());
}

/// This process's private bytes in MiB.
fn private_mib() -> f64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: our own process; the struct's size goes with it.
    let _ = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    counters.PrivateUsage as f64 / 1048576.0
}

/// Private bytes after each step of a two-screen engine's allocations (no swapchains).
/// `MYPAPER_LAB_THEME=themes\rain-city cargo test -p mypaper-wallpaper lab::memory -- --ignored --nocapture`
#[test]
#[ignore = "dev tool: MYPAPER_LAB_THEME=<theme>"]
fn memory() {
    let theme = std::env::var("MYPAPER_LAB_THEME").expect("set MYPAPER_LAB_THEME");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut last = private_mib();
    let mut step = |name: &str, gpu: Option<&Gpu>| {
        if let Some(gpu) = gpu {
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
        }
        let now = private_mib();
        let report = gpu.and_then(|g| g.device.generate_allocator_report());
        let pool = report.map_or(String::new(), |r| {
            format!(
                ", allocator {:.1} used / {:.1} reserved MiB",
                r.total_allocated_bytes as f64 / 1048576.0,
                r.total_reserved_bytes as f64 / 1048576.0
            )
        });
        eprintln!(
            "{name:>32}: {now:8.2} MiB private ({:+7.2}){pool}",
            now - last
        );
        last = now;
    };
    step("start", None);
    let _apartment = mypaper_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    step("device", Some(&gpu));
    let theme = mypaper_theme::LoadedTheme::load(root.join(theme)).unwrap();
    let mut source = sources::load(&gpu, &theme, &[]).unwrap();
    step("source loaded", Some(&gpu));
    let size = source.render_size(SIZE);
    let kept = [KeptFrame::new(&gpu, size), KeptFrame::new(&gpu, size)];
    step("two kept frames", Some(&gpu));
    let targets: Vec<_> = (0..2)
        .map(|_| {
            gpu.texture(
                "screen stand-in",
                SIZE,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                None,
            )
        })
        .collect();
    step("two 2560x1440 stand-ins", Some(&gpu));
    let monitor = Rect {
        x: 0,
        y: 0,
        width: SIZE.0 as i32,
        height: SIZE.1 as i32,
    };
    let mut glass = GlassPass::new(Some(&[monitor, monitor])).unwrap();
    glass.set_plates(fixture(0.0));
    for (i, (kept, target)) in kept.iter().zip(&targets).enumerate() {
        let view = target.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.render(
            &gpu,
            Frame {
                screen: i,
                size,
                mouse: [0.0, 0.0],
                clock: Duration::from_secs(5),
                frame: 1,
                view: &kept.view,
                encoder: &mut encoder,
            },
        );
        kept.restore(&gpu, target, &view, &mut encoder);
        kept.drawn.set(Some(1));
        gpu.queue.submit([encoder.finish()]);
    }
    step("first frames", Some(&gpu));
    for (i, (kept, target)) in kept.iter().zip(&targets).enumerate() {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        glass.encode(&gpu, i, kept, target, &mut encoder);
        gpu.queue.submit([encoder.finish()]);
    }
    step("glass on both", Some(&gpu));
    // Two backdrop pyramids (backdrop.rs DIVISORS), as two screens of different kept frames keep.
    let pyramids: Vec<_> = (0..2)
        .flat_map(|_| [2, 4, 8, 16, 32, 64])
        .map(|d: u32| {
            let size = (SIZE.0.div_ceil(d).max(1), SIZE.1.div_ceil(d).max(1));
            gpu.texture(
                "backdrop",
                size,
                gpu.format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                None,
            )
        })
        .collect();
    step("two backdrop pyramids", Some(&gpu));
    drop(pyramids);
    step("pyramids dropped", Some(&gpu));
    drop(kept);
    drop(targets);
    step("frames dropped", Some(&gpu));
}

/// Private bytes for screen-sized textures, freed and allocated again, under
/// `MYPAPER_LAB_BLOCK=<MiB>` suballocation blocks (`MemoryHints::Manual`), or under
/// `MemoryUsage` without it. On the integrated GPU a freed heap's memory stays committed.
/// `cargo test -p mypaper-wallpaper lab::memory_hints -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool"]
fn memory_hints() {
    let hints = std::env::var("MYPAPER_LAB_BLOCK").map_or(wgpu::MemoryHints::MemoryUsage, |mib| {
        let bytes = mib.parse::<u64>().unwrap() << 20;
        wgpu::MemoryHints::Manual {
            suballocated_device_memory_block_size: bytes..bytes,
        }
    });
    let instance = wgpu::Instance::new({
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        desc.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
        desc
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        ..Default::default()
    }))
    .unwrap();
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        memory_hints: hints.clone(),
        ..Default::default()
    }))
    .unwrap();
    let wait = || device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let texture = |w, h| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    wait();
    let start = private_mib();
    eprintln!("{hints:?}: device at {start:.2} MiB");
    let report = |label: &str| {
        wait();
        let r = device.generate_allocator_report().unwrap();
        eprintln!(
            "{label:>36}: {:+8.2} MiB private, allocator {:.1} / {:.1} MiB",
            private_mib() - start,
            r.total_allocated_bytes as f64 / 1048576.0,
            r.total_reserved_bytes as f64 / 1048576.0
        );
    };
    let a: Vec<_> = (0..4).map(|_| texture(2560, 1440)).collect();
    report("four 2560x1440 textures");
    drop(a);
    report("dropped");
    let b: Vec<_> = (0..4).map(|_| texture(2560, 1440)).collect();
    report("four again");
    drop(b);
    report("dropped");
    let c: Vec<_> = (0..16).map(|_| texture(640, 1440)).collect();
    report("sixteen 640x1440 (same bytes)");
    drop(c);
    let _small: Vec<_> = (0..64).map(|_| texture(320, 180)).collect();
    report("sixty-four 320x180");
}
