//! Windows services for the native shell, core and Studio. No binary or browser.

pub mod accent;
pub mod actions;
pub mod appbar;
pub mod apps;
pub mod audio;
pub mod clock;
pub mod config;
pub mod custom;
pub mod desktops;
pub mod events;
pub mod files;
#[path = "flyouts/config.rs"]
pub mod flyout_config;
pub mod flyouts;
pub mod glyphs;
pub mod hooks;
pub mod icon_pack;
pub mod icons;
pub mod media;
pub mod network;
pub mod presets;
pub mod preview;
pub mod style;
pub mod system;
pub mod taskbar;
pub mod trash;
pub mod tray;
pub mod tray_data;
pub mod tray_host;
pub mod tray_menu;
pub mod vars;
pub mod wake;
pub use config::{BarsConfig, Module, ModuleRef, TaskbarMode};
pub use taskbar::replay;
