//! N2 entry point; N3 replaces these Windows panels with native shell flyouts.
use mypaper_bars::{system, tray};
use mypaper_desktop::Monitor;
use mypaper_ui::Rect;
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_LWIN, VK_SPACE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Calendar,
    Quick,
    Wifi,
    Mixer,
    Notifications,
    Launcher,
    Power,
    TrayOverflow,
    Language,
}

/// `anchor_rect` is in screen pixels; `monitor` identifies the owning display.
pub fn open(kind: Kind, _anchor_rect: Rect, _monitor: &Monitor) {
    match kind {
        Kind::Calendar | Kind::Notifications => system::keys(&[VK_LWIN, VIRTUAL_KEY(b'N' as u16)]),
        Kind::Quick => system::open("ms-actioncenter:controlcenter/".into()),
        Kind::Wifi => system::open("ms-availablenetworks:".into()),
        Kind::Mixer => system::open("ms-settings:apps-volume".into()),
        Kind::Launcher => system::start_menu(),
        Kind::Power => system::keys(&[VK_LWIN, VIRTUAL_KEY(b'X' as u16)]),
        Kind::Language => system::keys(&[VK_LWIN, VK_SPACE]),
        Kind::TrayOverflow => tray::open_overflow(),
    }
}
