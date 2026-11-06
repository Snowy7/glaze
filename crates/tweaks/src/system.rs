//! Reversible, opt-in Windows preferences, independent of theme styling.
//! The existing undo engine persists exact registry bytes and power API originals
//! in `undo-system.json` before any write. No background work lives here.

mod power;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use mypaper_config::SystemConfig;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Registry::REG_DWORD;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::w;

use crate::registry::Registry;
use crate::undo::{Store, Undo, Value, Write};

pub const EXPLORER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
pub const GAME_BAR_KEY: &str = r"Software\Microsoft\GameBar";
const JOURNAL: &str = "undo-system.json";

/// Pure desired-state plan. Empty preferences make no writes or journal.
pub fn writes(config: &SystemConfig) -> Vec<Write> {
    let mut writes = Vec::new();
    for (key, name, value) in [
        (
            GAME_BAR_KEY,
            "AutoGameModeEnabled",
            config.game_mode.map(u32::from),
        ),
        (
            EXPLORER_KEY,
            "HideFileExt",
            config.show_extensions.map(|on| u32::from(!on)),
        ),
        (
            EXPLORER_KEY,
            "Hidden",
            config.show_hidden.map(|on| if on { 1 } else { 2 }),
        ),
        (
            EXPLORER_KEY,
            "SeparateProcess",
            config.separate_process.map(u32::from),
        ),
    ] {
        if let Some(value) = value {
            writes.push(Write {
                key,
                name,
                value: Value::Raw {
                    kind: REG_DWORD.0,
                    bytes: value.to_le_bytes().to_vec(),
                },
            });
        }
    }
    if let Some(mode) = config.power_mode {
        writes.extend(power::writes(mode));
    }
    writes
}

/// Registry values and documented power-mode APIs share the same undo transaction.
pub struct SystemStore;

impl Store for SystemStore {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
        if key == power::KEY {
            Ok(Some(power::read(name)?))
        } else {
            Registry.read_exact(key, name)
        }
    }
    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
        if key == power::KEY {
            power::write(name, value)
        } else {
            Registry.write(key, name, value)
        }
    }
    fn delete(&mut self, key: &str, name: &str) -> Result<()> {
        if key == power::KEY {
            bail!("power mode always has an original; cannot delete it")
        }
        Registry.delete(key, name)
    }
    fn first_missing(&self, key: &str) -> Result<Option<String>> {
        if key == power::KEY {
            Ok(None)
        } else {
            Registry.first_missing(key)
        }
    }
    fn delete_if_empty(&mut self, key: &str) -> Result<()> {
        if key == power::KEY {
            Ok(())
        } else {
            Registry.delete_if_empty(key)
        }
    }
}

/// Restores on drop/quit/panic and on the next start after an unclean exit.
pub struct SystemTweaks<S: Store = SystemStore> {
    undo: Undo<S>,
    active: Vec<Write>,
    announce: fn(),
}

impl SystemTweaks {
    pub fn new() -> Result<Self> {
        Self::with(
            SystemStore,
            crate::palette::local_dir()?.join(JOURNAL),
            announce,
        )
    }
}

impl<S: Store> SystemTweaks<S> {
    fn with(store: S, journal: PathBuf, announce: fn()) -> Result<Self> {
        let mut tweaks = Self {
            undo: Undo::new(store, journal, Duration::from_millis(100)),
            active: Vec::new(),
            announce,
        };
        tweaks.restore()?;
        Ok(tweaks)
    }

    /// Replacing/removing a field first restores the previous transaction, then
    /// captures fresh originals. Failed applies roll back and remain retryable.
    pub fn apply(&mut self, config: &SystemConfig) -> Result<bool> {
        let desired = writes(config);
        if desired == self.active && !self.undo.pending() {
            return Ok(false);
        }
        if desired == self.active && !desired.is_empty() {
            return Ok(false);
        }
        let restored = self.restore()?;
        if desired.is_empty() {
            return Ok(restored);
        }
        match self.undo.apply(&desired) {
            Ok(changed) => {
                if changed {
                    (self.announce)();
                }
                self.active = desired;
                Ok(changed || restored)
            }
            Err(error) => {
                if let Err(rollback) = self.restore() {
                    log::error!("system preference rollback pending: {rollback:#}");
                }
                Err(error)
            }
        }
    }

    pub fn restore(&mut self) -> Result<bool> {
        self.active.clear();
        self.undo.restore(self.announce)
    }
}

impl<S: Store> Drop for SystemTweaks<S> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            log::error!("system preferences await recovery on next start: {error:#}");
        }
    }
}

