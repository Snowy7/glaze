//! A 40-byte record and a single UTF-8 name blob. Snapshots are read-only file mappings;
//! changed records and new names live in an overlay until the next event-driven checkpoint.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;

use crate::Hit;

pub const NONE: u32 = u32::MAX;
pub const DIRECTORY: u32 = 0x10;
pub const REPARSE: u32 = 0x400;
const MAGIC: &[u8; 8] = b"MYPIDX01";
const HEADER: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub struct Record {
    pub name: u32,
    pub name_len: u16,
    pub fold_len: u16,
    pub parent: u32,
    pub first: u32,
    pub next: u32,
    pub attributes: u32,
    pub modified: u32,
    pub files: u32,
    pub size: u64,
}

impl Record {
    pub fn directory(self) -> bool {
        self.attributes & DIRECTORY != 0
    }
    pub fn live(self) -> bool {
        self.attributes != 0
    }
}

/// Unicode lowercase, shared by indexing, filters and path lookup. Original spelling is kept.
/// This is locale-independent; it deliberately does not apply accent removal or normalization.
pub fn fold(name: &str) -> String {
    name.to_lowercase()
}

fn name_hash(name: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
pub struct Store {
    mapping: Option<Mmap>,
    mapped_count: usize,
    mapped_names: usize,
    records: Vec<Record>,
    names: Vec<u8>,
    changed: BTreeMap<u32, Record>,
    // Created only for wide directories reached by live updates/path lookup. A compact
    // hash-to-ID table avoids quadratic bursts without retaining a String per indexed file.
    children: std::sync::Mutex<HashMap<u32, HashMap<u64, u32>>>,
}

impl Store {
    pub fn new() -> Self {
        let mut store = Self::default();
        store.records.push(Record {
            parent: NONE,
            first: NONE,
            next: NONE,
            attributes: DIRECTORY,
            ..Record::default()
        });
        store
    }

    pub fn len(&self) -> usize {
        self.mapped_count + self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() <= 1
    }
    pub fn overlay_len(&self) -> usize {
        self.changed.len() + self.records.len()
    }

    fn base(&self) -> &[Record] {
        self.mapping.as_ref().map_or(&[], |m| {
            bytemuck::cast_slice(&m[HEADER..HEADER + self.mapped_count * size_of::<Record>()])
        })
    }

    pub fn record(&self, id: u32) -> Record {
        self.changed.get(&id).copied().unwrap_or_else(|| {
            if (id as usize) < self.mapped_count {
                self.base()[id as usize]
            } else {
                self.records[id as usize - self.mapped_count]
            }
        })
    }

    pub fn set(&mut self, id: u32, record: Record) {
        if (id as usize) < self.mapped_count {
            self.changed.insert(id, record);
        } else {
            self.records[id as usize - self.mapped_count] = record;
        }
    }

    /// Sequential overlay merge: no hash/tree lookup in the million-record search loop.
    pub fn range(&self, start: usize, end: usize) -> impl Iterator<Item = (u32, Record)> + '_ {
        let mut changes = self.changed.range(start as u32..end as u32).peekable();
        (start..end).map(move |id| {
            let record = if changes.peek().is_some_and(|(key, _)| **key == id as u32) {
                *changes.next().expect("peeked").1
            } else if id < self.mapped_count {
                self.base()[id]
            } else {
                self.records[id - self.mapped_count]
            };
            (id as u32, record)
        })
    }

    /// New records tend to be recent. Visiting them first usually fills the top-k once,
    /// rather than replacing its worst member for every monotonically newer name.
    pub fn reverse_range(
        &self,
        start: usize,
        end: usize,
    ) -> impl Iterator<Item = (u32, Record)> + '_ {
        let mut changes = self
            .changed
            .range(start as u32..end as u32)
            .rev()
            .peekable();
        (start..end).rev().map(move |id| {
            let record = if changes.peek().is_some_and(|(key, _)| **key == id as u32) {
                *changes.next().expect("peeked").1
            } else if id < self.mapped_count {
                self.base()[id]
            } else {
                self.records[id - self.mapped_count]
            };
            (id as u32, record)
        })
    }

    fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        if offset < self.mapped_names {
            let start = HEADER + self.mapped_count * size_of::<Record>() + offset;
            &self.mapping.as_ref().expect("mapped names")[start..start + len]
        } else {
            &self.names[offset - self.mapped_names..offset - self.mapped_names + len]
        }
    }

    pub fn name(&self, record: Record) -> &str {
        // All names originate in UTF-8 Rust strings; open() validates persisted strings.
        std::str::from_utf8(self.bytes(record.name as usize, record.name_len as usize))
            .expect("validated name")
    }

    pub fn folded(&self, record: Record) -> &[u8] {
        if record.fold_len == 0 {
            self.bytes(record.name as usize, record.name_len as usize)
        } else {
            self.bytes(
                record.name as usize + record.name_len as usize,
                record.fold_len as usize,
            )
        }
    }

    fn intern(&mut self, name: &str) -> (u32, u16, u16) {
        let lower = fold(name);
        let offset =
            u32::try_from(self.mapped_names + self.names.len()).expect("name blob exceeds 4 GiB");
        let len = u16::try_from(name.len()).expect("name exceeds UTF-8 path limit");
        self.names.extend_from_slice(name.as_bytes());
        let folded = if name == lower {
            0
        } else {
            self.names.extend_from_slice(lower.as_bytes());
            u16::try_from(lower.len()).expect("folded name exceeds path limit")
        };
        (offset, len, folded)
    }

    /// Inserts a child. Bulk scanning rolls sizes up once; live changes use upsert().
    pub fn insert(
        &mut self,
        parent: u32,
        name: &str,
        attributes: u32,
        size: u64,
        modified: u32,
    ) -> u32 {
        let id = u32::try_from(self.len()).expect("index exceeds four billion entries");
        let (name, name_len, fold_len) = self.intern(name);
        let mut p = self.record(parent);
        self.records.push(Record {
            name,
            name_len,
            fold_len,
            parent,
            first: NONE,
            next: p.first,
            attributes,
            size: if attributes & DIRECTORY == 0 { size } else { 0 },
            files: u32::from(attributes & DIRECTORY == 0),
            modified,
        });
        p.first = id;
        self.set(parent, p);
        if self
            .children
            .get_mut()
            .expect("child lookup")
            .contains_key(&parent)
        {
            let key = name_hash(self.folded(self.record(id)));
            self.children
                .get_mut()
                .expect("child lookup")
                .get_mut(&parent)
                .expect("existing lookup")
                .insert(key, id);
        }
        id
    }

    pub fn child(&self, parent: u32, name: &str) -> Option<u32> {
        let lower = fold(name);
        let mut indexes = self.children.lock().expect("child lookup");
        if let Some(index) = indexes.get(&parent) {
            let id = *index.get(&name_hash(lower.as_bytes()))?;
            let r = self.record(id);
            if self.folded(r) == lower.as_bytes() {
                return r.live().then_some(id);
            }
            // A 64-bit hash collision falls back to the sibling chain, preserving correctness.
        }
        let mut at = self.record(parent).first;
        let mut count = 0;
        let mut found = None;
        while at != NONE {
            let r = self.record(at);
            count += 1;
            if r.live() && self.folded(r) == lower.as_bytes() {
                found = Some(at);
            }
            at = r.next;
        }
        if count > 128 && !indexes.contains_key(&parent) {
            let mut index = HashMap::with_capacity(count);
            let mut at = self.record(parent).first;
            while at != NONE {
                let r = self.record(at);
                if r.live() {
                    index.insert(name_hash(self.folded(r)), at);
                }
                at = r.next;
            }
            indexes.insert(parent, index);
        }
        found
    }

    pub fn find(&self, path: &str) -> Option<u32> {
        let path = path.trim_start_matches(r"\\?\").replace('/', "\\");
        let path = path.trim_end_matches('\\');
        let lower = fold(path);
        let mut root = self.record(0).first;
        while root != NONE {
            let r = self.record(root);
            let name = std::str::from_utf8(self.folded(r)).ok()?;
            if r.live()
                && lower.starts_with(name)
                && (lower.len() == name.len() || lower.as_bytes().get(name.len()) == Some(&b'\\'))
            {
                let mut id = root;
                for part in path[name.len()..].split('\\').filter(|s| !s.is_empty()) {
                    id = self.child(id, part)?;
                }
                return Some(id);
            }
            root = r.next;
        }
        None
    }

    pub fn path(&self, mut id: u32) -> String {
        let mut components = Vec::new();
        while id != 0 && id != NONE {
            let r = self.record(id);
            components.push(self.name(r));
            id = r.parent;
        }
        components.reverse();
        let mut path = components.join("\\");
        if path.ends_with(':') {
            path.push('\\');
        }
        path
    }

    pub fn hit(&self, id: u32) -> Hit {
        let r = self.record(id);
        Hit {
            name: self.name(r).into(),
            path: self.path(id),
            directory: r.directory(),
            size: r.size,
            files: r.files,
            modified: r.modified,
        }
    }

    pub fn sizes(&self, path: &str, limit: usize) -> Vec<Hit> {
        let Some(id) = self.find(path) else {
            return Vec::new();
        };
        let mut children = Vec::new();
        let mut at = self.record(id).first;
        while at != NONE {
            let r = self.record(at);
            if r.live() {
                children.push(at);
            }
            at = r.next;
        }
        children.sort_unstable_by_key(|id| std::cmp::Reverse(self.record(*id).size));
        std::iter::once(id)
            .chain(children.into_iter().take(limit.min(1000)))
            .map(|id| self.hit(id))
            .collect()
    }

    /// Parent IDs precede children during enumeration, so one backwards pass rolls everything up.
    pub fn roll_up(&mut self) {
        for id in 0..self.len() {
            let mut r = self.record(id as u32);
            if r.directory() {
                r.size = 0;
                r.files = 0;
                self.set(id as u32, r);
            }
        }
        for id in (1..self.len()).rev() {
            let r = self.record(id as u32);
            if r.live() {
                self.adjust(r.parent, i128::from(r.size), i64::from(r.files), false);
            }
        }
    }

    fn adjust(&mut self, mut id: u32, size: i128, files: i64, ancestors: bool) {
        while id != NONE {
            let mut r = self.record(id);
            r.size = (i128::from(r.size) + size).clamp(0, i128::from(u64::MAX)) as u64;
            r.files = (i64::from(r.files) + files).clamp(0, i64::from(u32::MAX)) as u32;
            self.set(id, r);
            if !ancestors {
                break;
            }
            id = r.parent;
        }
    }

    pub fn upsert(
        &mut self,
        parent: u32,
        name: &str,
        attributes: u32,
        size: u64,
        modified: u32,
    ) -> u32 {
        if let Some(id) = self.child(parent, name) {
            let old = self.record(id);
            if old.directory() == (attributes & DIRECTORY != 0) {
                let mut r = old;
                r.attributes = attributes;
                r.modified = modified;
                if !r.directory() {
                    r.size = size;
                    self.adjust(parent, i128::from(size) - i128::from(old.size), 0, true);
                }
                // Case-only renames need the current display spelling too.
                if self.name(r) != name {
                    (r.name, r.name_len, r.fold_len) = self.intern(name);
                }
                self.set(id, r);
                return id;
            }
            self.remove(id);
        }
        let id = self.insert(parent, name, attributes, size, modified);
        let r = self.record(id);
        self.adjust(parent, i128::from(r.size), i64::from(r.files), true);
        id
    }

    /// Tombstones preserve IDs and sibling links; descendants are removed in the same event.
    pub fn remove(&mut self, id: u32) {
        let r = self.record(id);
        if !r.live() {
            return;
        }
        self.adjust(r.parent, -i128::from(r.size), -i64::from(r.files), true);
        let mut pending = vec![id];
        while let Some(at) = pending.pop() {
            let mut record = self.record(at);
            let mut child = record.first;
            while child != NONE {
                pending.push(child);
                child = self.record(child).next;
            }
            record.attributes = 0;
            self.set(at, record);
        }
    }

    /// Replaces one root/subtree after reconciliation, leaving other volumes untouched.
    pub fn import(&mut self, source: &Store, source_id: u32, parent: u32) -> u32 {
        let r = source.record(source_id);
        let id = self.insert(parent, source.name(r), r.attributes, r.size, r.modified);
        let mut copied = self.record(id);
        copied.size = r.size;
        copied.files = r.files;
        self.set(id, copied);
        self.adjust(parent, i128::from(r.size), i64::from(r.files), true);
        let mut pending = vec![(source_id, id)];
        while let Some((from, to)) = pending.pop() {
            let mut child = source.record(from).first;
            while child != NONE {
                let c = source.record(child);
                if c.live() {
                    let added = self.insert(to, source.name(c), c.attributes, c.size, c.modified);
                    let mut copied = self.record(added);
                    copied.size = c.size;
                    copied.files = c.files;
                    self.set(added, copied);
                    if c.directory() {
                        pending.push((child, added));
                    }
                }
                child = c.next;
            }
        }
        id
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let temporary = path.with_extension("tmp");
        let mut out = BufWriter::new(File::create(&temporary)?);
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        // Compact tombstones and superseded names on checkpoint. IDs are internal to a
        // snapshot; preserving insertion order keeps every parent before its descendants.
        let mut ids = vec![NONE; self.len()];
        let mut count = 0u32;
        let mut names = 0u64;
        for (id, r) in self.range(0, self.len()) {
            if r.live() {
                ids[id as usize] = count;
                count += 1;
                names += u64::from(r.name_len) + u64::from(r.fold_len);
            }
        }
        ensure!(names < u64::from(u32::MAX), "name blob exceeds 4 GiB");
        header[8..16].copy_from_slice(&u64::from(count).to_le_bytes());
        header[16..24].copy_from_slice(&names.to_le_bytes());
        out.write_all(&header)?;
        let skip_dead = |mut at: u32| {
            while at != NONE && ids[at as usize] == NONE {
                at = self.record(at).next;
            }
            if at == NONE { NONE } else { ids[at as usize] }
        };
        let mut name = 0;
        for (_, mut record) in self.range(0, self.len()).filter(|(_, r)| r.live()) {
            record.name = name;
            name += u32::from(record.name_len) + u32::from(record.fold_len);
            record.parent = if record.parent == NONE {
                NONE
            } else {
                ids[record.parent as usize]
            };
            record.first = skip_dead(record.first);
            record.next = skip_dead(record.next);
            out.write_all(bytemuck::bytes_of(&record))?;
        }
        for (_, record) in self.range(0, self.len()).filter(|(_, r)| r.live()) {
            out.write_all(self.bytes(
                record.name as usize,
                record.name_len as usize + record.fold_len as usize,
            ))?;
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        drop(out);
        std::fs::rename(&temporary, path)
            .with_context(|| format!("checkpoint {}", path.display()))?;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: snapshot files are immutable, replaced by rename. We never truncate a mapped file.
        let mapping = unsafe { Mmap::map(&file)? };
        ensure!(
            mapping.len() >= HEADER && &mapping[..8] == MAGIC,
            "bad index header"
        );
        let count = usize::try_from(u64::from_le_bytes(mapping[8..16].try_into()?))?;
        let names = usize::try_from(u64::from_le_bytes(mapping[16..24].try_into()?))?;
        let end = count
            .checked_mul(size_of::<Record>())
            .and_then(|n| n.checked_add(HEADER))
            .and_then(|n| n.checked_add(names));
        ensure!(
            count > 0 && count < u32::MAX as usize && end == Some(mapping.len()),
            "bad index length"
        );
        let store = Self {
            mapping: Some(mapping),
            mapped_count: count,
            mapped_names: names,
            ..Self::default()
        };
        // One streaming UTF-8 validation is much cheaper than two decoder calls for each
        // short name. Record slices are then checked against character boundaries below.
        let blob = std::str::from_utf8(
            &store.mapping.as_ref().expect("mapped snapshot")
                [HEADER + count * size_of::<Record>()..],
        )?;
        let validate = |start: usize, end: usize| -> Result<()> {
            for (id, r) in store.range(start, end) {
                ensure!(
                    (r.name as usize)
                        .checked_add(r.name_len as usize + r.fold_len as usize)
                        .is_some_and(|n| n <= names),
                    "bad name offset"
                );
                ensure!(
                    (r.parent as usize) < count || (id == 0 && r.parent == NONE),
                    "bad parent"
                );
                ensure!(r.first == NONE || (r.first as usize) < count, "bad child");
                ensure!(r.next == NONE || (r.next as usize) < count, "bad sibling");
                ensure!(id == 0 || r.parent < id, "cyclic parent");
                if r.first != NONE {
                    ensure!(r.first > id, "cyclic child");
                }
                if r.next != NONE {
                    ensure!(
                        r.next < id && store.record(r.next).parent == r.parent,
                        "cyclic sibling"
                    );
                }
                let start = r.name as usize;
                let middle = start + r.name_len as usize;
                let end = middle + r.fold_len as usize;
                ensure!(
                    blob.is_char_boundary(start)
                        && blob.is_char_boundary(middle)
                        && blob.is_char_boundary(end),
                    "name splits a UTF-8 character"
                );
            }
            Ok(())
        };
        let workers = if count >= 250_000 {
            std::thread::available_parallelism().map_or(2, |n| n.get().min(8))
        } else {
            1
        };
        if workers == 1 {
            validate(0, count)?;
        } else {
            // Temporary workers keep validation and page faults parallel without retaining
            // another idle pool in the helper. The mapping outlives every scoped worker.
            std::thread::scope(|scope| -> Result<()> {
                let mut tasks = Vec::new();
                for worker in 0..workers {
                    let validate = &validate;
                    tasks.push(
                        std::thread::Builder::new()
                            .name(format!("index-load-{worker}"))
                            .stack_size(256 * 1024)
                            .spawn_scoped(scope, move || {
                                validate(count * worker / workers, count * (worker + 1) / workers)
                            })?,
                    );
                }
                for task in tasks {
                    task.join()
                        .map_err(|_| anyhow::anyhow!("index validation worker panicked"))??;
                }
                Ok(())
            })?;
        }
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_directory_lookup_survives_delete_recreate_and_case_changes() {
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        for i in 0..1000 {
            store.insert(root, &format!("file-{i}.txt"), 128, 1, 0);
        }
        let old = store.child(root, "FILE-500.TXT").unwrap();
        store.remove(old);
        assert!(store.child(root, "file-500.txt").is_none());
        let new = store.upsert(root, "File-500.txt", 128, 2, 1);
        assert_ne!(old, new);
        assert_eq!(store.child(root, "FILE-500.TXT"), Some(new));
        store.upsert(root, "FILE-500.TXT", 128, 3, 2);
        assert_eq!(store.name(store.record(new)), "FILE-500.TXT");
        let added = store.upsert(root, "fresh.txt", 128, 4, 3);
        assert_eq!(store.child(root, "fresh.txt"), Some(added));
    }

    #[test]
    fn sizes_follow_add_resize_delete_and_directory_removal() {
        assert_eq!(size_of::<Record>(), 40);
        let mut s = Store::new();
        let root = s.insert(0, "C:", DIRECTORY, 0, 0);
        let dir = s.upsert(root, "Photos", DIRECTORY, 0, 1);
        let file = s.upsert(dir, "İstanbul.JPG", 128, 20, 2);
        assert_eq!(s.record(root).size, 20);
        assert_eq!(s.find("c:/photos/İSTANBUL.jpg"), Some(file));
        s.upsert(dir, "İstanbul.JPG", 128, 30, 3);
        assert_eq!((s.record(root).size, s.record(root).files), (30, 1));
        s.remove(dir);
        assert_eq!((s.record(root).size, s.record(root).files), (0, 0));
        assert!(s.find(r"C:\Photos\İstanbul.JPG").is_none());
        assert_eq!(fold("ÉCOLE ΑΒΓ العربية"), "école αβγ العربية");
    }

    #[test]
    fn snapshot_round_trip_overlay_and_corrupt_records() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-store-{}.bin", std::process::id()));
        let mut s = Store::new();
        let root = s.insert(0, "D:", DIRECTORY, 0, 0);
        s.insert(root, "A.txt", 128, 42, 8);
        s.roll_up();
        s.save(&path).unwrap();
        let mut mapped = Store::open(&path).unwrap();
        assert_eq!(mapped.hit(2).path, r"D:\A.txt");
        mapped.upsert(root, "a.TXT", 128, 19, 9);
        mapped.upsert(root, "b.txt", 128, 7, 10);
        assert_eq!(mapped.sizes("d:\\", 3)[0].size, 26);
        mapped.save(&path).unwrap();
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.name(reopened.record(2)), "a.TXT");
        assert_eq!(reopened.record(root).files, 2);
        drop(mapped);
        drop(reopened);
        let mut compact = Store::open(&path).unwrap();
        let removed = compact.find(r"D:\b.txt").unwrap();
        compact.remove(removed);
        compact.save(&path).unwrap();
        drop(compact);
        let compact = Store::open(&path).unwrap();
        assert_eq!(compact.len(), 3);
        assert_eq!(compact.record(root).files, 1);
        assert_eq!(compact.hit(2).path, r"D:\a.TXT");
        drop(compact);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[HEADER + 2 * size_of::<Record>() + 8..HEADER + 2 * size_of::<Record>() + 12]
            .copy_from_slice(&2u32.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(Store::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn snapshot_validation_rejects_a_slice_starting_inside_utf8() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-utf8-{}.bin", std::process::id()));
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        store.insert(root, "é.txt", 128, 1, 0);
        let record = store.record(2);
        store.save(&path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let at = HEADER + 2 * size_of::<Record>();
        bytes[at..at + 4].copy_from_slice(&(record.name + 1).to_le_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&(record.name_len - 1).to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(Store::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
