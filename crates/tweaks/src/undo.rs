//! The undo journal: before the first change to any value, its original
//! (including "absent") is written to disk, so every change can be undone,
//! also after a crash. Storage is behind [`Store`] so the logic is tested
//! without touching the real registry.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// A registry value we may write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Value {
    Dword(u32),
    Binary(Vec<u8>),
}

/// Where values live. `key` is a path under `HKEY_CURRENT_USER`.
pub trait Store {
    /// `None` if the value does not exist.
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>>;
    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()>;
    /// Removes the value; removing an absent value is not an error.
    fn delete(&mut self, key: &str, name: &str) -> Result<()>;
}

/// One value to set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub key: &'static str,
    pub name: &'static str,
    pub value: Value,
}

/// What a value was before we first touched it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    key: String,
    name: String,
    /// `None`: the value did not exist.
    original: Option<Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}

impl Journal {
    fn has(&self, key: &str, name: &str) -> bool {
        self.entries.iter().any(|e| e.key == key && e.name == name)
    }
}

/// Applies writes to a [`Store`] and undoes them from the journal file.
pub struct Undo<S> {
    store: S,
    path: PathBuf,
    /// How long to let the system react between restore passes.
    settle: Duration,
}

/// Restore passes after the first, at most, while the system keeps rewriting values.
const SETTLE_ROUNDS: usize = 5;

impl<S: Store> Undo<S> {
    pub fn new(store: S, path: PathBuf, settle: Duration) -> Self {
        Self {
            store,
            path,
            settle,
        }
    }

    /// Does a journal exist, i.e. is there something to undo (after a crash)?
    pub fn pending(&self) -> bool {
        self.path.exists()
    }

    /// Did the journal record this value, i.e. did the run it belongs to write it?
    pub fn journaled(&self, key: &str, name: &str) -> bool {
        self.load()
            .ok()
            .flatten()
            .is_some_and(|journal| journal.has(key, name))
    }

