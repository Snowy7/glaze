//! The memory floor of a wgpu DX12 program on this machine: a device and one
//! swapchain, one cleared frame, then idle. Measure it from outside
//! (`WorkingSet64`, `PrivateMemorySize64`) to see what part of the engine's
//! footprint is the GPU stack and not ours.
//!
//! ```text
//! cargo run --release -p glaze-wallpaper --example bare_wgpu -- [auto|fxc] [SECONDS] [render] [trim]
//! ```
//!
//! `render` presents 60 frames a second instead of idling; `trim` empties the
//! working set once, 3 s in.
//!
//! `auto` is wgpu's default shader compiler choice (a `dxcompiler.dll` found
//! on `PATH`, else FXC); `fxc` is the in-box compiler. The window is created
//! hidden and never shown, so nothing takes focus.

use std::num::NonZeroIsize;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_POPUP,
};
use windows::core::w;

const SIZE: (u32, u32) = (2560, 1440);

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards the window's own message, unchanged.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let compiler = match args.next().as_deref() {
        None | Some("auto") => wgpu::Dx12Compiler::Auto,
        Some("fxc") => wgpu::Dx12Compiler::Fxc,
        Some(other) => bail!("unknown compiler {other:?}: use auto or fxc"),
    };
    let seconds: u64 = args.next().map_or(Ok(12), |s| s.parse())?;
    let flags: Vec<String> = args.collect();
    let (render, trim) = (
        flags.iter().any(|f| f == "render"),
        flags.iter().any(|f| f == "trim"),
    );

    // SAFETY: plain Win32 calls; the class and window live as long as the process.
    let (hwnd, hinstance) = unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: w!("glaze-bare"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("glaze-bare"),
            w!("bare"),
            WS_POPUP,
            0,
            0,
            SIZE.0 as i32,
            SIZE.1 as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        (hwnd, hinstance)
    };

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    desc.backend_options.dx12.shader_compiler = compiler;
    let instance = wgpu::Instance::new(desc);

    let mut handle = Win32WindowHandle::new(NonZeroIsize::new(hwnd.0 as isize).context("no hwnd")?);
    handle.hinstance = NonZeroIsize::new(hinstance.0 as isize);
    // SAFETY: the window outlives the surface (both live until the process exits).
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(WindowsDisplayHandle::new().into()),
            raw_window_handle: handle.into(),
        })?
    };
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))?;

    let mut config = surface
        .get_default_config(&adapter, SIZE.0, SIZE.1)
        .context("surface not supported")?;
    config.present_mode = wgpu::PresentMode::Mailbox;
    config.desired_maximum_frame_latency = 1;
    surface.configure(&device, &config);

    // Clears and presents the swapchain once.
    let draw_frame = || {
        if let wgpu::CurrentSurfaceTexture::Success(frame) = surface.get_current_texture() {
            let view = frame.texture.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
            queue.submit([encoder.finish()]);
            queue.present(frame);
        }
    };

    // One frame, so the swapchain has really been used; then idle, or (with
    // `render`) 60 frames a second. With `trim`, the working set is emptied
    // once, 3 s in, to see how fast it grows back.
    draw_frame();
    println!("ready");
    let start = Instant::now();
    let mut trim_at = trim.then(|| start + Duration::from_secs(3));
    // Frame times before the trim and in the 20 frames after it.
    let (mut before, mut after, mut trimmed) = (Vec::new(), Vec::new(), false);
    while start.elapsed() < Duration::from_secs(seconds) {
        if render {
            let frame = Instant::now();
            draw_frame();
            let took = frame.elapsed();
            if !trimmed {
                before.push(took);
            } else if after.len() < 20 {
                after.push(took);
            }
        }
        if trim_at.is_some_and(|t| Instant::now() >= t) {
            trim_at = None;
            trimmed = true;
            // SAFETY: trims our own working set.
            unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX)? };
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    if trimmed && render {
        before.sort();
        println!(
            "frame time: median {:?} before the trim, worst of the 20 frames after it {:?}",
            before[before.len() / 2],
            after.iter().max().copied().unwrap_or_default()
        );
    }
    Ok(())
}
