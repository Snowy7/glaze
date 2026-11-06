//! One supervised child process: spawn, restart policy and its wait handle.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::WaitForSingleObject;

use crate::restart::{Decision, Restarts};

/// What the supervisor should do after a child exited.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Scheduled for a restart; nothing else to do.
    Restarting,
    /// The child is done for this session (clean exit, broken config, or
    /// crashing too often).
    Stopped,
}

pub struct Supervised {
    pub name: &'static str,
    exe: PathBuf,
    args: Vec<PathBuf>,
    child: Option<Child>,
    restarts: Restarts,
    restart_at: Option<Instant>,
}

impl Supervised {
    pub fn new(name: &'static str, exe: PathBuf, args: Vec<PathBuf>) -> Self {
        Self {
            name,
            exe,
            args,
            child: None,
            restarts: Restarts::default(),
            restart_at: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        let child = Command::new(&self.exe)
            .args(&self.args)
            // Optional console-based services must not allocate a visible console at boot.
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .with_context(|| format!("starting the {}", self.name))?;
        self.child = Some(child);
        Ok(())
    }

    /// The handle to wait on, while the process is alive.
    pub fn handle(&self) -> Option<HANDLE> {
        self.child.as_ref().map(|c| HANDLE(c.as_raw_handle()))
    }

    pub fn restart_at(&self) -> Option<Instant> {
        self.restart_at
    }

    /// Reaps the exited child and applies the restart policy.
    pub fn on_exit(&mut self) -> Result<Outcome> {
        let mut exited = self.child.take().context("child vanished")?;
        let code = exited.wait()?.code();
        let name = self.name;
        Ok(match self.restarts.on_exit(code, Instant::now()) {
            Decision::RestartAfter(delay) => {
                // Shell recovery has no intentional delay; crash-loop limiting still applies.
                // Other children retain their backoff.
                let delay = if self.name == "shell" {
                    Duration::ZERO
                } else {
                    delay
                };
                log::warn!("{name} exited ({code:?}); restarting in {delay:?}");
                self.restart_at = Some(Instant::now() + delay);
                Outcome::Restarting
            }
            Decision::Stop => {
                log::info!("{name} exited cleanly");
                Outcome::Stopped
            }
            Decision::Broken => {
                log::error!("{name} cannot start with this configuration (see error above)");
                Outcome::Stopped
            }
            Decision::GiveUp => {
                log::error!("{name} keeps crashing; giving up");
                Outcome::Stopped
            }
        })
    }

    /// Starts the child if its restart delay has passed.
    pub fn restart_if_due(&mut self) -> Result<()> {
        if self.restart_at.is_some_and(|t| Instant::now() >= t) {
            self.restart_at = None;
            self.start()?;
        }
        Ok(())
    }

    /// Kills the child (if running) and schedules a fresh start.
    pub fn restart_after(&mut self, delay: Duration) {
        self.stop();
        self.restart_at = Some(Instant::now() + delay);
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Asks the child to exit with `ask` (so it can clean up), waits up to `grace`, and kills it
    /// if it's still running then. Returns whether it exited on its own.
    pub fn stop_gracefully(&mut self, ask: impl FnOnce() -> bool, grace: Duration) -> bool {
        self.restart_at = None;
        let Some(mut child) = self.child.take() else {
            return true;
        };
        let asked = ask();
        let handle = HANDLE(child.as_raw_handle());
        let millis = grace.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: the child's process handle stays valid while `child` is alive.
        let exited = asked && unsafe { WaitForSingleObject(handle, millis) } == WAIT_OBJECT_0;
        if !exited {
            log::warn!("the {} didn't exit in time; killing it", self.name);
            let _ = child.kill();
        }
        let _ = child.wait();
        exited
    }
}
