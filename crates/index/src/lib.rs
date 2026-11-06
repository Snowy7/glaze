//! File-backed name search. The shell uses only the bounded named-pipe client; scanning and
//! watches live in the opt-in `mypaper-index` process. See `docs/index.md`.

pub mod pipe;
pub mod query;
pub mod scan;
pub mod service;
pub mod store;
pub mod watch;

use serde::{Deserialize, Serialize};

pub const PIPE: &str = r"\\.\pipe\mypaper-index";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Request {
    Search { query: String, limit: usize },
    Sizes { path: String, limit: usize },
    Status,
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
