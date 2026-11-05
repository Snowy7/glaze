//! The `native` capability (docs/plugins.md › "Native modules"): a plugin's own native modules
//! run in `mypaper-native-host.exe`, supervised by `mypaper_native_host::Supervisor`. The
//! supervisor blocks, so it lives on its own lane: one thread runs activate, deactivate and
//! calls in order and posts replies to the host loop as `event::native`, and another blocks on
//! its event queue and posts `event::native-event` (including `native-module-restarted`).
//! Nothing starts until a plugin that declares `native` modules loads.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use mypaper_native_host::{HostConfig, Isolation, PluginSpec, Supervisor};

use crate::host::Input;

pub enum Job {
    /// Reaches this barrier only after queued native calls and deactivation have finished.
    Removed(String),
    Activate { plugin: String, folder: PathBuf, native: Vec<String>, permissions: Vec<String> },
    Deactivate(String),
    Call { plugin: String, id: u64, module: String, method: String, bytes: Vec<u8> },
}

pub struct Lane {
    jobs: Sender<Job>,
}

impl Lane {
    /// Starts the lane threads; the supervisor starts a host process only on `Activate`.
    pub fn start(tx: Sender<Input>) -> std::io::Result<Self> {
        // The sibling exe; `MYPAPER_NATIVE_HOST` points the tests at a freshly built one.
        let exe = match std::env::var_os("MYPAPER_NATIVE_HOST") {
            Some(path) => PathBuf::from(path),
            None => std::env::current_exe()?.with_file_name("mypaper-native-host.exe"),
        };
        let supervisor = Arc::new(Supervisor::new(exe, HostConfig::default()));
        let (events, posts) = (Arc::clone(&supervisor), tx.clone());
        std::thread::Builder::new().name("native events".into()).stack_size(128 << 10).spawn(move || {
            loop {
                // Blocks on the supervisor's condvar; the timeout only bounds the wait.
                if let Some(event) = events.recv_event_timeout(Duration::from_secs(3600))
                    && posts.send(Input::NativeEvent(event)).is_err()
                {
                    break;
                }
            }
        })?;
        let (jobs, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new().name("native".into()).stack_size(256 << 10).spawn(move || {
            for job in rx {
                match job {
                    Job::Removed(plugin) => { if tx.send(Input::Removed(plugin)).is_err() { break; } }
                    Job::Activate { plugin, folder, native, permissions } => {
                        let spec = PluginSpec { id: plugin.clone(), folder, native, permissions, isolation: Isolation::Shared };
                        if let Err(e) = supervisor.activate(spec) {
                            log::warn!("plugin {plugin}: native modules didn't start: {e}");
                        }
                    }
                    Job::Deactivate(plugin) => {
                        if let Err(e) = supervisor.deactivate(&plugin) {
                            log::warn!("plugin {plugin}: stopping native modules: {e}");
                        }
                    }
                    Job::Call { plugin, id, module, method, bytes } => {
                        let result = supervisor.call(&plugin, &module, &method, &bytes).map_err(|e| e.to_string());
                        if tx.send(Input::Native { plugin, id, result }).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;
        Ok(Self { jobs })
    }

    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}
