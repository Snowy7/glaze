//! CLI remains a console application so `--query`, `--sizes` and benchmarks can be piped.
//! Core starts the service with CREATE_NO_WINDOW (no idle console allocation).

fn main() -> anyhow::Result<()> {
    mypaper_index::service::cli(std::env::args().skip(1).collect())
}
