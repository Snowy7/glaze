//! `mypaper-wallpaper --preview <theme dir> --out <file.jpg> [--size WxH] [--fps N]`: Studio's
//! live preview of a draft theme. A headless child that never touches the desktop: it renders
//! the theme offscreen at `size`, writes `<out>.tmp` and renames it over `out` (so a reader
//! never sees half a file), and renders again whenever a file in the theme folder or a used
//! effect folder changes. While the wallpaper moves it renders at `fps` (default 10); a still
//! one costs nothing between changes. Studio stops it by ending the process.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_POSTMESSAGE,
};

use super::{Frame, Redraw, Source};
use crate::gpu::Gpu;
use crate::window::WM_RELOAD;

pub struct PreviewArgs {
    pub theme: PathBuf,
    pub out: PathBuf,
    pub size: (u32, u32),
    pub fps: u32,
}

impl PreviewArgs {
    /// Parses the flags after `--preview <dir>`.
    pub fn parse(theme: String, rest: &mut impl Iterator<Item = String>) -> Result<Self> {
        let mut args = Self {
            theme: PathBuf::from(theme),
            out: PathBuf::new(),
            size: (1280, 720),
            fps: 10,
        };
        while let Some(flag) = rest.next() {
            let value = rest
                .next()
                .with_context(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--out" => args.out = PathBuf::from(value),
                "--size" => {
                    let (w, h) = value.split_once('x').context("--size is WxH")?;
                    args.size = (w.parse()?, h.parse()?);
                }
                "--fps" => args.fps = value.parse::<u32>()?.clamp(1, 60),
                _ => bail!("unknown preview flag {flag}"),
            }
        }
        if args.out.as_os_str().is_empty() {
            bail!("--preview needs --out <file.jpg>");
        }
        Ok(args)
    }
}

/// Runs until the process is ended.
pub fn run(args: PreviewArgs) -> Result<()> {
    // SAFETY: COM for WIC (decoding images, encoding the JPEG) on this thread.
    let _ = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
    };
    // The integrated GPU: a preview is small, and the desktop may be using the other one.
    let gpu = Gpu::opened(crate::gpu::Opening::start(
        mypaper_config::GpuPreference::Integrated,
    ));
    // SAFETY: no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    // Our own output (and Studio's previews, logs and credits) must not reload the draft.
    let out = std::path::absolute(&args.out)?;
    let _watch = mypaper_fswatch::Watch::directory(&args.theme, true, move |event| {
        let ignored = |path: &std::path::Path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            path.starts_with(&out)
                || name.ends_with(".tmp")
                || name.ends_with(".log")
                || name.starts_with("preview.")
                || name.eq_ignore_ascii_case("CREDITS.md")
        };
        if let mypaper_fswatch::Event::Paths(paths) = &event
            && paths.iter().all(|p| ignored(p))
        {
            return;
        }
        // SAFETY: posting to a thread id is always safe; failure means it is gone.
        let _ = unsafe { PostThreadMessageW(thread, WM_RELOAD, WPARAM(0), LPARAM(0)) };
    })?;
    let started = Instant::now();
    let mut source: Option<Box<dyn Source>> = None;
    let mut reload = true;
    let mut frame = 0u32;
    loop {
        if reload {
            reload = false;
            // Drop the old source (and its effect watches) before loading the new one.
            source = None;
            match LoadedTheme::load(&args.theme).and_then(|theme| super::load(&gpu, &theme, &[])) {
                Ok(loaded) => source = Some(loaded),
                Err(e) => log::error!("preview: {e:#}"),
            }
        }
        let wait = match &mut source {
            Some(source) => {
                let clock = started.elapsed();
                frame += 1;
                let pixels = render(&gpu, source.as_mut(), args.size, clock, frame);
                if let Err(e) = crate::snapshot::save_jpeg(&gpu, args.size, &pixels, &args.out) {
                    log::error!("preview: {e:#}");
                }
                match source.redraw(clock) {
                    Redraw::Every(_) => Duration::from_secs(1) / args.fps,
                    Redraw::At(at) => at.saturating_sub(clock),
                    Redraw::OnChange => Duration::MAX,
                }
            }
            None => Duration::MAX,
        };
        let ms = if wait == Duration::MAX {
            INFINITE
        } else {
            wait.as_millis().min(u128::from(INFINITE - 1)) as u32
        };
        // SAFETY: waits only on this thread's posted messages.
        unsafe { MsgWaitForMultipleObjects(None, false, ms, QS_POSTMESSAGE) };
        let mut msg = MSG::default();
        // SAFETY: drains this thread's queue; WM_RELOAD comes from the watches.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_RELOAD {
                reload = true;
            }
        }
    }
}

/// Draws one frame of `source` at `size` and returns tightly packed BGRA.
pub fn render(
    gpu: &Gpu,
    source: &mut dyn Source,
    size: (u32, u32),
    clock: Duration,
    frame: u32,
) -> Vec<u8> {
    let target = gpu.texture(
        "preview",
        size,
        gpu.format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        None,
    );
    let view = target.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    source.update(gpu, clock);
    source.render(
        gpu,
        Frame {
            screen: 0,
            size,
            mouse: [size.0 as f32 / 2.0, size.1 as f32 / 2.0],
            clock,
            frame,
            view: &view,
            encoder: &mut encoder,
        },
    );
    let row = (size.0 * 4).next_multiple_of(256);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview readback"),
        size: u64::from(row * size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let mapped = buffer.get_mapped_range(..).expect("mapped readback");
    mapped
        .chunks(row as usize)
        .flat_map(|r| &r[..(size.0 * 4) as usize])
        .copied()
        .collect()
}
