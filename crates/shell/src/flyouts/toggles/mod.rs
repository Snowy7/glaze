//! Quick toggles Windows has no public setter for, switched the way Windows' own Settings and
//! quick settings do it, from this process (no injection). Each mechanism is undocumented and
//! lives in its own module; each `set` reads the state back and fails when Windows kept the
//! old one, so the tile always shows the real state.
//!
//! Verified on Windows 11 26H2 (build 26300) against Settings, 2026-11-05:
//!
//! | Toggle | Mechanism | Result |
//! |---|---|---|
//! | Do not disturb | `CLSID_QuietHoursSettings` user-selected profile | works |
//! | Night light | CloudStore `bluelightreductionstate` blob | works |
//! | Airplane mode | `IRadioManager::SetSystemRadioState` | works; shown only with a radio, as Windows does |
//! | Energy saver | power service (`NtPowerInformation`, no public level) | not switchable; not shown |
//! | Focus | `FocusSessionManager`, a Limited Access Feature | access denied; not shown |
//!
//! The WNF route for do not disturb (`WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED`) is accepted
//! but ignored on this build: Settings kept do not disturb off.

pub mod airplane;
pub mod dnd;
pub mod night_light;

/// A toggle switched through one of these mechanisms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Dnd,
    NightLight,
    Airplane,
}

impl Toggle {
    /// Switches it and reads it back (on the quick lane: COM and the registry).
    pub fn set(self, on: bool) -> Result<(), String> {
        match self {
            Self::Dnd => dnd::set(on),
            Self::NightLight => night_light::set(on),
            Self::Airplane => airplane::set(on),
        }
    }
}

/// `set` then read back: an error when Windows did not take `on` (logged; the caller shows the
/// state it reads next).
fn confirm(name: &str, on: bool, read: Option<bool>) -> Result<(), String> {
    match read {
        Some(now) if now == on => Ok(()),
        now => {
            log::warn!("quick settings: {name} asked {on}, Windows reads back {now:?}");
            Err(format!("Windows kept {name} {}", if on { "off" } else { "on" }))
        }
    }
}
