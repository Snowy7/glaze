//! Widget services shared by the native shell and Studio. This crate owns no windows or browser.
pub mod appicon;
pub mod audio;
pub mod bridge;
pub mod display;
pub mod files;
pub mod layout;
pub mod look;
pub mod manifest;
pub mod media;
pub mod net;
pub mod pick;
pub mod plugins;
pub mod registry;
pub mod rpc;
pub mod stats;
pub mod storage;
pub mod userinfo;
pub mod wake;

// Reuse the bundled RSS plugin's pure parser; the native worker supplies WinHTTP I/O.
#[path = "../../../plugins/rss/src/date.rs"]
mod date;
#[path = "../../../plugins/rss/src/feed.rs"]
pub mod feed;
#[path = "../../../plugins/rss/src/text.rs"]
mod text;
