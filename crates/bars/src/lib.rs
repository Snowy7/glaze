//! The shell bars' shared half (docs/bars.md): `bars.json` and the native taskbar with its undo
//! journal. The `mypaper-bars` binary uses it, and so does core, which edits `bars.json` from the
//! tray and replays the journal when the bars didn't exit cleanly.

pub mod config;
pub mod files;
pub mod taskbar;

pub use config::{BarsConfig, Module, TaskbarMode};
pub use taskbar::replay;