fn announce() {
    // SAFETY: the static UTF-16 area outlives the bounded synchronous broadcast.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ShellState").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            100,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::tests::{Memory, Scratch};
    use mypaper_config::PowerMode;

    fn settings() -> SystemConfig {
        SystemConfig {
            game_mode: Some(true),
            show_extensions: Some(true),
            show_hidden: Some(true),
            separate_process: Some(true),
            power_mode: Some(PowerMode::Performance),
            ..SystemConfig::default()
        }
    }

    #[test]
    fn plan_has_only_explicit_settings_and_correct_boolean_polarity() {
        assert!(writes(&SystemConfig::default()).is_empty());
        let on = writes(&settings());
        let off = writes(&SystemConfig {
            game_mode: Some(false),
            show_extensions: Some(false),
            show_hidden: Some(false),
            separate_process: Some(false),
            ..SystemConfig::default()
        });
        for (write, expected) in on.iter().take(4).zip([1u32, 0, 1, 1]) {
            assert_eq!(
                write.value,
                Value::Raw {
                    kind: 4,
                    bytes: expected.to_le_bytes().to_vec()
                }
            );
        }
        for (write, expected) in off.iter().zip([0u32, 1, 2, 0]) {
            assert_eq!(
                write.value,
                Value::Raw {
                    kind: 4,
                    bytes: expected.to_le_bytes().to_vec()
                }
            );
        }
        assert_eq!(on.len(), 6, "power preserves AC and DC independently");
    }

    #[test]
    fn each_setting_restores_original_type_bytes_and_absence() {
        let scratch = Scratch::new("system-each-setting");
        let mut store = Memory::default();
        let original = Value::Raw {
            kind: 7,
            bytes: vec![65, 0, 0, 0, 0, 0, 0, 0],
        };
        for (index, write) in writes(&settings()).iter().enumerate() {
            let path = scratch.0.join(format!("{index}.json"));
            store.write(write.key, write.name, &original).unwrap();
            let mut undo = Undo::new(store.clone(), path, Duration::ZERO);
            assert!(undo.apply(std::slice::from_ref(write)).unwrap());
            assert!(!undo.apply(std::slice::from_ref(write)).unwrap());
            undo.restore(|| {}).unwrap();
            assert_eq!(
                store.read(write.key, write.name).unwrap(),
                Some(original.clone())
            );
            store.delete(write.key, write.name).unwrap();
            undo.apply(std::slice::from_ref(write)).unwrap();
            undo.restore(|| {}).unwrap();
            assert_eq!(store.read(write.key, write.name).unwrap(), None);
        }
    }

    #[test]
    fn removing_one_field_restores_it_and_drop_restores_the_rest() {
        let scratch = Scratch::new("system-removal");
        let store = Memory::default();
        let mut tweaks = SystemTweaks::with(store.clone(), scratch.journal(), || {}).unwrap();
        tweaks.apply(&settings()).unwrap();
        let config = SystemConfig {
            show_hidden: Some(false),
            ..SystemConfig::default()
        };
        tweaks.apply(&config).unwrap();
        assert_eq!(
            store.read(GAME_BAR_KEY, "AutoGameModeEnabled").unwrap(),
            None
        );
        assert_eq!(store.read(EXPLORER_KEY, "HideFileExt").unwrap(), None);
        assert!(store.read(EXPLORER_KEY, "Hidden").unwrap().is_some());
        assert!(!tweaks.apply(&config).unwrap());
        drop(tweaks);
        assert_eq!(store.read(EXPLORER_KEY, "Hidden").unwrap(), None);
        assert!(!scratch.journal().exists());
    }

    #[test]
    fn restart_recovers_before_accepting_any_new_setting() {
        let scratch = Scratch::new("system-restart");
        let store = Memory::default();
        let mut crashed = Undo::new(store.clone(), scratch.journal(), Duration::ZERO);
        crashed.apply(&writes(&settings())).unwrap();
        let _next = SystemTweaks::with(store.clone(), scratch.journal(), || {}).unwrap();
        for write in writes(&settings()) {
            assert_eq!(store.read(write.key, write.name).unwrap(), None);
        }
    }

    #[test]
    fn a_partial_write_failure_rolls_back_before_retry() {
        use std::cell::Cell;
        use std::rc::Rc;
        struct FailOnce {
            store: Memory,
            fail: Rc<Cell<bool>>,
        }
        impl Store for FailOnce {
            fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
                self.store.read(key, name)
            }
            fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
                if name == "Hidden" && self.fail.replace(false) {
                    bail!("injected write failure")
                }
                self.store.write(key, name, value)
            }
            fn delete(&mut self, key: &str, name: &str) -> Result<()> {
                self.store.delete(key, name)
            }
            fn first_missing(&self, key: &str) -> Result<Option<String>> {
                self.store.first_missing(key)
            }
            fn delete_if_empty(&mut self, key: &str) -> Result<()> {
                self.store.delete_if_empty(key)
            }
        }
        let scratch = Scratch::new("system-failure");
        let store = Memory::default();
        let fail = Rc::new(Cell::new(true));
        let mut tweaks = SystemTweaks::with(
            FailOnce {
                store: store.clone(),
                fail,
            },
            scratch.journal(),
            || {},
        )
        .unwrap();
        let config = SystemConfig {
            show_extensions: Some(true),
            show_hidden: Some(true),
            ..Default::default()
        };
        assert!(tweaks.apply(&config).is_err());
        assert!(!scratch.journal().exists());
        assert_eq!(store.read(EXPLORER_KEY, "HideFileExt").unwrap(), None);
        assert_eq!(store.read(EXPLORER_KEY, "Hidden").unwrap(), None);
        assert!(tweaks.apply(&config).unwrap());
    }
}
