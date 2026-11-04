//! The schema half of `crates/bars`, built for Studio.
//!
//! `mypaper-bars` is one package for the bars host (a binary that needs WebView2 and a pile of
//! `windows` features) and the library core uses. Cargo compiles a package's dependencies for
//! both, so a path dependency on it would pull the whole host into Studio. Until the schema modules
//! are split into their own crate, this shim includes the very same source files, so Studio parses
//! `bars.json` with the host's own types and can never drift from them. `taskbar.rs` (Win32) is
//! the one module of the library it leaves out.
//!
//! If `crates/bars/src/lib.rs` gains a module that `config.rs` needs, add its `#[path]` line
//! here; the build fails until you do.

#[path = "../../../../crates/bars/src/config.rs"]
pub mod config;
#[path = "../../../../crates/bars/src/files.rs"]
pub mod files;
#[path = "../../../../crates/bars/src/flyouts/config.rs"]
pub mod flyout_config;
#[path = "../../../../crates/bars/src/icon_pack.rs"]
pub mod icon_pack;
#[path = "../../../../crates/bars/src/presets.rs"]
pub mod presets;
#[path = "../../../../crates/bars/src/style.rs"]
pub mod style;
#[path = "../../../../crates/bars/src/vars.rs"]
pub mod vars;

pub use config::{BarsConfig, DockStyle, Module, ModuleRef, TaskbarMode, TopLayout};
pub use presets::Preset;
