//! Optional, nonblocking GPU timestamp telemetry for tools/perf. Four stamps separate the
//! wallpaper, restore/backdrop, and plate passes. No allocations or queries when disabled.
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};

use crate::gpu::Gpu;
use anyhow::{Context, Result, ensure};

pub const FEATURES: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

pub fn requested() -> bool {
    std::env::var_os("MYPAPER_GPU_TIMINGS").is_some()
}

/// Two GPU timestamps, allocated only for a covered change or a Studio request.
pub struct OneOff {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl OneOff {
    pub fn new(gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) -> Option<Self> {
        if !gpu.device.features().contains(FEATURES) {
            return None;
        }
        let this = Self {
            queries: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("covered refresh timing"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("covered refresh timing resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("covered refresh timing readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        };
        encoder.write_timestamp(&this.queries, 0);
        Some(this)
    }

    pub fn finish(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.queries, 1);
        encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 16);
    }

    pub fn read(self, gpu: &Gpu) -> Option<f64> {
        let (tx, rx) = mpsc::channel();
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = tx.send(result);
            });
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let bytes = self.readback.get_mapped_range(..).ok()?;
        let start = u64::from_ne_bytes(bytes[..8].try_into().ok()?);
        let end = u64::from_ne_bytes(bytes[8..16].try_into().ok()?);
        let ms = end.saturating_sub(start) as f64 * f64::from(gpu.queue.get_timestamp_period())
            / 1_000_000.0;
        drop(bytes);
        self.readback.unmap();
        Some(ms)
    }
}

