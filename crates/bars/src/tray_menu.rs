//! Core-to-shell tray menu contract. Core owns commands; the shell owns presentation.
use serde::{Deserialize, Serialize};
use windows::core::{PCWSTR, w};

pub const HOST: PCWSTR = w!("mypaper.shell.tray-menu");
pub const MAGIC: usize = 0x4D50_544D;
pub const CHOSEN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 18;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Separator,
    Action {
        label: String,
        id: u32,
        checked: bool,
        enabled: bool,
    },
    Submenu {
        label: String,
        items: Vec<Item>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub nonce: usize,
    pub point: (i32, i32),
    pub items: Vec<Item>,
}
