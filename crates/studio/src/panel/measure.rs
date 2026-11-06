//! Read-only measurements of this Studio process, never another window or desktop component.
#[derive(Debug, Clone, Copy)]
pub struct Memory {
    pub working_mb: f64,
    pub private_mb: f64,
}

pub fn memory(fixture: bool) -> Option<Memory> {
    if fixture {
        return Some(Memory {
            working_mb: 32.0,
            private_mb: 24.0,
        });
    }
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the current-process pseudo handle needs no close; the writable buffer has the declared size.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb) }
        .ok()?;
    Some(Memory {
        working_mb: counters.WorkingSetSize as f64 / 1048576.0,
        private_mb: counters.PrivateUsage as f64 / 1048576.0,
    })
}

/// The selected display's current refresh rate, read from Windows without changing its mode.
pub fn refresh_rate(app: &crate::app::App) -> Option<u32> {
    if app.fixture {
        return Some(144);
    }
    use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
    let monitor = app.display()?;
    let device = windows::core::HSTRING::from(&monitor.device);
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: a read-only query with a correctly sized writable DEVMODEW.
    unsafe { EnumDisplaySettingsW(&device, ENUM_CURRENT_SETTINGS, &mut mode) }
        .as_bool()
        .then_some(mode.dmDisplayFrequency)
        .filter(|hz| (1..=360).contains(hz))
}
