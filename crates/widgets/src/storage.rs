//! Per-instance widget storage: `%APPDATA%\mypaper\widget-data\<instance>.json`,
//! one JSON object per instance, written atomically on every `set`.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};

/// The most one instance (or one shared namespace) may store.
const MAX_BYTES: usize = 1 << 20;

/// The storage id of a shared namespace (`widget-data\_shared\<ns>.json`). It can't collide
/// with an instance id, which never starts with `_`; and instance removal, which deletes by
/// instance id, never touches it.
pub fn shared(namespace: &str) -> String {
    format!("_shared/{namespace}")
}

pub struct Storage {
    dir: PathBuf,
    cache: HashMap<String, Map<String, Value>>,
}

impl Storage {
    /// A duplicate owns a complete snapshot, including any picked-file URLs in stored data.
    pub fn duplicate(&mut self, from: &str, to: &str) -> Result<()> {
        ensure!(crate::manifest::valid_id(from) && crate::manifest::valid_id(to), "invalid storage id");
        let mut value = Value::Object(self.data(from)?.clone());
        crate::pick::rebase_value(&mut value, from, to);
        let bytes = serde_json::to_vec(&value)?;
        ensure!(bytes.len() <= MAX_BYTES, "storage is limited to 1 MB");
        let path = self.path(to);
        ensure!(!path.exists(), "duplicate storage already exists");
        crate::files::write_atomic(&path, &bytes)?;
        self.cache.remove(to);
        Ok(())
    }

    /// Removal follows a successful layout save; a failed save keeps all instance data.
    pub fn remove(&mut self, instance: &str) -> Result<()> {
        ensure!(crate::manifest::valid_id(instance), "invalid storage id");
        match std::fs::remove_file(self.path(instance)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.cache.remove(instance);
        Ok(())
    }
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            cache: HashMap::new(),
        }
    }

    /// `instance` is a valid id (the layout guarantees it) or a [`shared`] id built from a valid
    /// namespace, so it's a safe file name.
    fn path(&self, instance: &str) -> PathBuf {
        self.dir.join(format!("{instance}.json"))
    }

    fn data(&mut self, instance: &str) -> Result<&mut Map<String, Value>> {
        if !self.cache.contains_key(instance) {
            let path = self.path(instance);
            let data = match std::fs::read_to_string(&path) {
                Ok(text) => {
                    serde_json::from_str(&text).with_context(|| format!("in {}", path.display()))?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            };
            self.cache.insert(instance.to_owned(), data);
        }
        Ok(self.cache.get_mut(instance).expect("just inserted"))
    }

    /// The stored value, or `null`.
    pub fn get(&mut self, instance: &str, key: &str) -> Result<Value> {
        Ok(self
            .data(instance)?
            .get(key)
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Stores `value` under `key`, or deletes the key when `value` is `None`.
    pub fn set(&mut self, instance: &str, key: &str, value: Option<Value>) -> Result<()> {
        let path = self.path(instance);
        let data = self.data(instance)?;
        let mut next = data.clone();
        match value {
            Some(value) => next.insert(key.to_owned(), value),
            None => next.remove(key),
        };
        let bytes = serde_json::to_vec(&next)?;
        ensure!(
            bytes.len() <= MAX_BYTES,
            "storage is limited to 1 MB per instance or shared namespace"
        );
        crate::files::write_atomic(&path, &bytes)?;
        *data = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn values_persist_per_instance() {
        let dir = crate::files::temp_dir("storage");
        let mut storage = Storage::new(dir.clone());
        assert_eq!(storage.get("notes-1", "text").unwrap(), Value::Null);
        storage.set("notes-1", "text", Some(json!("hi"))).unwrap();
        storage
            .set("notes-2", "text", Some(json!({ "a": [1, 2] })))
            .unwrap();

        let mut fresh = Storage::new(dir.clone());
        assert_eq!(fresh.get("notes-1", "text").unwrap(), json!("hi"));
        assert_eq!(
            fresh.get("notes-2", "text").unwrap(),
            json!({ "a": [1, 2] })
        );
        fresh.set("notes-1", "text", None).unwrap();
        assert_eq!(
            Storage::new(dir.clone()).get("notes-1", "text").unwrap(),
            Value::Null
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instances_share_a_namespace_but_not_their_own_storage() {
        let dir = crate::files::temp_dir("storage-shared");
        let mut storage = Storage::new(dir.clone());
        let pins = shared("pins");
        storage.set(&pins, "list", Some(json!([1, 2]))).unwrap();
        // Any instance reads what another wrote, even after a restart.
        assert_eq!(storage.get(&pins, "list").unwrap(), json!([1, 2]));
        assert_eq!(
            Storage::new(dir.clone()).get(&pins, "list").unwrap(),
            json!([1, 2])
        );
        // Namespaces are separate from each other and from instance storage.
        assert_eq!(storage.get(&shared("other"), "list").unwrap(), Value::Null);
        assert_eq!(storage.get("pins", "list").unwrap(), Value::Null);
        assert!(dir.join("_shared").join("pins.json").is_file());
        // A missing value deletes, as in instance storage.
        storage.set(&pins, "list", None).unwrap();
        assert_eq!(storage.get(&pins, "list").unwrap(), Value::Null);

        let big = Value::String("x".repeat(MAX_BYTES));
        assert!(storage.set(&pins, "big", Some(big)).is_err(), "1 MB cap");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn oversized_writes_are_refused_and_change_nothing() {
        let dir = crate::files::temp_dir("storage-limit");
        let mut storage = Storage::new(dir.clone());
        storage.set("a-1", "k", Some(json!(1))).unwrap();
        let big = Value::String("x".repeat(MAX_BYTES));
        assert!(storage.set("a-1", "big", Some(big)).is_err());
        assert_eq!(storage.get("a-1", "big").unwrap(), Value::Null);
        assert_eq!(Storage::new(dir.clone()).get("a-1", "k").unwrap(), json!(1));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
