//! Native DLLs live only in `mypaper-native-host.exe`, never in this client library.
//! Use the blocking supervisor from the platform's worker lane, not a UI thread.
pub mod protocol;
#[cfg(windows)]
mod server;
#[cfg(windows)]
mod supervisor;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use supervisor::*;

#[cfg(windows)]
#[doc(hidden)]
pub fn run_host(pipe: &str) -> std::io::Result<()> {
    server::run(pipe)
}
