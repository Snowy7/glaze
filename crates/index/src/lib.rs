//! File-backed name search. The shell uses only the bounded named-pipe client; scanning and
//! watches live in the opt-in `glaze-index` process. See `docs/index.md`.

mod backing;
mod budget;
mod names;
pub mod pipe;
pub mod query;
mod quiet;
mod residency;
pub mod rules;
pub mod scan;
pub mod service;
mod spool;
mod startup;
pub mod store;
mod volumes;
pub mod watch;

use serde::{Deserialize, Serialize};

pub const PIPE: &str = r"\\.\pipe\glaze-index";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Request {
    Search { query: String, limit: usize },
    Sizes { path: String, limit: usize },
    Status,
    Rescan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub size: u64,
    pub files: u32,
    /// Unix seconds, saturated at the representable range.
    pub modified: u32,
    /// Logical bytes and file count are lower bounds when an excluded/inaccessible tree exists.
    #[serde(default)]
    pub size_unknown: bool,
    #[serde(default)]
    pub excluded: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub hits: Vec<Hit>,
    pub entries: usize,
    pub scanning: bool,
    pub elapsed_us: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
