//! mypaper-shell as a library. The `mypaper-shell` binary (`main.rs`) runs the shell from these
//! modules; Studio links the same code to draw its previews with the shell's own views
//! ([`preview`], plans/native/studio-redesign.md D3), so a preview can never drift from the
//! desktop.

pub mod apps;
pub mod bars;
pub mod desktop_menu;
pub mod flyouts;
pub mod glass;
pub mod inbox;
pub mod integrations;
pub mod look;
pub mod overlay_client;
pub mod preview;
pub mod widgets;
