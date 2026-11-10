//! Capture one explicitly supplied, owned test HWND through Windows.Graphics.Capture.
//! No activation, input, movement or monitor capture. Writes a top-down BGRA BMP.
use anyhow::{Context, Result, bail};
use std::time::{Duration, Instant};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows_core::Interface;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let hwnd = HWND(args[1].parse::<usize>()? as *mut std::ffi::c_void);
    let expected: u32 = args[2].parse()?;
    let mut pid = 0;
    // SAFETY: query only; refuse a stale HWND or one belonging to another process.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    anyhow::ensure!(pid == expected, "the supplied HWND is not owned by the expected test process");
    let (mut device, mut context) = (None, None);
    // SAFETY: device creation writes only to initialized output slots.
    unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION,
        Some(&mut device), None, Some(&mut context))?; }
    let device = device.context("no device")?;
    let context = context.context("no context")?;
    let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&device.cast::<IDXGIDevice>()?)? }.cast()?;
    let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, item.Size()?)?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetIsCursorCaptureEnabled(false);
    session.StartCapture()?;
    let start = Instant::now();
    let frame = loop {
        if let Ok(frame) = pool.TryGetNextFrame() { break frame; }
        if start.elapsed() > Duration::from_secs(3) { bail!("no capture frame"); }
        std::thread::sleep(Duration::from_millis(10));
    };
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc); }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging))?; }
    let staging = staging.context("no staging texture")?;
    let mut pixels = Vec::new();
    unsafe {
        context.CopyResource(&staging, &texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        for y in 0..desc.Height as usize {
            pixels.extend_from_slice(std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize), desc.Width as usize * 4));
        }
        context.Unmap(&staging, 0);
    }
    session.Close()?;
    pool.Close()?;
    let mut bmp = Vec::new();
    bmp.extend(b"BM"); bmp.extend((54 + pixels.len() as u32).to_le_bytes());
    bmp.extend([0u8; 4]); bmp.extend(54u32.to_le_bytes()); bmp.extend(40u32.to_le_bytes());
    bmp.extend(desc.Width.to_le_bytes()); bmp.extend((-(desc.Height as i32)).to_le_bytes());
    bmp.extend(1u16.to_le_bytes()); bmp.extend(32u16.to_le_bytes()); bmp.extend([0u8; 24]); bmp.extend(pixels);
    std::fs::write(&args[3], bmp)?;
    println!("captured {}x{} HWND {} PID {}", desc.Width, desc.Height, args[1], expected);
    Ok(())
}
