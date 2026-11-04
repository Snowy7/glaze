//! The undo journal: before the first change to any value, its original
//! (including "absent") is written to disk, so every change can be undone,
//! also after a crash. Storage is behind [`Store`] so the logic is tested
//! without touching the real registry.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// A registry value we may write. The empty name is a key's default value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Value {
    Dword(u32),
    Binary(Vec<u8>),
    /// `REG_SZ`.
    String(String),
    /// `REG_EXPAND_SZ`: kept apart from `REG_SZ` so a restore writes the same type back.
    ExpandString(String),
}

/// Where values live. `key` is a path under `HKEY_CURRENT_USER`.
pub trait Store {
    /// `None` if the value does not exist.
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>>;
    /// Writes the value, creating the key (and its parents) if needed.
    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()>;
    /// Removes the value; removing an absent value is not an error.
    fn delete(&mut self, key: &str, name: &str) -> Result<()>;
    /// The topmost ancestor of `key` (or `key` itself) that does not exist
    /// yet, i.e. the key a write to `key` would create first. `None` if `key` exists.
    fn first_missing(&self, key: &str) -> Result<Option<String>>;
    /// Deletes `key` if it has no values and no subkeys. Absent or non-empty: nothing happens.
    fn delete_if_empty(&mut self, key: &str) -> Result<()>;
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
    /// The topmost key that did not exist and was created for this value.
    /// Restoring removes it again (and the keys below it, if empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}

impl Journal {
    fn has(&self, key: &str, name: &str) -> bool {
        self.entries.iter().any(|e| e.key == key && e.name == name)
    }

    /// Did the run write anything under `key`?
    fn touches(&self, key: &str) -> bool {
        self.entries.iter().any(|e| e.key == key)
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

    /// Did the journal record any value under this key?
    pub fn journaled_key(&self, key: &str) -> bool {
        self.load()
            .ok()
            .flatten()
            .is_some_and(|journal| journal.touches(key))
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
                    created: self.store.first_missing(w.key)?,
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
        let changed = self.store.read(key, name)? != entry.original;
        if changed {
            match &entry.original {
                Some(value) => self.store.write(key, name, value)?,
                None => self.store.delete(key, name)?,
            }
        }
        // A key we created goes away with its last value, leaf first.
        if entry.original.is_none()
            && let Some(top) = &entry.created
        {
            let mut path = key;
            loop {
                self.store.delete_if_empty(path)?;
                if path == top {
                    break;
                }
                let Some((parent, _)) = path.rsplit_once('\\') else {
                    break;
                };
                path = parent;
            }
        }
        Ok(changed)
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

/// `pub(crate)`: the cursor and shell-toggle tests reuse the in-memory registry.
#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeSet, HashMap};
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct Data {
        values: HashMap<(String, String), Value>,
        keys: BTreeSet<String>,
    }

    /// An in-memory registry. Clones share the data, so a test can play
    /// "Windows" (rewriting values behind the journal's back) while `Undo`
    /// owns another handle.
    #[derive(Default, Clone)]
    pub(crate) struct Memory(Rc<RefCell<Data>>);

    impl Memory {
        /// Makes `key` and its parents exist, like a key Windows ships.
        pub(crate) fn create_key(&self, key: &str) {
            let mut data = self.0.borrow_mut();
            let mut path = String::new();
            for part in key.split('\\') {
                if !path.is_empty() {
                    path.push('\\');
                }
                path.push_str(part);
                data.keys.insert(path.clone());
            }
        }

        pub(crate) fn has_key(&self, key: &str) -> bool {
            self.0.borrow().keys.contains(key)
        }
    }

    impl Store for Memory {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            Ok(self
                .0
                .borrow()
                .values
                .get(&(key.into(), name.into()))
                .cloned())
        }
        fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
            self.create_key(key);
            self.0
                .borrow_mut()
                .values
                .insert((key.into(), name.into()), value.clone());
            Ok(())
        }
        fn delete(&mut self, key: &str, name: &str) -> Result<()> {
            self.0
                .borrow_mut()
                .values
                .remove(&(key.into(), name.into()));
            Ok(())
        }
        fn first_missing(&self, key: &str) -> Result<Option<String>> {
            let data = self.0.borrow();
            let mut path = String::new();
            for part in key.split('\\') {
                if !path.is_empty() {
                    path.push('\\');
                }
                path.push_str(part);
                if !data.keys.contains(&path) {
                    return Ok(Some(path));
                }
            }
            Ok(None)
        }
        fn delete_if_empty(&mut self, key: &str) -> Result<()> {
            let mut data = self.0.borrow_mut();
            let below = format!("{key}\\");
            let in_use = data
                .values
                .keys()
                .any(|(k, _)| k == key || k.starts_with(&below))
                || data.keys.iter().any(|k| k.starts_with(&below));
            if !in_use {
                data.keys.remove(key);
            }
            Ok(())
        }
    }