    /// Writes `writes`, journaling each value's original first. Originals
    /// are recorded once: applying again (e.g. a new palette) keeps the
    /// values from before the first change. The journal exists after this
    /// call even if `writes` is empty. Returns whether any value changed.
    pub fn apply(&mut self, writes: &[Write]) -> Result<bool> {
        let mut journal = self.load()?.unwrap_or_default();
        let mut grown = !self.path.exists();
        for w in writes {
            if !journal.has(w.key, w.name) {
                journal.entries.push(Entry {
                    key: w.key.into(),
                    name: w.name.into(),
                    original: self.store.read(w.key, w.name)?,
                });
                grown = true;
            }
        }
        if grown {
            self.save(&journal)?;
        }

        let mut changed = false;
        for w in writes {
            if self.store.read(w.key, w.name)?.as_ref() != Some(&w.value) {
                self.store.write(w.key, w.name, &w.value)?;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Puts every journaled value back (deleting those that were absent),
    /// then deletes the journal. If some value cannot be restored the
    /// journal stays, so the next start retries. Returns whether any value
    /// changed.
    ///
    /// `announce` runs once after the values are back and something changed
    /// (core broadcasts `WM_SETTINGCHANGE` there). Windows reacts to our
    /// writes and to that broadcast by rewriting related values (the DWM
    /// accent follows the menu accent), so afterwards the journal is
    /// re-checked until a pass finds nothing to fix: the registry ends up
    /// exactly as it was.
    pub fn restore(&mut self, announce: impl FnOnce()) -> Result<bool> {
        let Some(journal) = self.load()? else {
            return Ok(false);
        };
        let changed = self.put_back_all(&journal)?;
        if changed {
            announce();
            for _ in 0..SETTLE_ROUNDS {
                std::thread::sleep(self.settle);
                if !self.put_back_all(&journal)? {
                    break;
                }
            }
        }
        std::fs::remove_file(&self.path).context("deleting the undo journal")?;
        Ok(changed)
    }

    /// One pass over the journal, newest entry first. Returns whether any
    /// value had to be written.
    fn put_back_all(&mut self, journal: &Journal) -> Result<bool> {
        let mut changed = false;
        let mut failures = Vec::new();
        for entry in journal.entries.iter().rev() {
            match self.put_back(entry) {
                Ok(did_change) => changed |= did_change,
                Err(e) => failures.push(format!("{}\\{}: {e:#}", entry.key, entry.name)),
            }
        }
        if !failures.is_empty() {
            bail!("could not restore:\n  {}", failures.join("\n  "));
        }
        Ok(changed)
    }

    fn put_back(&mut self, entry: &Entry) -> Result<bool> {
        let (key, name) = (entry.key.as_str(), entry.name.as_str());
        if self.store.read(key, name)? == entry.original {
            return Ok(false);
        }
        match &entry.original {
            Some(value) => self.store.write(key, name, value)?,
            None => self.store.delete(key, name)?,
        }
        Ok(true)
    }

    /// `None` if there is no journal. An unreadable one is set aside as
    /// `undo.json.bad` rather than blocking every future start.
    fn load(&self) -> Result<Option<Journal>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("reading the undo journal"),
        };
        match serde_json::from_str(&text) {
            Ok(journal) => Ok(Some(journal)),
            Err(e) => {
                let bad = self.path.with_extension("json.bad");
                let _ = std::fs::rename(&self.path, &bad);
                bail!("undo journal is corrupt ({e}); moved to {}", bad.display())
            }
        }
    }

    /// Atomic: temp file + rename.
    fn save(&self, journal: &Journal) -> Result<()> {
        let dir = self.path.parent().context("journal path has no parent")?;
        std::fs::create_dir_all(dir)?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_string_pretty(journal)? + "\n")?;
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::*;

    /// An in-memory registry. Clones share the data, so a test can play
    /// "Windows" (rewriting values behind the journal's back) while `Undo`
    /// owns another handle.
    #[derive(Default, Clone)]
    struct Memory(Rc<RefCell<HashMap<(String, String), Value>>>);

    impl Store for Memory {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self.0.borrow().get(&(key.into(), name.into())).cloned())
        }
        fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
            self.0
                .borrow_mut()
                .insert((key.into(), name.into()), value.clone());
            Ok(())
        }
        fn delete(&mut self, key: &str, name: &str) -> Result<()> {
            self.0.borrow_mut().remove(&(key.into(), name.into()));
            Ok(())
        }
    }

    fn put(key: &'static str, name: &'static str, value: Value) -> Write {
        Write { key, name, value }
    }

    /// A journal path in a fresh temp folder, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mypaper-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
        fn journal(&self) -> PathBuf {
            self.0.join("undo.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn restore_brings_back_changed_and_absent_values_and_deletes_the_journal() {
        let scratch = Scratch::new("undo-roundtrip");
        let mut store = Memory::default();
        store.write("K", "existing", &Value::Dword(1)).unwrap();
        let mut undo = Undo::new(store, scratch.journal(), Duration::ZERO);

        let changed = undo
            .apply(&[
                put("K", "existing", Value::Dword(2)),
                put("K", "fresh", Value::Binary(vec![9, 9])),
            ])
            .unwrap();
        assert!(changed && undo.pending());
        assert_eq!(
            undo.store.read("K", "existing").unwrap(),
            Some(Value::Dword(2))
        );

        assert!(undo.restore(|| {}).unwrap());
        assert_eq!(
            undo.store.read("K", "existing").unwrap(),
            Some(Value::Dword(1))
        );
        assert_eq!(undo.store.read("K", "fresh").unwrap(), None);
        assert!(!undo.pending());
    }

    #[test]
    fn reapplying_keeps_the_first_originals_and_reports_no_change_when_equal() {
        let scratch = Scratch::new("undo-reapply");
        let mut store = Memory::default();
        store.write("K", "a", &Value::Dword(1)).unwrap();
        let mut undo = Undo::new(store, scratch.journal(), Duration::ZERO);

        undo.apply(&[put("K", "a", Value::Dword(2))]).unwrap();
        // A later palette changes the color again: the journal must still hold 1.
        assert!(undo.apply(&[put("K", "a", Value::Dword(3))]).unwrap());
        assert!(!undo.apply(&[put("K", "a", Value::Dword(3))]).unwrap());

        undo.restore(|| {}).unwrap();
        assert_eq!(undo.store.read("K", "a").unwrap(), Some(Value::Dword(1)));
    }

    #[test]
    fn restore_fixes_values_the_system_rewrites_in_reaction() {
        let scratch = Scratch::new("undo-settle");
        let mut windows = Memory::default();
        windows.write("DWM", "accent", &Value::Dword(1)).unwrap();
        windows.write("Accent", "menu", &Value::Dword(1)).unwrap();
        let mut undo = Undo::new(windows.clone(), scratch.journal(), Duration::ZERO);
        undo.apply(&[
            put("DWM", "accent", Value::Dword(2)),
            put("Accent", "menu", Value::Dword(2)),
        ])
        .unwrap();

        // Windows reacts to the announcement by syncing the DWM accent to the menu accent.
        let mut announced = 0;
        let mut react = windows.clone();
        undo.restore(|| {
            announced += 1;
            react.write("DWM", "accent", &Value::Dword(7)).unwrap();
        })
        .unwrap();

        assert_eq!(announced, 1);
        assert_eq!(
            windows.read("DWM", "accent").unwrap(),
            Some(Value::Dword(1))
        );
        assert_eq!(
            windows.read("Accent", "menu").unwrap(),
            Some(Value::Dword(1))
        );
    }

    #[test]
    fn a_journal_survives_a_restart_and_an_empty_apply_still_arms_it() {
        let scratch = Scratch::new("undo-crash");
        let mut store = Memory::default();
        store.write("K", "a", &Value::Dword(1)).unwrap();
        let mut first = Undo::new(store, scratch.journal(), Duration::ZERO);
        first.apply(&[put("K", "a", Value::Dword(2))]).unwrap();

        // "Crash": a new Undo over the same registry state and journal file.
        let mut second = Undo::new(first.store, scratch.journal(), Duration::ZERO);
        assert!(second.pending());
        assert!(second.journaled("K", "a") && !second.journaled("K", "other"));
        second.restore(|| {}).unwrap();
        assert_eq!(second.store.read("K", "a").unwrap(), Some(Value::Dword(1)));

        assert!(!second.apply(&[]).unwrap());
        assert!(second.pending());
        assert!(!second.restore(|| {}).unwrap());
        assert!(!second.pending());
    }
}
