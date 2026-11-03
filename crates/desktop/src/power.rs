use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// True while the machine runs on battery. Desktops and unknown states
/// count as plugged in, so they never pause for it.
pub fn on_battery() -> bool {
    const OFFLINE: u8 = 0;
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` outlives the call.
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == OFFLINE
}