pub struct Timing {
    slots: Vec<Slot>,
    sender: Option<mpsc::Sender<String>>,
    worker: Option<std::thread::JoinHandle<()>>,
    period: f64,
    composition: bool,
}
struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    ready: Arc<AtomicU8>, // 0 free, 1 mapping, 2 ready, 3 failed
    meta: Meta,
}
#[derive(Default, Clone, Copy)]
pub struct Meta {
    pub screen: usize,
    pub frame: u32,
    /// The submission shows a new wallpaper frame.
    pub wallpaper: bool,
    /// It also ran the wallpaper pass (false when a screen sharing the kept frame did).
    pub pass: bool,
    pub sequence: u64,
    pub published_qpc: u64,
    pub submit_qpc: u64,
    pub glass: bool,
    pub composition_qpc: u64,
}
impl Timing {
    pub fn new(gpu: &Gpu) -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("MYPAPER_GPU_TIMINGS") else {
            return Ok(None);
        };
        ensure!(
            gpu.device.features().contains(FEATURES),
            "adapter does not support encoder timestamp queries"
        );
        let file = std::fs::File::create(&path).context("creating GPU timing JSONL")?;
        let (sender, receiver) = mpsc::channel::<String>();
        let worker = std::thread::spawn(move || {
            let mut file = std::io::BufWriter::new(file);
            for line in receiver {
                if writeln!(file, "{line}").is_err() {
                    break;
                }
            }
        });
        let slots = (0..8)
            .map(|_| Slot {
                queries: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("frame timing"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 4,
                }),
                resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp resolve"),
                    size: 32,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp readback"),
                    size: 32,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                ready: Arc::new(AtomicU8::new(0)),
                meta: Meta::default(),
            })
            .collect();
        Ok(Some(Self {
            slots,
            sender: Some(sender),
            worker: Some(worker),
            composition: std::env::var_os("MYPAPER_COMPOSITION_TIMING").is_some(),
            period: f64::from(gpu.queue.get_timestamp_period()),
        }))
    }

    pub fn begin(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        meta: Meta,
    ) -> Option<usize> {
        self.collect(gpu);
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.ready.load(Ordering::Acquire) == 0)?;
        slot.meta = meta;
        slot.ready.store(1, Ordering::Release);
        encoder.write_timestamp(&slot.queries, 0);
        Some(index)
    }
    pub fn stamp(&self, index: usize, stamp: u32, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.slots[index].queries, stamp);
    }
    pub fn resolve(&self, index: usize, encoder: &mut wgpu::CommandEncoder) {
        let s = &self.slots[index];
        encoder.resolve_query_set(&s.queries, 0..4, &s.resolve, 0);
        encoder.copy_buffer_to_buffer(&s.resolve, 0, &s.read, 0, 32);
    }
    pub fn submitted(&mut self, index: usize, glass: bool) {
        let s = &mut self.slots[index];
        s.meta.submit_qpc = mypaper_glass_ipc::qpc();
        s.meta.glass = glass;
        let ready = s.ready.clone();
        s.read.map_async(wgpu::MapMode::Read, .., move |r| {
            ready.store(if r.is_ok() { 2 } else { 3 }, Ordering::Release)
        });
    }
    /// Benchmark-only upper bound: DWM has drawn this process's queued surface changes when
    /// DwmFlush returns. Once per engine frame, after both screens have been presented.
    pub fn presented(&mut self, frame: u32) {
        if !self.composition {
            return;
        }
        // SAFETY: synchronizes this process's DWM updates; no pointers or handles required.
        if let Err(error) = unsafe { windows::Win32::Graphics::Dwm::DwmFlush() } {
            log::error!("composition timing: {error}");
            return;
        }
        let now = mypaper_glass_ipc::qpc();
        for slot in &mut self.slots {
            if slot.meta.frame == frame && slot.ready.load(Ordering::Acquire) != 0 {
                slot.meta.composition_qpc = now;
            }
        }
    }
    pub fn pending(&self) -> bool {
        self.slots
            .iter()
            .any(|s| s.ready.load(Ordering::Acquire) != 0)
    }
    pub fn collect(&mut self, gpu: &Gpu) {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        for s in &mut self.slots {
            match s.ready.load(Ordering::Acquire) {
                2 if !self.composition || s.meta.composition_qpc != 0 => {
                    let bytes = s
                        .read
                        .get_mapped_range(..)
                        .expect("completed timestamp mapping");
                    let times: Vec<_> = bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|b| u64::from_ne_bytes(*b))
                        .collect();
                    let ms = |a: usize, b: usize| {
                        times[b].saturating_sub(times[a]) as f64 * self.period / 1_000_000.0
                    };
                    let m = s.meta;
                    let lag = m.submit_qpc.saturating_sub(m.published_qpc) as f64 * 1000.0
                        / mypaper_glass_ipc::qpc_frequency() as f64;
                    let composed = if m.composition_qpc == 0 {
                        "null".into()
                    } else {
                        (m.composition_qpc.saturating_sub(m.published_qpc) as f64 * 1000.0
                            / mypaper_glass_ipc::qpc_frequency() as f64)
                            .to_string()
                    };
                    let line = format!(
                        "{{\"screen\":{},\"frame\":{},\"wallpaper_drawn\":{},\"wallpaper_pass\":{},\"glass_drawn\":{},\"sequence\":{},\"wallpaper_ms\":{},\"restore_ms\":{},\"glass_ms\":{},\"publish_to_submit_ms\":{},\"publish_to_composition_ms\":{},\"submit_qpc\":{},\"qpc_frequency\":{}}}",
                        m.screen,
                        m.frame,
                        m.wallpaper,
                        m.pass,
                        m.glass,
                        m.sequence,
                        ms(0, 1),
                        ms(1, 2),
                        ms(2, 3),
                        lag,
                        composed,
                        m.submit_qpc,
                        mypaper_glass_ipc::qpc_frequency()
                    );
                    if let Some(sender) = &self.sender {
                        let _ = sender.send(line);
                    }
                    drop(bytes);
                    s.read.unmap();
                    s.ready.store(0, Ordering::Release);
                }
                3 => {
                    log::warn!("timestamp readback failed");
                    s.ready.store(0, Ordering::Release);
                }
                _ => {}
            }
        }
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
