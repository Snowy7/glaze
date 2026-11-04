//! The shell bars' shared half (docs/bars.md): `bars.json` with its presets, look and animation
//! schema, theme variables, icon packs, and the native taskbar with its undo journal. The
//! `mypaper-bars` binary uses it, and so do core (which edits `bars.json` from the tray and
//! replays the journal when the bars didn't exit cleanly) and Studio (presets, variables, packs).

pub mod config;
pub mod files;
/// The flyouts' `bars.json` sections, kept beside the flyouts (the binary's `flyouts` module).
#[path = "flyouts/config.rs"]
pub mod flyout_config;
pub mod icon_pack;
pub mod presets;
pub mod style;
pub mod taskbar;
pub mod vars;

pub use config::{BarsConfig, Module, ModuleRef, TaskbarMode};
pub use taskbar::replay;