    fn put(key: &'static str, name: &'static str, value: Value) -> Write {
        Write { key, name, value }
    }

    /// A journal path in a fresh temp folder, removed on drop.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mypaper-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
        pub(crate) fn journal(&self) -> PathBuf {
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
    fn string_values_keep_their_type_and_default_values_work() {
        let scratch = Scratch::new("undo-strings");
        let mut store = Memory::default();
        store
            .write(
                "C",
                "Arrow",
                &Value::ExpandString(r"%SystemRoot%\a.cur".into()),
            )
            .unwrap();
        store
            .write("C", "", &Value::String("Old scheme".into()))
            .unwrap();
        let mut undo = Undo::new(store, scratch.journal(), Duration::ZERO);

        undo.apply(&[
            put("C", "Arrow", Value::ExpandString(r"D:\new.cur".into())),
            put("C", "", Value::String("mypaper".into())),
        ])
        .unwrap();
        assert!(undo.journaled_key("C") && !undo.journaled_key("D"));

        // The journal is plain JSON on disk; a reload must keep REG_SZ and REG_EXPAND_SZ apart.
        let mut after_restart = Undo::new(undo.store.clone(), scratch.journal(), Duration::ZERO);
        after_restart.restore(|| {}).unwrap();
        assert_eq!(
            after_restart.store.read("C", "Arrow").unwrap(),
            Some(Value::ExpandString(r"%SystemRoot%\a.cur".into()))
        );
        assert_eq!(
            after_restart.store.read("C", "").unwrap(),
            Some(Value::String("Old scheme".into()))
        );
    }

    #[test]
    fn restore_removes_keys_it_created_but_never_ones_that_existed_or_got_other_content() {
        let scratch = Scratch::new("undo-prune");
        let store = Memory::default();
        store.create_key("Software\\Classes\\CLSID");
        let mut undo = Undo::new(store.clone(), scratch.journal(), Duration::ZERO);

        let deep = "Software\\Classes\\CLSID\\{x}\\Inproc";
        undo.apply(&[put(deep, "", Value::String(String::new()))])
            .unwrap();
        assert!(store.has_key("Software\\Classes\\CLSID\\{x}"));
        // Someone else adds a value to the key we created: it is theirs, so it stays.
        store
            .clone()
            .write("Software\\Classes\\CLSID\\{x}", "theirs", &Value::Dword(1))
            .unwrap();

        undo.restore(|| {}).unwrap();
        assert!(!store.has_key(deep), "our own leaf key is gone");
        assert!(
            store.has_key("Software\\Classes\\CLSID\\{x}"),
            "a key with foreign content stays"
        );
        assert!(
            store.has_key("Software\\Classes\\CLSID"),
            "pre-existing keys stay"
        );

        // Without interference the whole created chain disappears.
        let mut undo = Undo::new(store.clone(), scratch.journal(), Duration::ZERO);
        let fresh = "Software\\Classes\\CLSID\\{y}\\Inproc";
        undo.apply(&[put(fresh, "", Value::String(String::new()))])
            .unwrap();
        undo.restore(|| {}).unwrap();
        assert!(!store.has_key("Software\\Classes\\CLSID\\{y}"));
        assert!(store.has_key("Software\\Classes\\CLSID"));
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
