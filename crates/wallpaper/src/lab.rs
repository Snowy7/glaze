//! Offline frame lab (dev tool, `#[ignore]`): one screen of the engine's real per-frame path
//! (wallpaper into the kept frame, restore into a 2560x1440 target, the fixture's eight glass
//! plates) on the default integrated GPU, without touching the desktop. It times every pass
//! with GPU timestamps and dumps raw frames, so a change can be measured and pixel-diffed
//! (`tools/perf/diff-frames.py`) before anyone takes the desktop lock.
//!
//! ```text
//! set GLAZE_LAB=themes\rain-city;%TEMP%\glaze-lab\before;3,12,30;150
//! cargo test -p glaze-wallpaper lab::lab -- --ignored --exact --nocapture
//! ```
//!
//! Fields: theme folder (from the repo root); output folder; capture times in seconds; timed
//! frames. Each capture writes `<t>s.bgra` (the final 2560x1440 frame) and `<t>s-kept.bgra`
//! (the wallpaper at its render scale), tightly packed BGRA, with sizes in `frames.json`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use glaze_desktop::Rect;
use glaze_glass_ipc::{Material, Plate, Style};

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

/// One plate per line, `x,y,w,h,radius,clarity,blur,refraction,highlight,r,g,b,a` in physical
/// pixels of the lab's screen (`GLAZE_LAB_PLATES`; `lab::live_plates` prints this form).
fn plates_file(path: &Path) -> Vec<Plate> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .enumerate()
        .map(|(i, line)| {
            let v: Vec<f32> = line.split(',').map(|x| x.trim().parse().unwrap()).collect();
            Plate {
                id: i as u64,
                rect: [v[0], v[1], v[2], v[3]],
                radius: v[4],
                material: Material {
                    clarity: v[5],
                    blur: v[6],
                    refraction: v[7],
                    highlight: v[8],
                    tint: [v[9], v[10], v[11], v[12]],
                },
            }
        })
        .collect()
}

