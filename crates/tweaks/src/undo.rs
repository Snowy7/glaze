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
    /// Exact registry type and bytes, including unusual string encodings/terminators.
    /// The system-preferences journal uses this; existing styling retains its typed values.
    Raw {
        kind: u32,
        bytes: Vec<u8>,
    },
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
    /// Last committed value (or the intended value in journals predating `pending`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    written: Option<Value>,
    /// Values an interrupted apply may have installed. Keep every uncommitted attempt
    /// alongside `written` until a successful apply commits this entry's ownership.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pending: Vec<Value>,
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

    /// The journal's original, distinguishing an absent entry from a value that was absent.
    /// Used when resolving an automatic scheme so our own forced mode cannot feed back.
    pub fn original(&self, key: &str, name: &str) -> Result<Option<Option<Value>>> {
        Ok(self.load()?.and_then(|journal| {
            journal
                .entries
                .into_iter()
                .find(|e| e.key == key && e.name == name)
                .map(|e| e.original)
        }))
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
    /// Ownership retains committed and pending values until the batch succeeds.
    pub fn apply(&mut self, writes: &[Write]) -> Result<bool> {
        let mut journal = self.prepared(writes)?;

        let mut changed = false;
        for w in writes {
            if self.store.read(w.key, w.name)?.as_ref() != Some(&w.value) {
                self.store.write(w.key, w.name, &w.value)?;
                changed = true;
            }
        }

        // Only retire previous ownership after the entire batch succeeded. A crash,
        // failed write or failed commit save retains all possible owned values on disk.
        let mut committed = false;
        for entry in &mut journal.entries {
            if let Some(w) = writes
                .iter()
                .rev()
                .find(|w| entry.key == w.key && entry.name == w.name)
                && (entry.written.as_ref() != Some(&w.value) || !entry.pending.is_empty())
            {
                entry.written = Some(w.value.clone());
                entry.pending.clear();
                committed = true;
            }
        }
        if committed {
            self.save(&journal)?;
        }
        Ok(changed)
    }

    /// Makes a forthcoming registry/window transition recoverable before either is touched.
    pub fn prepare(&mut self, writes: &[Write]) -> Result<()> {
        self.prepared(writes).map(|_| ())
    }

    fn prepared(&self, writes: &[Write]) -> Result<Journal> {
        let mut journal = self.load()?.unwrap_or_default();
        let mut grown = !self.path.exists();
        for w in writes {
            if !journal.has(w.key, w.name) {
                journal.entries.push(Entry {
                    key: w.key.into(),
                    name: w.name.into(),
                    original: self.store.read(w.key, w.name)?,
                    written: None,
                    pending: Vec::new(),
                    created: self.store.first_missing(w.key)?,
                });
                grown = true;
            }
            if let Some(entry) = journal
                .entries
                .iter_mut()
                .find(|e| e.key == w.key && e.name == w.name)
                && entry.written.as_ref() != Some(&w.value)
                && !entry.pending.contains(&w.value)
            {
                entry.pending.push(w.value.clone());
                grown = true;
            }
        }
        if grown {
            self.save(&journal)?;
        }

        Ok(journal)
    }

    /// Replaces the managed set without restoring values that remain managed. New originals
    /// and pending writes are durable before mutation, as in `apply`. Released entries stay
    /// journaled until their originals have survived Windows' reaction to the single broadcast.
    /// An interruption at any boundary therefore leaves a complete recovery journal.
    pub fn transition(&mut self, writes: &[Write], announce: impl FnOnce()) -> Result<bool> {
        let mut changed = self.apply(writes)?;
        let mut journal = self.load()?.context("transition journal disappeared")?;
        let released = Journal {
            entries: journal
                .entries
                .iter()
                .filter(|e| !writes.iter().any(|w| e.key == w.key && e.name == w.name))
                .cloned()
                .collect(),
        };
        changed |= self.put_back_all(&released)?;
        if changed {
            announce();
            if !released.entries.is_empty() {
                for _ in 0..SETTLE_ROUNDS {
                    std::thread::sleep(self.settle);
                    if !self.put_back_all(&released)? {
                        break;
                    }
                }
            }
        }
        if !released.entries.is_empty() {
            journal.entries.retain(|e| writes.iter().any(|w| e.key == w.key && e.name == w.name));
            self.save(&journal)?;
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

    /// Restore a persistent experiment only where the value is still ours. A later change by
    /// the user or another app wins. Old journals without ownership data are left untouched.
    /// Failed restores retain the journal; retries also accept values already restored.
    pub fn restore_if_unchanged(&mut self) -> Result<bool> {
        let Some(journal) = self.load()? else {
            return Ok(false);
        };
        let mut changed = false;
        for entry in journal.entries.iter().rev() {
            let current = self.store.read(&entry.key, &entry.name)?;
            let ours = current.as_ref().is_some_and(|value| {
                entry.written.as_ref() == Some(value) || entry.pending.contains(value)
            });
            let already_restored =
                (entry.written.is_some() || !entry.pending.is_empty()) && current == entry.original;
            if ours || already_restored {
                changed |= self.put_back(entry)?;
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
    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeSet, HashMap};
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct Data {
        values: HashMap<(String, String), Value>,
        keys: BTreeSet<String>,
        mutations: Vec<(String, String)>,
    }

    /// An in-memory registry. Clones share the data, so a test can play
    /// "Windows" (rewriting values behind the journal's back) while `Undo`
    /// owns another handle.
    #[derive(Default, Clone)]
    pub(crate) struct Memory(Rc<RefCell<Data>>);

    impl Memory {
        pub(crate) fn take_mutations(&self) -> Vec<(String, String)> {
            std::mem::take(&mut self.0.borrow_mut().mutations)
        }
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
            self.0.borrow_mut().mutations.push((key.into(), name.into()));
            self.0
                .borrow_mut()
                .values
                .insert((key.into(), name.into()), value.clone());
            Ok(())
        }
        fn delete(&mut self, key: &str, name: &str) -> Result<()> {
            self.0.borrow_mut().mutations.push((key.into(), name.into()));
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
            undo.original("K", "existing").unwrap(),
            Some(Some(Value::Dword(1)))
        );
        assert_eq!(undo.original("K", "fresh").unwrap(), Some(None));
        assert_eq!(undo.original("K", "unmanaged").unwrap(), None);
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
        assert_eq!(undo.original("K", "existing").unwrap(), None);
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

    #[derive(Clone, Copy, Debug)]
    enum Interruption {
        FailedWrite,
        Crash,
    }

    /// Interrupts a registry write after the real journal has been saved. The shared
    /// memory and journal survive when the Undo instance is dropped and reopened.
    struct Interrupting {
        memory: Memory,
        interrupt: Rc<Cell<Option<(usize, Interruption)>>>,
    }

    impl Interrupting {
        fn before_mutation(&self) -> Result<()> {
            if let Some((remaining, interruption)) = self.interrupt.get() {
                if remaining == 0 {
                    self.interrupt.set(None);
                    match interruption {
                        Interruption::FailedWrite => bail!("injected write failure"),
                        Interruption::Crash => bail!("injected crash before registry write"),
                    }
                }
                self.interrupt.set(Some((remaining - 1, interruption)));
            }
            Ok(())
        }
    }

    impl Store for Interrupting {
        fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
            self.memory.read(key, name)
        }

        fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
            self.before_mutation()?;
            self.memory.write(key, name, value)
        }

        fn delete(&mut self, key: &str, name: &str) -> Result<()> {
            self.before_mutation()?;
            self.memory.delete(key, name)
        }

        fn first_missing(&self, key: &str) -> Result<Option<String>> {
            self.memory.first_missing(key)
        }

        fn delete_if_empty(&mut self, key: &str) -> Result<()> {
            self.memory.delete_if_empty(key)
        }
    }

    fn interrupted_apply(undo: &mut Undo<Interrupting>, writes: &[Write], how: Interruption) {
        match how {
            Interruption::FailedWrite => {
                let error = undo.apply(writes).unwrap_err();
                assert_eq!(error.to_string(), "injected write failure");
            }
            Interruption::Crash => assert_eq!(
                undo.apply(writes).unwrap_err().to_string(),
                "injected crash before registry write"
            ),
        }
        assert!(undo.pending(), "interrupted writes must retain the journal");
    }

    /// Exercise the real setting plans at each write boundary, including a partial
    /// batch with both old and new values installed, then reopen the disk journal.
    fn interrupted_setting(
        kind: &str,
        first: &[Write],
        next: &[Write],
        owned_restore: bool,
        how: Interruption,
    ) {
        let changing = next.iter().filter(|next| !first.contains(next)).count();
        assert!(changing > 0);
        for (original_index, original) in [
            None,
            Some(Value::ExpandString("%ORIGINAL%\\handler.exe".into())),
            Some(Value::Raw {
                kind: 7,
                bytes: vec![65, 0, 0, 0, 0, 0, 0, 0],
            }),
        ]
        .into_iter()
        .enumerate()
        {
            for cut in 0..changing {
                let scratch = Scratch::new(&format!("undo-{kind}-{how:?}-{original_index}-{cut}"));
                let mut memory = Memory::default();
                for w in next {
                    if let Some(original) = &original {
                        memory.write(w.key, w.name, original).unwrap();
                    }
                }
                let interrupt = Rc::new(Cell::new(None));
                let mut undo = Undo::new(
                    Interrupting {
                        memory: memory.clone(),
                        interrupt: interrupt.clone(),
                    },
                    scratch.journal(),
                    Duration::ZERO,
                );
                undo.apply(first).unwrap();
                interrupt.set(Some((cut, how)));
                interrupted_apply(&mut undo, next, how);
                // Stopping before the Store write has the same durable state as a
                // crash here: Undo has no drop/rollback. Reopen without unwinding.
                let mut recovered = match how {
                    Interruption::Crash => {
                        drop(undo);
                        Undo::new(
                            Interrupting {
                                memory: memory.clone(),
                                interrupt,
                            },
                            scratch.journal(),
                            Duration::ZERO,
                        )
                    }
                    Interruption::FailedWrite => undo,
                };
                if owned_restore {
                    assert!(recovered.restore_if_unchanged().unwrap());
                } else {
                    recovered.restore(|| {}).unwrap();
                }
                for w in next {
                    assert_eq!(
                        memory.read(w.key, w.name).unwrap(),
                        original,
                        "{kind}, {how:?}, write {cut}: {}\\{}",
                        w.key,
                        w.name
                    );
                }
                assert!(!recovered.pending());
            }
        }
    }

    fn interrupted_folder_routing(how: Interruption) {
        use crate::folder_routing::writes;
        use std::path::Path;

        interrupted_setting(
            "folders",
            &writes(Path::new(r"C:\A\mypaper-files.exe")).unwrap(),
            &writes(Path::new(r"C:\B\mypaper-files.exe")).unwrap(),
            true,
            how,
        );
    }

    #[test]
    fn folder_routing_crash_between_journal_and_write_restores_the_true_originals() {
        interrupted_folder_routing(Interruption::Crash);
    }

    #[test]
    fn folder_routing_failed_update_restores_the_true_originals() {
        interrupted_folder_routing(Interruption::FailedWrite);
    }

    fn interrupted_system_preferences(how: Interruption) {
        use mypaper_config::{PowerMode, SystemConfig};

        let config = |on| SystemConfig {
            game_mode: Some(on),
            show_extensions: Some(on),
            show_hidden: Some(on),
            separate_process: Some(on),
            power_mode: Some(if on {
                PowerMode::Performance
            } else {
                PowerMode::Efficiency
            }),
            ..Default::default()
        };
        interrupted_setting(
            "system",
            &crate::system::writes(&config(true)),
            &crate::system::writes(&config(false)),
            false,
            how,
        );
    }

    #[test]
    fn system_preferences_crash_between_journal_and_write_restores_the_true_originals() {
        interrupted_system_preferences(Interruption::Crash);
    }

    #[test]
    fn system_preferences_failed_update_restores_the_true_originals() {
        interrupted_system_preferences(Interruption::FailedWrite);
    }

    fn interrupted_windows_styling(how: Interruption) {
        use mypaper_theme::{Color, Paint, WindowsStyle};

        let style = |on| WindowsStyle {
            accent: Some(Paint::Color(if on {
                Color([1, 2, 3])
            } else {
                Color([4, 5, 6])
            })),
            dark_mode: Some(on),
            accent_on_taskbar: Some(on),
            accent_on_title_bars: Some(on),
            ..Default::default()
        };
        interrupted_setting(
            "styling",
            &crate::plan::system_writes(&style(true), None),
            &crate::plan::system_writes(&style(false), None),
            false,
            how,
        );
    }

    #[test]
    fn windows_styling_crash_between_journal_and_write_restores_the_true_originals() {
        interrupted_windows_styling(Interruption::Crash);
    }

    #[test]
    fn windows_styling_failed_update_restores_the_true_originals() {
        interrupted_windows_styling(Interruption::FailedWrite);
    }

    #[test]
    fn classic_menu_crash_between_journal_and_write_keeps_the_true_originals() {
        interrupted_setting(
            "classic",
            &[],
            &crate::classic_menu::writes(),
            false,
            Interruption::Crash,
        );
    }

    #[test]
    fn classic_menu_failed_write_keeps_the_true_originals() {
        interrupted_setting(
            "classic",
            &[],
            &crate::classic_menu::writes(),
            false,
            Interruption::FailedWrite,
        );
    }

    #[test]
    fn interrupted_retries_keep_ownership_of_each_possible_installed_value() {
        let scratch = Scratch::new("undo-interrupted-retries");
        let memory = Memory::default();
        let interrupt = Rc::new(Cell::new(None));
        let mut undo = Undo::new(
            Interrupting {
                memory: memory.clone(),
                interrupt: interrupt.clone(),
            },
            scratch.journal(),
            Duration::ZERO,
        );
        let writes = |value| {
            [
                put("K", "", Value::Dword(value)),
                put("L", "", Value::Dword(value)),
            ]
        };
        undo.apply(&writes(1)).unwrap();
        interrupt.set(Some((1, Interruption::FailedWrite)));
        interrupted_apply(&mut undo, &writes(2), Interruption::FailedWrite);
        // K contains pending 2, L still contains committed 1. A retry with a
        // third value must not forget either if it fails before its first write.
        interrupt.set(Some((0, Interruption::FailedWrite)));
        interrupted_apply(&mut undo, &writes(3), Interruption::FailedWrite);
        assert_eq!(memory.read("K", "").unwrap(), Some(Value::Dword(2)));
        assert_eq!(memory.read("L", "").unwrap(), Some(Value::Dword(1)));
        drop(undo);
        let mut recovered = Undo::new(memory.clone(), scratch.journal(), Duration::ZERO);
        assert!(recovered.restore_if_unchanged().unwrap());
        assert_eq!(memory.read("K", "").unwrap(), None);
        assert_eq!(memory.read("L", "").unwrap(), None);
        assert!(!recovered.pending());
    }

    #[test]
    fn an_interrupted_first_apply_owns_pending_writes_and_preserves_foreign_changes() {
        let scratch = Scratch::new("undo-first-pending");
        let mut memory = Memory::default();
        let interrupt = Rc::new(Cell::new(Some((1, Interruption::FailedWrite))));
        let mut undo = Undo::new(
            Interrupting {
                memory: memory.clone(),
                interrupt,
            },
            scratch.journal(),
            Duration::ZERO,
        );
        let original = Value::ExpandString("%ORIGINAL%".into());
        memory.write("L", "", &original).unwrap();
        interrupted_apply(
            &mut undo,
            &[put("K", "", Value::Dword(1)), put("L", "", Value::Dword(2))],
            Interruption::FailedWrite,
        );
        // A foreign deletion is not ownership just because `written` is None.
        memory.delete("L", "").unwrap();
        assert!(undo.restore_if_unchanged().unwrap());
        assert_eq!(memory.read("K", "").unwrap(), None);
        assert_eq!(memory.read("L", "").unwrap(), None);
        assert!(!undo.pending());
    }

    #[test]
    fn successful_commit_retires_previous_and_pending_ownership() {
        let scratch = Scratch::new("undo-commit-ownership");
        let mut memory = Memory::default();
        let interrupt = Rc::new(Cell::new(None));
        let mut undo = Undo::new(
            Interrupting {
                memory: memory.clone(),
                interrupt: interrupt.clone(),
            },
            scratch.journal(),
            Duration::ZERO,
        );
        undo.apply(&[put("K", "", Value::Dword(1))]).unwrap();
        interrupt.set(Some((0, Interruption::FailedWrite)));
        interrupted_apply(
            &mut undo,
            &[put("K", "", Value::Dword(2))],
            Interruption::FailedWrite,
        );
        undo.apply(&[put("K", "", Value::Dword(3))]).unwrap();
        let journal = undo.load().unwrap().unwrap();
        assert_eq!(journal.entries[0].written, Some(Value::Dword(3)));
        assert!(journal.entries[0].pending.is_empty());

        // After committing 3, neither retired 1 nor abandoned 2 is ours.
        for foreign in [Value::Dword(1), Value::Dword(2)] {
            undo.save(&journal).unwrap();
            memory.write("K", "", &foreign).unwrap();
            assert!(!undo.restore_if_unchanged().unwrap());
            assert_eq!(memory.read("K", "").unwrap(), Some(foreign));
        }
    }

    #[test]
    fn legacy_journals_keep_their_ownership_policy_without_pending_data() {
        let scratch = Scratch::new("undo-legacy-ownership");
        std::fs::create_dir_all(&scratch.0).unwrap();
        let journal = serde_json::json!({ "entries": [
            { "key": "K", "name": "owned", "original": { "expandstring": "%OLD%" },
              "written": { "string": "A.exe" } },
            { "key": "K", "name": "unknown", "original": null }
        ] });
        std::fs::write(scratch.journal(), journal.to_string()).unwrap();
        let mut memory = Memory::default();
        memory
            .write("K", "owned", &Value::String("A.exe".into()))
            .unwrap();
        memory.write("K", "unknown", &Value::Dword(9)).unwrap();
        let mut undo = Undo::new(memory.clone(), scratch.journal(), Duration::ZERO);
        assert!(undo.restore_if_unchanged().unwrap());
        assert_eq!(
            memory.read("K", "owned").unwrap(),
            Some(Value::ExpandString("%OLD%".into()))
        );
        assert_eq!(memory.read("K", "unknown").unwrap(), Some(Value::Dword(9)));
        assert!(!undo.pending());
    }

    #[test]
    fn transitions_release_only_removed_values_and_broadcast_once_when_changed() {
        let scratch = Scratch::new("undo-transition");
        let mut memory = Memory::default();
        let original = Value::Raw { kind: 7, bytes: vec![65, 0, 0, 0] };
        memory.write("K", "new", &original).unwrap();
        let mut undo = Undo::new(memory.clone(), scratch.journal(), Duration::ZERO);
        let a = [put("K", "kept", Value::Dword(1)), put("K", "released", Value::Dword(2))];
        let b = [put("K", "kept", Value::Dword(3)), put("K", "new", Value::Dword(4))];
        let broadcasts = Cell::new(0);
        let announce = || broadcasts.set(broadcasts.get() + 1);
        undo.transition(&a, announce).unwrap();
        memory.take_mutations();
        undo.transition(&b, announce).unwrap();
        assert_eq!(memory.take_mutations(), [("K".into(), "kept".into()), ("K".into(), "new".into()), ("K".into(), "released".into())]);
        assert_eq!(broadcasts.get(), 2);
        assert_eq!(memory.read("K", "kept").unwrap(), Some(Value::Dword(3)));
        assert_eq!(memory.read("K", "released").unwrap(), None);
        assert!(!undo.journaled("K", "released"));
        assert_eq!(undo.original("K", "kept").unwrap(), Some(None));
        assert!(!undo.transition(&b, announce).unwrap());
        assert!(memory.take_mutations().is_empty());
        assert_eq!(broadcasts.get(), 2, "an identical transition sends no broadcast");
        undo.transition(&a, announce).unwrap();
        assert_eq!(memory.take_mutations(), [("K".into(), "kept".into()), ("K".into(), "released".into()), ("K".into(), "new".into())]);
        assert_eq!(broadcasts.get(), 3);
        assert_eq!(memory.read("K", "new").unwrap(), Some(original.clone()));
        undo.restore(announce).unwrap();
        assert_eq!(memory.read("K", "kept").unwrap(), None);
        assert_eq!(memory.read("K", "released").unwrap(), None);
        assert_eq!(memory.read("K", "new").unwrap(), Some(original));
    }

    #[test]
    fn interrupted_transitions_recover_at_each_write_and_release_boundary() {
        for how in [Interruption::Crash, Interruption::FailedWrite] {
            for cut in 0..3 {
                let scratch = Scratch::new(&format!("undo-transition-{how:?}-{cut}"));
                let mut memory = Memory::default();
                let original = Value::ExpandString("%ORIGINAL%".into());
                memory.write("K", "new", &original).unwrap();
                let interrupt = Rc::new(Cell::new(None));
                let mut undo = Undo::new(Interrupting { memory: memory.clone(), interrupt: interrupt.clone() }, scratch.journal(), Duration::ZERO);
                let a = [put("K", "kept", Value::Dword(1)), put("K", "released", Value::Dword(2))];
                let b = [put("K", "kept", Value::Dword(3)), put("K", "new", Value::Dword(4))];
                undo.transition(&a, || {}).unwrap();
                interrupt.set(Some((cut, how)));
                assert!(undo.transition(&b, || {}).is_err());
                drop(undo);
                let mut recovered = Undo::new(memory.clone(), scratch.journal(), Duration::ZERO);
                recovered.restore(|| {}).unwrap();
                assert_eq!(memory.read("K", "kept").unwrap(), None);
                assert_eq!(memory.read("K", "released").unwrap(), None);
                assert_eq!(memory.read("K", "new").unwrap(), Some(original));
                assert!(!recovered.pending());
            }
        }
    }

    #[test]
    fn released_originals_settle_after_one_broadcast_while_kept_values_stay_installed() {
        let scratch = Scratch::new("undo-release-settle");
        let mut memory = Memory::default();
        memory.write("K", "released", &Value::Dword(7)).unwrap();
        let mut undo = Undo::new(memory.clone(), scratch.journal(), Duration::ZERO);
        undo.apply(&[put("K", "kept", Value::Dword(1)), put("K", "released", Value::Dword(2))]).unwrap();
        undo.transition(&[put("K", "kept", Value::Dword(3))], || {
            assert_eq!(memory.read("K", "kept").unwrap(), Some(Value::Dword(3)));
            memory.write("K", "released", &Value::Dword(99)).unwrap();
        }).unwrap();
        assert_eq!(memory.read("K", "kept").unwrap(), Some(Value::Dword(3)));
        assert_eq!(memory.read("K", "released").unwrap(), Some(Value::Dword(7)));
        assert!(!undo.journaled("K", "released"));
    }
}
