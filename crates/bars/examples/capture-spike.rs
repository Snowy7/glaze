//! Spike (docs/bars.md, "Liquid glass"): can a bar read the pixels behind its own window? Shows a
//! red, never-activated topmost window on the secondary monitor, then BitBlts its area from the
//! screen twice: as is (red), and with the window excluded from capture
//! (`WDA_EXCLUDEFROMCAPTURE`), which should give what's behind it. Prints the average color and
//! the time each took.
//!
//! ```text
//! cargo run -p mypaper-bars --example capture-spike
//! ```

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // SAFETY: default handling of a message we were called with.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// Average RGB of the screen rect, and how long the copy took.
fn grab(x: i32, y: i32, width: i32, height: i32) -> ([u8; 3], Duration) {
    let start = Instant::now();
    // SAFETY: GDI objects made, selected and released here; `bits` is read before the DIB goes.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let old = SelectObject(memory, bitmap.into());
        BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        )
        .unwrap();
        let elapsed = start.elapsed();
        let pixels = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
        let mut sum = [0u64; 3];
        for px in pixels.as_chunks::<4>().0 {
            sum[0] += u64::from(px[2]);
            sum[1] += u64::from(px[1]);
            sum[2] += u64::from(px[0]);
        }
        let n = (width * height) as u64;
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        (sum.map(|s| (s / n) as u8), elapsed)
    }
}

fn main() -> windows::core::Result<()> {
    // SAFETY: a spike: plain Win32 on our own window, on this thread.
    unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )?;
        let instance = GetModuleHandleW(None)?.into();
        let class = w!("mypaper.capture.spike");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class,
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_00ff)),
            ..Default::default()
        });
        let (x, y, width, height) = (-2560 + 900, 9 + 500, 420, 300);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            w!("capture spike"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let settle = Instant::now() + Duration::from_millis(600);
        let mut msg = MSG::default();
        while Instant::now() < settle {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let (shown, t1) = grab(x + 20, y + 20, width - 40, height - 40);
        println!("as is:    rgb{shown:?} in {t1:?}");
        let start = Instant::now();
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
        let _ = DwmFlush();
        let set = start.elapsed();
        let (behind, t2) = grab(x + 20, y + 20, width - 40, height - 40);
        SetWindowDisplayAffinity(hwnd, WDA_NONE)?;
        println!("excluded: rgb{behind:?} in {t2:?} (+{set:?} to exclude and flush)");
        let (again, t3) = grab(x + 20, y + 20, width - 40, height - 40);
        println!("restored: rgb{again:?} in {t3:?}");
        // Without a flush: does the exclusion apply at once?
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
        let (unflushed, t4) = grab(x + 20, y + 20, width - 40, height - 40);
        SetWindowDisplayAffinity(hwnd, WDA_NONE)?;
        println!("excluded, no flush: rgb{unflushed:?} in {t4:?}");
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}