/// The shell's live plates on the screen at the origin (or monitor `GLAZE_LAB_SCREEN`, moved to
/// the origin), in [`plates_file`]'s form (a passive read of the shared snapshot).
#[test]
#[ignore = "dev tool: needs a running shell"]
fn live_plates() {
    let mut reader = glaze_glass_ipc::Reader::open().unwrap();
    let snapshot = reader.read_changed().unwrap().expect("a snapshot");
    let monitors = glaze_desktop::monitor_infos();
    eprintln!("{} plates; screens: {monitors:?}", snapshot.plates.len());
    let origin = std::env::var("GLAZE_LAB_SCREEN")
        .ok()
        .and_then(|n| monitors.get(n.parse::<usize>().ok()?))
        .map_or((0.0, 0.0), |m| (m.rect.x as f32, m.rect.y as f32));
    for p in snapshot.plates {
        let ([x, y, w, h], m) = (p.rect, p.material);
        let (x, y) = (x - origin.0, y - origin.1);
        if x >= 0.0 && y >= 0.0 && x < SIZE.0 as f32 && y < SIZE.1 as f32 {
            let [r, g, b, a] = m.tint;
            println!(
                "{x},{y},{w},{h},{},{},{},{},{},{r},{g},{b},{a}",
                p.radius, m.clarity, m.blur, m.refraction, m.highlight
            );
        }
    }
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
        let theme = glaze_theme::LoadedTheme::load(theme).unwrap();
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

    /// Submits one timed frame, waits for it and returns [wallpaper, restore, glass] in GPU ms,
    /// the CPU ms of recording and submitting it (nothing in that waits for the GPU), and the
    /// glass renderer's passes (label, ms) once [`Lab::profile`] is on.
    fn timed(
        &mut self,
        clock: Duration,
        frame: u32,
        wallpaper: bool,
    ) -> ([f64; 3], f64, Vec<(&'static str, f64)>) {
        let started = std::time::Instant::now();
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
                encoder.copy_buffer_to_buffer(
                    &self.pass_resolve,
                    0,
                    &self.pass_read,
                    0,
                    u64::from(n) * 8,
                );
                labels
            })
            .unwrap_or_default();
        self.gpu.queue.submit([encoder.finish()]);
        let cpu = started.elapsed().as_secs_f64() * 1e3;
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
        (
            [ms(&stamps, 0, 1), ms(&stamps, 1, 2), ms(&stamps, 2, 3)],
            cpu,
            passes,
        )
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
#[ignore = "dev tool: GLAZE_LAB=<theme>;<out dir>;<t1,t2,..>;<frames>"]
fn lab() {
    let spec = std::env::var("GLAZE_LAB").expect("set GLAZE_LAB");
    let parts: Vec<&str> = spec.split(';').collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = PathBuf::from(parts[1]);
    std::fs::create_dir_all(&out).unwrap();
    let times: Vec<f32> = parts.get(2).map_or(vec![12.0], |s| {
        s.split(',').map(|t| t.parse().unwrap()).collect()
    });
    let frames: u32 = parts.get(3).map_or(150, |s| s.parse().unwrap());
    let _apartment = glaze_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let mut lab = Lab::new(&root.join(parts[0]));
    let custom = std::env::var_os("GLAZE_LAB_PLATES").map(|p| plates_file(Path::new(&p)));
    // Optional optics fixture: brightness,saturation,smoothing,sheen,depth,inner-shadow.
    // Negative sheen/depth retain the highlight-derived defaults, just like the overlay wire.
    let style = std::env::var("GLAZE_LAB_STYLE")
        .ok()
        .map(|line| {
            let v: Vec<f32> = line.split(',').map(|x| x.trim().parse().unwrap()).collect();
            assert_eq!(v.len(), 6);
            Style {
                brightness: v[0],
                saturation: v[1],
                smoothing: v[2],
                sheen: (v[3] >= 0.0).then_some(v[3]),
                depth: (v[4] >= 0.0).then_some(v[4]),
                inner_shadow: v[5],
            }
        })
        .unwrap_or_default();
    lab.glass.set_styled(
        custom
            .clone()
            .unwrap_or_else(|| fixture(0.0))
            .into_iter()
            .map(|p| (p, style))
            .collect(),
    );
    lab.glass.settle();
    if custom.is_some() {
        // Captures only: the timed runs move the fixture's dock.
        let mut entries = Vec::new();
        for (i, &t) in times.iter().enumerate() {
            let encoder = lab.encode(Duration::from_secs_f32(t), i as u32, true);
            lab.gpu.queue.submit([encoder.finish()]);
            let name = format!("{t}s.bgra");
            std::fs::write(out.join(&name), lab.pixels(&lab.target)).unwrap();
            let (w, h) = SIZE;
            entries.push(format!("{{\"file\":\"{name}\",\"width\":{w},\"height\":{h}}}"));
        }
        std::fs::write(out.join("frames.json"), format!("[{}]", entries.join(","))).unwrap();
        eprintln!("wrote {}", out.display());
        return;
    }

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
    let mut wallpaper = [vec![], vec![], vec![], vec![], vec![]];
    let mut wallpaper_passes = Passes::default();
    for f in 0..frames {
        let clock = Duration::from_secs_f64(5.0 + f64::from(f) / 30.0);
        let (ms, cpu, passes) = lab.timed(clock, f, true);
        for (k, v) in ms.iter().enumerate() {
            wallpaper[k].push(*v);
        }
        wallpaper[3].push(ms.iter().sum());
        wallpaper[4].push(cpu);
        wallpaper_passes.add(&passes);
    }
    let mut glass = [vec![], vec![], vec![], vec![]];
    let mut glass_passes = Passes::default();
    for f in 0..frames {
        lab.glass.set_styled(
            fixture(160.0 * (f as f32 * 0.05).sin())
                .into_iter()
                .map(|p| (p, style))
                .collect(),
        );
        let (ms, cpu, passes) = lab.timed(Duration::from_secs(5), f, false);
        glass[0].push(ms[1]);
        glass[1].push(ms[2]);
        glass[2].push(ms[1] + ms[2]);
        glass[3].push(cpu);
        glass_passes.add(&passes);
    }
    let names = ["wallpaper", "restore", "glass", "frame", "cpu"];
    let mut report = Vec::new();
    for (name, values) in names.iter().zip(&mut wallpaper) {
        report.push((format!("wallpaper-frame {name}"), spread(values)));
    }
    for (name, values) in &mut wallpaper_passes.0 {
        report.push((format!("wallpaper-frame | {name}"), spread(values)));
    }
    for (name, values) in ["restore", "glass", "frame", "cpu"].iter().zip(&mut glass) {
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

use crate::trim::private_mib;

/// Transparent overlay fixture: proves the continuous edge keeps its own coverage and that
/// the independent shadow darkens the bottom without changing the top. Dumps inspectable BGRA.
#[test]
#[ignore = "dev tool: GLAZE_PARITY_OUT=<output directory>"]
fn glass_parity() {
    use liquid_rust as lr;
    let out = PathBuf::from(std::env::var_os("GLAZE_PARITY_OUT").expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lab = Lab::new(&root.join("themes/rain-city"));
    let gpu = &lab.gpu;
    let size = (512, 320);
    let format = gpu.format.add_srgb_suffix();
    let content = gpu.texture(
        "parity backdrop",
        size,
        format,
        wgpu::TextureUsages::TEXTURE_BINDING,
        None,
    );
    gpu.write_texture(
        &content,
        &[140, 160, 180, 255].repeat((size.0 * size.1) as usize),
    );
    let content = content.create_view(&Default::default());
    let target = gpu.texture(
        "parity glass",
        size,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let view = target.create_view(&Default::default());
    let mut renderer = lr::Renderer::new(&gpu.device, format);
    let mut render = |clip, shadow| {
        let mut scene = lr::Scene::new();
        scene.physics = false;
        scene.add(
            lr::Glass::new(lr::Rect::new(48.0, 48.0, 416.0, 224.0))
                .shape(crate::material::shape(48.0, 0.6))
                .material(lr::Material {
                    refraction: 0.0,
                    dispersion: 0.0,
                    frost: 0.0,
                    specular: 0.0,
                    rim: 0.0,
                    rim_shade: 0.0,
                    inner_shadow: shadow,
                    clip_to_shape: clip,
                    adaptive: false,
                    ..lr::Material::regular()
                })
                .appearance(lr::Appearance {
                    adapt_to_content: false,
                    ..Default::default()
                }),
        );
        scene.update(0.0);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &lr::Frame {
                content: &content,
                target: &view,
                size,
                scale: 1.0,
                content_changed: true,
                copy_content: false,
            },
            &scene,
        );
        gpu.queue.submit([encoder.finish()]);
        lab.pixels(&target)
    };
    let uncut = render(false, 0.0);
    let clipped = render(true, 0.0);
    let shadow = render(true, 1.0);
    let pixel = |data: &[u8], x, y| data[((y * size.0 + x) * 4) as usize..][..4].to_vec();
    assert_eq!(
        pixel(&clipped, 48, 48)[3],
        0,
        "corner outside the contour stays transparent"
    );
    assert_eq!(
        pixel(&clipped, 256, 48)[3],
        255,
        "straight edge stays covered"
    );
    assert!(
        uncut
            .as_chunks::<4>().0.iter()
            .zip(clipped.as_chunks::<4>().0)
            .any(|(a, b)| a[3] > 0 && b[3] == 0),
        "the shader clip removes the outer shadow/hairline"
    );
    assert_eq!(
        pixel(&clipped, 256, 50),
        pixel(&shadow, 256, 50),
        "top rim is unchanged"
    );
    assert!(
        pixel(&shadow, 256, 269)[0] < pixel(&clipped, 256, 269)[0],
        "bottom darkens"
    );
    assert!(
        clipped
            .as_chunks::<4>().0.iter()
            .zip(shadow.as_chunks::<4>().0)
            .all(|(a, b)| a[3] == b[3]),
        "inner shadow cannot change the shape"
    );
    for (name, bytes) in [
        ("continuous", clipped),
        ("bottom-shadow", shadow),
        ("unclipped", uncut),
    ] {
        std::fs::write(out.join(format!("{name}.bgra")), bytes).unwrap();
    }
    eprintln!("512x320 overlay fixtures: {}", out.display());
}

/// Private bytes after each step of a two-screen engine's allocations (no swapchains).
/// `GLAZE_LAB_THEME=themes\rain-city cargo test -p glaze-wallpaper lab::memory -- --ignored --nocapture`
#[test]
#[ignore = "dev tool: GLAZE_LAB_THEME=<theme>"]
fn memory() {
    let theme = std::env::var("GLAZE_LAB_THEME").expect("set GLAZE_LAB_THEME");
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
    let _apartment = glaze_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    step("device", Some(&gpu));
    let theme = glaze_theme::LoadedTheme::load(root.join(theme)).unwrap();
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

/// What releasing the GPU frees and what bringing it back costs, on the engine's own device path
/// (`Opening`, `engine.gpu: auto`): the theme drawn into two screens with glass, then released
/// down to each level and brought back, three times. Swapchains are measured live (they need
/// windows). `GLAZE_LAB_THEME=themes\rain-city cargo test -p glaze-wallpaper --release
/// lab::release -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool: GLAZE_LAB_THEME=<theme>"]
fn release() {
    use std::time::Instant;
    let theme = std::env::var("GLAZE_LAB_THEME").expect("set GLAZE_LAB_THEME");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let theme = glaze_theme::LoadedTheme::load(root.join(theme)).unwrap();
    let _apartment = glaze_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let monitor = Rect {
        x: 0,
        y: 0,
        width: SIZE.0 as i32,
        height: SIZE.1 as i32,
    };
    let mut last = private_mib();
    let mut step = |name: &str, started: Instant| {
        let now = private_mib();
        eprintln!(
            "{name:>34}: {now:8.2} MiB private ({:+8.2}), {:7.1} ms",
            now - last,
            started.elapsed().as_secs_f64() * 1e3
        );
        last = now;
    };
    // Two screens' kept frame, stand-ins and glass, with one frame drawn.
    let draw = |gpu: &Gpu, source: &mut Box<dyn Source>| {
        let size = source.render_size(SIZE);
        let kept = KeptFrame::new(gpu, size);
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
            .collect::<Vec<_>>();
        let mut glass = GlassPass::new(Some(&[monitor, monitor])).unwrap();
        glass.set_plates(fixture(0.0));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.update(gpu, Duration::from_secs(5));
        source.render(
            gpu,
            Frame {
                screen: 0,
                size,
                mouse: [0.0, 0.0],
                clock: Duration::from_secs(5),
                frame: 1,
                view: &kept.view,
                encoder: &mut encoder,
            },
        );
        kept.drawn.set(Some(1));
        for (i, target) in targets.iter().enumerate() {
            let view = target.create_view(&Default::default());
            kept.restore(gpu, target, &view, &mut encoder);
            glass.encode(gpu, i, &kept, target, &mut encoder);
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        (kept, targets, glass)
    };

    step("start", Instant::now());
    let t = Instant::now();
    let mut gpu = Gpu::opened(crate::gpu::Opening::start(
        glaze_config::GpuPreference::Auto,
    ));
    eprintln!("GPU: {}", gpu.adapter.get_info().name);
    step("cold: device opened", t);
    for round in 1..=3 {
        // Twice on one device: what a reload costs without a new device.
        for again in ["", " again"] {
            let t = Instant::now();
            let mut source = sources::load(&gpu, &theme, &[]).unwrap();
            step(&format!("{round}: source loaded{again}"), t);
            let t = Instant::now();
            let first = draw(&gpu, &mut source);
            step(&format!("{round}: first frame + glass{again}"), t);
            let t = Instant::now();
            let second = draw(&gpu, &mut source);
            step(&format!("{round}: second frame + glass{again}"), t);
            let t = Instant::now();
            drop((first, second, source));
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            step(&format!("{round}: drawing released{again}"), t);
        }
        let t = Instant::now();
        let (instance, adapter) = gpu.into_adapter();
        step(&format!("{round}: device released"), t);
        let t = Instant::now();
        let reopened = Gpu::from_adapter(instance, adapter);
        step(&format!("{round}: device on the kept adapter"), t);
        let t = Instant::now();
        drop(reopened);
        step(&format!("{round}: everything released"), t);
        let t = Instant::now();
        gpu = Gpu::opened(crate::gpu::Opening::start(
            glaze_config::GpuPreference::Auto,
        ));
        step(&format!("{round}: warm: device opened"), t);
    }
}

/// Private bytes over release/resume cycles on one device (what `Engine::release` drops and
/// `resume` makes again: the source, a kept frame, two screen-sized targets, glass), on the
/// integrated GPU, or the discrete one with `GLAZE_LAB_GPU=high`. Does the memory come back,
/// and does it grow per cycle? `GLAZE_LAB_THEME=themes\rain-city cargo test -p
/// glaze-wallpaper --release lab::release_cycles -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool: GLAZE_LAB_THEME=<theme>"]
fn release_cycles() {
    let theme = std::env::var("GLAZE_LAB_THEME").expect("set GLAZE_LAB_THEME");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let theme = glaze_theme::LoadedTheme::load(root.join(theme)).unwrap();
    let _apartment = glaze_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let gpu = Gpu::headless().expect("a DX12 GPU");
    let monitor = Rect {
        x: 0,
        y: 0,
        width: SIZE.0 as i32,
        height: SIZE.1 as i32,
    };
    let wait = || {
        for _ in 0..2 {
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
    };
    let mut line = format!("start {:.0}", private_mib());
    for _ in 0..6 {
        let mut source = sources::load(&gpu, &theme, &[]).unwrap();
        let kept = KeptFrame::new(&gpu, source.render_size(SIZE));
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
        let mut glass = GlassPass::new(Some(&[monitor, monitor])).unwrap();
        glass.set_plates(fixture(0.0));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.render(
            &gpu,
            Frame {
                screen: 0,
                size: kept.size(),
                mouse: [0.0, 0.0],
                clock: Duration::from_secs(5),
                frame: 1,
                view: &kept.view,
                encoder: &mut encoder,
            },
        );
        kept.drawn.set(Some(1));
        for (i, target) in targets.iter().enumerate() {
            let view = target.create_view(&Default::default());
            kept.restore(&gpu, target, &view, &mut encoder);
            glass.encode(&gpu, i, &kept, target, &mut encoder);
        }
        gpu.queue.submit([encoder.finish()]);
        wait();
        line += &format!(" | drawn {:.0}", private_mib());
        drop((source, kept, targets, glass));
        wait();
        std::thread::sleep(Duration::from_secs(4));
        line += &format!(" released {:.0}", private_mib());
    }
    eprintln!("{line}");
}

/// Idles 12 s in each of three states, for a wake sampler run alongside on this process: the
/// device with a frame drawn, the drawing released (device kept), and no device at all.
/// `cargo test -p glaze-wallpaper --release lab::idle -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool: sample this process's thread wakes while it runs"]
fn idle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let theme = glaze_theme::LoadedTheme::load(root.join("themes/rain-city")).unwrap();
    let phase = |name: &str| {
        eprintln!("PHASE {name} pid {} at {:?}", std::process::id(), std::time::SystemTime::now());
        std::thread::sleep(Duration::from_secs(12));
    };
    let gpu = Gpu::opened(crate::gpu::Opening::start(
        glaze_config::GpuPreference::Auto,
    ));
    let mut source = sources::load(&gpu, &theme, &[]).unwrap();
    let kept = KeptFrame::new(&gpu, source.render_size(SIZE));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    source.render(
        &gpu,
        Frame {
            screen: 0,
            size: kept.size(),
            mouse: [0.0, 0.0],
            clock: Duration::from_secs(5),
            frame: 1,
            view: &kept.view,
            encoder: &mut encoder,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    phase("drawn");
    drop((source, kept));
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    phase("device only");
    drop(gpu);
    phase("no device");
}

/// Like [`idle`], with a swapchain: which driver threads presenting adds and whether they outlive
/// the swapchain (the engine releases it while every screen is covered, keeping the device). A
/// hidden window of the engine's class, a few presented frames, then 12 s each: presented,
/// swapchain released, no device.
/// `cargo test -p glaze-wallpaper --release lab::idle_present -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool: sample this process's thread wakes while it runs"]
fn idle_present() {
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, WS_POPUP};
    let phase = |name: &str| {
        eprintln!("PHASE {name} pid {} at {:?}", std::process::id(), std::time::SystemTime::now());
        std::thread::sleep(Duration::from_secs(12));
    };
    let _ = crate::window::register_class();
    // SAFETY: the engine's registered class; a hidden popup this test owns (`Window` drops it).
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            windows::core::w!("glaze.wallpaper"),
            windows::core::w!("lab present"),
            WS_POPUP,
            0,
            0,
            640,
            360,
            None,
            None,
            None,
            None,
        )
    }
    .unwrap();
    let windows = vec![crate::window::Window(hwnd)];
    let opening = crate::gpu::Opening::start(glaze_config::GpuPreference::Auto);
    let (gpu, surfaces) = Gpu::new(opening, &windows).unwrap();
    let window = windows.into_iter().next().unwrap();
    let surface = surfaces.into_iter().next().unwrap();
    let mut screen = crate::gpu::Screen::new(&gpu, surface, window).unwrap();
    for _ in 0..3 {
        if let Some(texture) = screen.acquire(&gpu) {
            let view = texture.texture.create_view(&Default::default());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            drop(sources::begin(&mut encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::BLACK)));
            gpu.queue.submit([encoder.finish()]);
            gpu.queue.present(texture);
        }
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    phase("presented");
    screen.release();
    for _ in 0..2 {
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
    phase("swapchain released");
    drop(screen);
    drop(gpu);
    phase("no device");
}

/// Private bytes for screen-sized textures, freed and allocated again, under
/// `GLAZE_LAB_BLOCK=<MiB>` suballocation blocks (`MemoryHints::Manual`), or under
/// `MemoryUsage` without it. On the integrated GPU a freed heap's memory stays committed.
/// `cargo test -p glaze-wallpaper lab::memory_hints -- --ignored --exact --nocapture`
#[test]
#[ignore = "dev tool"]
fn memory_hints() {
    let hints = std::env::var("GLAZE_LAB_BLOCK").map_or(wgpu::MemoryHints::MemoryUsage, |mib| {
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
