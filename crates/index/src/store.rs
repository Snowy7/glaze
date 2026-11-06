//! A 40-byte record and a single UTF-8 name blob. Snapshots are read-only file mappings;
//! changed records and new names live in an overlay until the next event-driven checkpoint.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::Hit;

pub const NONE: u32 = u32::MAX;
pub const DIRECTORY: u32 = 0x10;
pub const REPARSE: u32 = 0x400;
// Private snapshot bits, outside Win32's currently defined file attribute range.
pub const EXCLUDED: u32 = 0x4000_0000;
pub const SIZE_UNKNOWN: u32 = 0x8000_0000;
const MAGIC: &[u8; 8] = b"MYPIDX01";
const HEADER: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable, Serialize, Deserialize)]
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
    fn same_search(self, other: Self) -> bool {
        (
            self.name,
            self.name_len,
            self.fold_len,
            self.parent,
            self.modified,
            self.attributes & (DIRECTORY | EXCLUDED),
            self.live(),
        ) == (
            other.name,
            other.name_len,
            other.fold_len,
            other.parent,
            other.modified,
            other.attributes & (DIRECTORY | EXCLUDED),
            other.live(),
        )
    }
}

/// Unicode lowercase, shared by indexing, filters and path lookup. Original spelling is kept.
/// This is locale-independent; it deliberately does not apply accent removal or normalization.
pub fn fold(name: &str) -> String {
    name.to_lowercase()
}

fn name_hash(name: &[u8]) -> u32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish() as u32
}

/// The snapshot stays immutable. Small updates are atomically checkpointed as a bounded
/// overlay; its generation prevents replay onto a different snapshot after a crash.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Changes<'a> {
    generation: u64,
    mapped_count: usize,
    mapped_names: usize,
    changed: Cow<'a, BTreeMap<u32, Record>>,
    records: Cow<'a, [Record]>,
    names: Cow<'a, str>,
}

struct SearchChange {
    record: Record,
    name: Vec<u8>,
}

#[derive(Default)]
struct PathCache {
    entries: HashMap<u32, String>,
    bytes: usize,
}

impl PathCache {
    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
    fn insert(&mut self, id: u32, path: &str) {
        if self.bytes + path.len() > 2 * 1024 * 1024 || self.entries.len() >= 8192 {
            self.clear();
        }
        self.bytes += path.len();
        self.entries.insert(id, path.to_owned());
    }
}

#[derive(Default)]
pub struct Store {
    quiet: Option<std::sync::Arc<crate::quiet::Quiet>>,
    column: Option<crate::names::Column>,
    column_start: usize,
    search_windows: Vec<crate::residency::Windows>,
    path_cache: std::sync::Mutex<PathCache>,
    windows: Option<crate::residency::Windows>,
    spool: Option<crate::spool::Spool>,
    mapping: Option<Mmap>,
    source: Option<std::sync::Arc<File>>,
    mapped_count: usize,
    mapped_names: usize,
    generation: u64,
    records: Vec<Record>,
    names: Vec<u8>,
    changed: BTreeMap<u32, Record>,
    search_changed: BTreeMap<u32, SearchChange>,
    path_masks_stale: bool,
    // Created only for wide directories reached by live updates/path lookup. A compact
    // hash-to-ID table avoids quadratic bursts without retaining a String per indexed file.
    children: std::sync::Mutex<HashMap<u32, HashMap<u32, u32>>>,
}

impl Store {
    pub(crate) fn set_quiet(&mut self, quiet: std::sync::Arc<crate::quiet::Quiet>) {
        self.quiet = Some(quiet);
    }
    pub(crate) fn quiet(&self) -> Option<std::sync::Arc<crate::quiet::Quiet>> {
        self.quiet.clone()
    }
    pub(crate) fn release_query_paths(&self) {
        *self.path_cache.lock().expect("path cache") = PathCache::default();
    }
    pub(crate) fn clear_quiet(&mut self) {
        self.quiet = None;
    }
    /// An independent view for volume reconciliation. The immutable mapping is shared by the
    /// OS; only the bounded overlay is copied. Readers keep the old view until the final swap.
    pub fn fork(&self) -> Result<Self> {
        ensure!(self.spool.is_none(), "cannot fork unfinished scan scratch");
        let mapping = match &self.source {
            // SAFETY: snapshots are immutable, replaced by rename, never truncated in place.
            Some(file) => Some(unsafe { Mmap::map(file.as_ref())? }),
            None => None,
        };
        Ok(Self {
            quiet: self.quiet.clone(),
            column: None,
            column_start: self.column_start,
            search_windows: Vec::new(),
            path_cache: std::sync::Mutex::new(PathCache::default()),
            windows: None,
            spool: None,
            mapping,
            source: self.source.clone(),
            mapped_count: self.mapped_count,
            mapped_names: self.mapped_names,
            generation: self.generation,
            records: self.records.clone(),
            names: self.names.clone(),
            changed: self.changed.clone(),
            search_changed: BTreeMap::new(),
            path_masks_stale: self.path_masks_stale,
            children: std::sync::Mutex::new(HashMap::new()),
        })
    }

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

    pub fn temporary(directory: &Path) -> Result<Self> {
        let mut store = Self::new();
        let mut spool = crate::spool::Spool::new(directory)?;
        spool.push(store.records.pop().expect("virtual root"));
        store.spool = Some(spool);
        Ok(store)
    }

    pub fn len(&self) -> usize {
        if let Some(spool) = &self.spool {
            return spool.count;
        }
        self.mapped_count + self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() <= 1
    }

    pub fn search_blocks(&self) -> usize {
        self.column
            .as_ref()
            .map_or(self.len(), |column| column.blocks.len())
    }

    pub(crate) fn candidate_blocks(&self, filter: &crate::names::Filter) -> (usize, usize) {
        let Some(column) = &self.column else {
            return (0, 0);
        };
        let mut blocks = 0;
        let mut bytes = 0;
        for (i, block) in column.blocks.iter().enumerate() {
            if column.may_match(i, filter) {
                blocks += 1;
                let next = column.blocks.get(i + 1).map_or(
                    self.mapping.as_ref().expect("column").len() - self.column_start,
                    |next| next.offset,
                );
                bytes += next - block.offset;
            }
        }
        (blocks, bytes)
    }

    pub fn overridden(&self, id: u32) -> bool {
        (id as usize) < self.mapped_count && self.search_changed.contains_key(&id)
    }

    pub(crate) fn short_prefix(
        &self,
        name: &[u8],
        directory: Option<bool>,
        limit: usize,
        extra: &[crate::names::Candidate],
    ) -> Option<Vec<crate::names::Candidate>> {
        self.column
            .as_ref()?
            .short_prefix(name, directory, limit, extra, |id| self.overridden(id))
    }

    pub(crate) fn path_possible(&self, parent: u32, name: &[u8], required: &[[u64; 2]]) -> bool {
        self.path_masks_stale
            || self
                .column
                .as_ref()
                .is_none_or(|column| column.path_possible(parent, name, required))
    }

    /// Search reads only folded names and ranking fields. Overlay entries are searched once
    /// by the final shard; old versions are ignored before candidate ranking.
    pub(crate) fn visit_names(
        &self,
        start: usize,
        end: usize,
        filter: &crate::names::Filter,
        lane: usize,
        mut visit: impl FnMut(u32, u32, u32, u32, &[u8], bool),
    ) {
        if let Some(column) = &self.column {
            let map = self.mapping.as_ref().expect("names mapping");
            let touch = |offset, len| {
                if let Some(windows) = self.search_windows.get(lane) {
                    windows.touch(map.as_ptr(), map.len(), self.column_start + offset, len);
                }
            };
            for index in (start..end).rev() {
                let block = &column.blocks[index];
                if !column.may_match(index, filter) {
                    continue;
                }
                let offset = block.offset;
                let next = column
                    .blocks
                    .get(index + 1)
                    .map(|block| block.offset)
                    .unwrap_or(map.len() - self.column_start);
                touch(offset, next - offset);
                crate::names::Column::visit(
                    &map[self.column_start + offset..self.column_start + next],
                    |id, parent, attr, modified, name| {
                        visit(id, parent, attr, modified, name, true)
                    },
                );
            }
            if end == column.blocks.len() {
                for (id, r) in &self.search_changed {
                    visit(
                        *id,
                        r.record.parent,
                        r.record.attributes,
                        r.record.modified,
                        &r.name,
                        false,
                    );
                }
                for (i, r) in self.records.iter().enumerate() {
                    visit(
                        (self.mapped_count + i) as u32,
                        r.parent,
                        r.attributes,
                        r.modified,
                        self.folded(*r),
                        false,
                    );
                }
            }
        } else {
            for (id, r) in self.reverse_range(start, end) {
                visit(
                    id,
                    r.parent,
                    r.attributes,
                    r.modified,
                    self.folded(r),
                    false,
                );
            }
        }
    }
    pub fn overlay_len(&self) -> usize {
        self.changed.len() + self.records.len()
    }

    pub fn should_compact(&self) -> bool {
        self.mapped_count == 0 || self.overlay_len() > 50_000 || self.names.len() > 4 * 1024 * 1024
    }

    pub fn save_changes(&self, path: &Path) -> Result<()> {
        let temporary = path.with_extension("tmp");
        let mut out = BufWriter::new(File::create(&temporary)?);
        let changes = Changes {
            generation: self.generation,
            mapped_count: self.mapped_count,
            mapped_names: self.mapped_names,
            changed: Cow::Borrowed(&self.changed),
            records: Cow::Borrowed(&self.records),
            names: Cow::Borrowed(std::str::from_utf8(&self.names)?),
        };
        serde_json::to_writer(&mut out, &changes)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        drop(out);
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    /// Validates an overlay completely before changing the live store. Missing or mismatched
    /// generations leave the snapshot usable; callers may ignore/rebuild an unusable overlay.
    pub fn load_changes(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path)?;
        ensure!(
            file.metadata()?.len() <= 64 * 1024 * 1024,
            "overlay exceeds its size limit"
        );
        let changes: Changes<'static> = serde_json::from_reader(BufReader::new(file))?;
        ensure!(
            changes.generation == self.generation
                && changes.mapped_count == self.mapped_count
                && changes.mapped_names == self.mapped_names,
            "overlay belongs to another snapshot"
        );
        let count = self
            .mapped_count
            .checked_add(changes.records.len())
            .context("overlay count overflow")?;
        ensure!(
            count < u32::MAX as usize
                && self.mapped_names + changes.names.len() <= u32::MAX as usize,
            "overlay too large"
        );
        let raw = |id: u32| -> Option<Record> {
            changes.changed.get(&id).copied().or_else(|| {
                if (id as usize) < self.mapped_count {
                    self.base().get(id as usize).copied()
                } else {
                    changes
                        .records
                        .get(id as usize - self.mapped_count)
                        .copied()
                }
            })
        };
        for (id, record) in changes.changed.iter().map(|(id, r)| (*id, *r)).chain(
            changes
                .records
                .iter()
                .enumerate()
                .map(|(i, r)| ((self.mapped_count + i) as u32, *r)),
        ) {
            ensure!((id as usize) < count, "overlay record out of range");
            ensure!(
                record.parent < id || (id == 0 && record.parent == NONE),
                "invalid overlay parent"
            );
            ensure!(
                record.first == NONE
                    || (record.first > id
                        && (record.first as usize) < count
                        && raw(record.first).is_some_and(|r| r.parent == id)),
                "invalid overlay child"
            );
            ensure!(
                record.next == NONE
                    || (record.next < id
                        && raw(record.next).is_some_and(|r| r.parent == record.parent)),
                "invalid overlay sibling"
            );
            let start = record.name as usize;
            let length = record.name_len as usize + record.fold_len as usize;
            let bytes = if start < self.mapped_names {
                ensure!(
                    start
                        .checked_add(length)
                        .is_some_and(|end| end <= self.mapped_names),
                    "overlay name crosses blobs"
                );
                self.bytes(start, length)
            } else {
                changes
                    .names
                    .as_bytes()
                    .get(start - self.mapped_names..)
                    .and_then(|s| s.get(..length))
                    .context("invalid overlay name")?
            };
            std::str::from_utf8(&bytes[..record.name_len as usize])?;
            std::str::from_utf8(&bytes[record.name_len as usize..])?;
        }
        ensure!(
            changes
                .changed
                .keys()
                .all(|id| (*id as usize) < self.mapped_count),
            "overlay replaced an appended record"
        );
        self.changed = changes.changed.into_owned();
        self.records = changes.records.into_owned();
        self.names = changes.names.into_owned().into_bytes();
        self.path_cache.get_mut().expect("path cache").clear();
        self.search_changed.clear();
        for (id, record) in self
            .changed
            .iter()
            .map(|(id, r)| (*id, *r))
            .collect::<Vec<_>>()
        {
            self.cache_search_change(id, record);
        }
        self.children.get_mut().expect("child lookup").clear();
        Ok(())
    }

    fn base(&self) -> &[Record] {
        self.mapping.as_ref().map_or(&[], |m| {
            bytemuck::cast_slice(&m[HEADER..HEADER + self.mapped_count * size_of::<Record>()])
        })
    }

    fn touch_mapping(&self, offset: usize, length: usize) {
        if let Some(windows) = &self.windows {
            let map = self.mapping.as_ref().expect("maintenance mapping");
            windows.touch(map.as_ptr(), map.len(), offset, length);
        }
    }

    pub fn record(&self, id: u32) -> Record {
        if let Some(spool) = &self.spool {
            return spool.record(id as usize);
        }
        self.changed.get(&id).copied().unwrap_or_else(|| {
            if (id as usize) < self.mapped_count {
                self.touch_mapping(
                    HEADER + id as usize * size_of::<Record>(),
                    size_of::<Record>(),
                );
                self.base()[id as usize]
            } else {
                self.records[id as usize - self.mapped_count]
            }
        })
    }

    pub fn set(&mut self, id: u32, record: Record) {
        if let Some(spool) = &mut self.spool {
            spool.set(id as usize, record);
            return;
        }
        let old = self.record(id);
        if (old.name, old.name_len, old.parent) != (record.name, record.name_len, record.parent) {
            self.path_cache.get_mut().expect("path cache").clear();
        }
        if (id as usize) < self.mapped_count {
            self.cache_search_change(id, record);
            self.changed.insert(id, record);
        } else {
            self.records[id as usize - self.mapped_count] = record;
        }
    }

    /// Size/count-only ancestor updates use the unchanged column. Search-relevant changes
    /// own their small folded names once, rather than faulting scattered base pages per key.
    fn cache_search_change(&mut self, id: u32, record: Record) {
        if self
            .search_changed
            .get(&id)
            .is_some_and(|old| old.record.same_search(record))
        {
            return;
        }
        self.touch_mapping(
            HEADER + id as usize * size_of::<Record>(),
            size_of::<Record>(),
        );
        let base = self.base()[id as usize];
        if base.directory()
            && (base.parent != record.parent
                || (base.name != record.name && self.folded(base) != self.folded(record)))
        {
            self.path_masks_stale = true;
        }
        if base.same_search(record) {
            self.search_changed.remove(&id);
        } else {
            let name = if record.live() {
                self.folded(record).to_vec()
            } else {
                Vec::new()
            };
            self.search_changed
                .insert(id, SearchChange { record, name });
        }
    }

    /// Sequential overlay merge: no hash/tree lookup in the million-record search loop.
    pub fn range(&self, start: usize, end: usize) -> impl Iterator<Item = (u32, Record)> + '_ {
        let mut changes = self.changed.range(start as u32..end as u32).peekable();
        (start..end).map(move |id| {
            if id % 4096 == 0
                && let Some(quiet) = &self.quiet
            {
                quiet.wait();
            }
            let record = if changes.peek().is_some_and(|(key, _)| **key == id as u32) {
                *changes.next().expect("peeked").1
            } else if let Some(spool) = &self.spool {
                spool.record(id)
            } else if id < self.mapped_count {
                self.touch_mapping(HEADER + id * size_of::<Record>(), size_of::<Record>());
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
            } else if let Some(spool) = &self.spool {
                spool.record(id)
            } else if id < self.mapped_count {
                self.touch_mapping(HEADER + id * size_of::<Record>(), size_of::<Record>());
                self.base()[id]
            } else {
                self.records[id - self.mapped_count]
            };
            (id as u32, record)
        })
    }

    fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        if let Some(spool) = &self.spool {
            return spool.bytes(offset, len);
        }
        if offset < self.mapped_names {
            let start = HEADER + self.mapped_count * size_of::<Record>() + offset;
            self.touch_mapping(start, len);
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
        if let Some(spool) = &mut self.spool {
            let offset = spool.names_len as u32;
            let len = u16::try_from(name.len()).expect("name exceeds UTF-8 path limit");
            spool.append(name.as_bytes());
            let folded = if name == lower {
                0
            } else {
                spool.append(lower.as_bytes());
                u16::try_from(lower.len()).expect("folded name exceeds path limit")
            };
            return (offset, len, folded);
        }
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
        if id % 1024 == 0
            && let Some(quiet) = &self.quiet
        {
            quiet.wait();
        }
        let (name, name_len, fold_len) = self.intern(name);
        let mut p = self.record(parent);
        let record = Record {
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
        };
        if let Some(spool) = &mut self.spool {
            spool.push(record);
        } else {
            self.records.push(record);
        }
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
            // A hash collision falls back to the sibling chain, preserving correctness.
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
        let mut cache = self.path_cache.lock().expect("path cache");
        let mut components = Vec::new();
        let mut path = String::new();
        while id != 0 && id != NONE {
            if let Some(prefix) = cache.entries.get(&id) {
                path = prefix.clone();
                break;
            }
            let r = self.record(id);
            components.push((id, self.name(r)));
            id = r.parent;
        }
        for (id, name) in components.into_iter().rev() {
            if !path.is_empty() {
                path.push('\\');
            }
            path.push_str(name);
            cache.insert(id, &path);
        }
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
            size_unknown: r.attributes & SIZE_UNKNOWN != 0,
            excluded: r.attributes & EXCLUDED != 0,
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
            if id % 4096 == 0
                && let Some(quiet) = &self.quiet
            {
                quiet.wait();
            }
            let mut r = self.record(id as u32);
            if r.directory() {
                r.size = 0;
                r.files = 0;
                self.set(id as u32, r);
            }
        }
        for id in (1..self.len()).rev() {
            if id % 4096 == 0
                && let Some(quiet) = &self.quiet
            {
                quiet.wait();
            }
            let r = self.record(id as u32);
            if r.live() {
                self.adjust(r.parent, i128::from(r.size), i64::from(r.files), false);
                if r.attributes & SIZE_UNKNOWN != 0 {
                    let mut parent = self.record(r.parent);
                    parent.attributes |= SIZE_UNKNOWN;
                    self.set(r.parent, parent);
                }
            }
        }
    }

    /// Recompute only the affected ancestry; other excluded siblings must keep it unknown.
    pub fn refresh_unknown(&mut self, mut id: u32) {
        while id != NONE {
            let mut record = self.record(id);
            let mut unknown = record.attributes & EXCLUDED != 0;
            let mut child = record.first;
            while child != NONE {
                let r = self.record(child);
                unknown |= r.live() && r.attributes & SIZE_UNKNOWN != 0;
                child = r.next;
            }
            record.attributes = if unknown {
                record.attributes | SIZE_UNKNOWN
            } else {
                record.attributes & !SIZE_UNKNOWN
            };
            self.set(id, record);
            id = record.parent;
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
                r.attributes = attributes
                    | if r.directory() {
                        old.attributes & SIZE_UNKNOWN
                    } else {
                        0
                    };
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
        if r.attributes & SIZE_UNKNOWN != 0 {
            self.refresh_unknown(r.parent);
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
        if r.attributes & SIZE_UNKNOWN != 0 {
            let mut at = parent;
            while at != NONE {
                let mut record = self.record(at);
                record.attributes |= SIZE_UNKNOWN;
                self.set(at, record);
                at = record.parent;
            }
        }
        id
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if self.mapping.is_none() {
            return self.save_snapshot(path, None);
        }
        let mut streamed = self.fork()?;
        streamed.windows = Some(crate::residency::Windows::default());
        streamed.save_snapshot(path, None)
    }

    pub(crate) fn save_quiet(
        &self,
        path: &Path,
        quiet: std::sync::Arc<crate::quiet::Quiet>,
    ) -> Result<()> {
        let mut streamed = self.fork()?;
        streamed.quiet = Some(quiet);
        streamed.windows = Some(crate::residency::Windows::default());
        streamed.save_snapshot(path, None)
    }

    /// Streams a freshly scanned volume beside the other cached volumes. A four-byte ID
    /// map replaces the former private copy plus millions of deletion overlay entries.
    pub fn save_replacing_volume(&self, path: &Path, source: &Store, root: &Path) -> Result<()> {
        let excluded = self.find(&root.to_string_lossy());
        if let Some(id) = excluded {
            ensure!(
                self.record(id).parent == 0,
                "replacement must be a volume root"
            );
        }
        ensure!(
            source.record(0).first == 1,
            "replacement must contain one scanned root"
        );
        let mut streamed = self.fork()?;
        streamed.quiet = source.quiet.clone();
        streamed.windows = Some(crate::residency::Windows::default());
        streamed.save_snapshot(path, Some((source, excluded)))
    }

    fn save_snapshot(&self, path: &Path, replacement: Option<(&Store, Option<u32>)>) -> Result<()> {
        let temporary = path.with_extension("tmp");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_SEQUENTIAL_SCAN.0)
            .open(&temporary)?;
        let mut out = BufWriter::with_capacity(1024 * 1024, file);
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        // Compact tombstones and superseded names on checkpoint. IDs are internal to a
        // snapshot; preserving insertion order keeps every parent before its descendants.
        let mut ids = vec![NONE; self.len()];
        let mut count = 0u32;
        let mut names = 0u64;
        for (id, r) in self.range(0, self.len()) {
            if r.live()
                && replacement.is_none_or(|(_, excluded)| excluded != Some(id))
                && (id == 0 || ids[r.parent as usize] != NONE)
            {
                ids[id as usize] = count;
                count += 1;
                names += u64::from(r.name_len) + u64::from(r.fold_len);
            }
        }
        let kept = count;
        if let Some((source, _)) = replacement {
            count = count
                .checked_add(source.len() as u32 - 1)
                .context("too many records")?;
            for (_, r) in source.range(1, source.len()) {
                names += u64::from(r.name_len) + u64::from(r.fold_len);
            }
        }
        ensure!(names < u64::from(u32::MAX), "name blob exceeds 4 GiB");
        header[8..16].copy_from_slice(&u64::from(count).to_le_bytes());
        header[16..24].copy_from_slice(&names.to_le_bytes());
        let generation = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos() as u64;
        header[24..32].copy_from_slice(&generation.to_le_bytes());
        out.write_all(&header)?;
        let skip_dead = |mut at: u32| {
            while at != NONE && ids[at as usize] == NONE {
                at = self.record(at).next;
            }
            if at == NONE { NONE } else { ids[at as usize] }
        };
        let mut name = 0;
        for (id, mut record) in self
            .range(0, self.len())
            .filter(|(id, _)| ids[*id as usize] != NONE)
        {
            record.name = name;
            name += u32::from(record.name_len) + u32::from(record.fold_len);
            record.parent = if record.parent == NONE {
                NONE
            } else {
                ids[record.parent as usize]
            };
            record.first = skip_dead(record.first);
            record.next = skip_dead(record.next);
            if id == 0
                && let Some((source, excluded)) = replacement
            {
                let previous = excluded.map(|id| self.record(id)).unwrap_or_default();
                record.size = record
                    .size
                    .saturating_sub(previous.size)
                    .saturating_add(source.record(0).size);
                record.files = record
                    .files
                    .saturating_sub(previous.files)
                    .saturating_add(source.record(0).files);
                record.first = kept;
            }
            out.write_all(bytemuck::bytes_of(&record))?;
        }
        if let Some((source, _)) = replacement {
            let offset = kept - 1;
            for (id, mut record) in source.range(1, source.len()) {
                record.name = name;
                name += u32::from(record.name_len) + u32::from(record.fold_len);
                record.parent = if record.parent == 0 {
                    0
                } else {
                    record.parent + offset
                };
                if record.first != NONE {
                    record.first += offset;
                }
                record.next = if id == 1 {
                    skip_dead(self.record(0).first)
                } else if record.next == NONE {
                    NONE
                } else {
                    record.next + offset
                };
                out.write_all(bytemuck::bytes_of(&record))?;
            }
        }
        for (_, record) in self
            .range(0, self.len())
            .filter(|(id, _)| ids[*id as usize] != NONE)
        {
            out.write_all(self.bytes(
                record.name as usize,
                record.name_len as usize + record.fold_len as usize,
            ))?;
        }
        if let Some((source, _)) = replacement {
            for (_, record) in source.range(1, source.len()) {
                out.write_all(source.bytes(
                    record.name as usize,
                    record.name_len as usize + record.fold_len as usize,
                ))?;
            }
        }
        // The independent column is streamed, never materialized as a full private copy.
        let mut column = crate::names::Builder::default();
        for (id, record) in self
            .range(0, self.len())
            .filter(|(id, _)| ids[*id as usize] != NONE)
        {
            column.push(
                &mut out,
                ids[id as usize],
                if record.parent == NONE {
                    NONE
                } else {
                    ids[record.parent as usize]
                },
                record.attributes,
                record.modified,
                self.folded(record),
            )?;
        }
        if let Some((source, _)) = replacement {
            for (id, record) in source.range(1, source.len()) {
                column.push(
                    &mut out,
                    id + kept - 1,
                    if record.parent == 0 {
                        0
                    } else {
                        record.parent + kept - 1
                    },
                    record.attributes,
                    record.modified,
                    source.folded(record),
                )?;
            }
        }
        column.flush(&mut out)?;
        out.seek(SeekFrom::Start(32))?;
        out.write_all(&(column.bytes as u64).to_le_bytes())?;
        out.write_all(b"NAME0001")?;
        out.flush()?;
        out.get_ref().sync_all()?;
        drop(out);
        std::fs::rename(&temporary, path)
            .with_context(|| format!("checkpoint {}", path.display()))?;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path, None)
    }

    pub(crate) fn open_quiet(
        path: &Path,
        quiet: std::sync::Arc<crate::quiet::Quiet>,
    ) -> Result<Self> {
        quiet.wait();
        Self::open_inner(path, Some(quiet))
    }

    fn open_inner(path: &Path, quiet: Option<std::sync::Arc<crate::quiet::Quiet>>) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_SEQUENTIAL_SCAN.0)
            .open(path)?;
        // SAFETY: snapshot files are immutable, replaced by rename. We never truncate a mapped file.
        let mapping = unsafe { Mmap::map(&file)? };
        ensure!(
            mapping.len() >= HEADER && &mapping[..8] == MAGIC,
            "bad index header"
        );
        let count = usize::try_from(u64::from_le_bytes(mapping[8..16].try_into()?))?;
        let names = usize::try_from(u64::from_le_bytes(mapping[16..24].try_into()?))?;
        let column_bytes = usize::try_from(u64::from_le_bytes(mapping[32..40].try_into()?))?;
        let end = count
            .checked_mul(size_of::<Record>())
            .and_then(|n| n.checked_add(HEADER))
            .and_then(|n| n.checked_add(names))
            .and_then(|n| n.checked_add(column_bytes));
        ensure!(
            count > 0 && count < u32::MAX as usize && end == Some(mapping.len()),
            "bad index length"
        );
        let generation = u64::from_le_bytes(mapping[24..32].try_into()?);
        let mut store = Self {
            quiet,
            windows: Some(crate::residency::Windows::validation()),
            mapping: Some(mapping),
            source: Some(std::sync::Arc::new(file)),
            mapped_count: count,
            mapped_names: names,
            generation,
            ..Self::default()
        };
        // One streaming UTF-8 validation is much cheaper than two decoder calls for each
        // short name. Record slices are then checked against character boundaries below.
        let names_start = HEADER + count * size_of::<Record>();
        let blob_bytes =
            &store.mapping.as_ref().expect("mapped snapshot")[names_start..names_start + names];
        let mut start = 0;
        while start < blob_bytes.len() {
            if let Some(quiet) = &store.quiet {
                quiet.wait();
            }
            let mut end = (start + 4 * 1024 * 1024).min(blob_bytes.len());
            while end < blob_bytes.len() && end > start && blob_bytes[end] & 0xc0 == 0x80 {
                end -= 1;
            }
            ensure!(end > start, "invalid UTF-8 name blob");
            store.touch_mapping(names_start + start, end - start);
            std::str::from_utf8(&blob_bytes[start..end])?;
            start = end;
        }
        // SAFETY: every byte was decoded above, splitting only at UTF-8 character boundaries.
        let blob = unsafe { std::str::from_utf8_unchecked(blob_bytes) };
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
                store.touch_mapping(names_start + start, end - start + usize::from(end < names));
                ensure!(
                    blob.is_char_boundary(start)
                        && blob.is_char_boundary(middle)
                        && blob.is_char_boundary(end),
                    "name splits a UTF-8 character"
                );
            }
            Ok(())
        };
        let workers = if count >= 250_000 { 4 } else { 1 };
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
        if let Some(windows) = store.windows.take() {
            windows.release();
        }
        if column_bytes != 0 {
            let map = store.mapping.as_ref().expect("mapped snapshot");
            ensure!(&map[40..48] == b"NAME0001", "bad names column version");
            let start = names_start + names;
            let windows = crate::residency::Windows::default();
            store.column = Some(crate::names::Column::open(
                &map[start..],
                count,
                store.quiet.as_deref(),
                path.parent(),
                |offset, length| {
                    windows.touch(map.as_ptr(), map.len(), start + offset, length);
                },
            )?);
            windows.release();
            store.column_start = start;
            store.search_windows = (0..8)
                .map(|_| crate::residency::Windows::search())
                .collect();
        }
        store.windows = Some(crate::residency::Windows::query_records());
        store.quiet = None;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_prefilter_stays_conservative_after_a_directory_name_change() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-pathmask-{}.bin", std::process::id()));
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        let dir = store.insert(root, "Before", DIRECTORY, 0, 0);
        store.insert(dir, "report.txt", 128, 1, 0);
        store.save(&path).unwrap();
        let mut mapped = Store::open(&path).unwrap();
        let mut r = mapped.record(dir);
        (r.name, r.name_len, r.fold_len) = mapped.intern("After");
        mapped.set(dir, r);
        assert_eq!(
            crate::query::search(&mapped, "path:after report", 10).len(),
            1
        );
        assert!(crate::query::search(&mapped, "path:before report", 10).is_empty());
        drop(mapped);
        std::fs::remove_file(path).unwrap();
    }

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
        // Simulate a hash collision: the original spelling must still win over its hash.
        store
            .children
            .get_mut()
            .unwrap()
            .get_mut(&root)
            .unwrap()
            .insert(name_hash(b"fresh.txt"), new);
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

    #[test]
    fn incremental_checkpoint_restores_names_changes_and_sizes_and_rejects_old_generations() {
        let base =
            std::env::temp_dir().join(format!("mypaper-index-delta-{}.bin", std::process::id()));
        let delta = base.with_extension("json");
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        store.upsert(root, "old.txt", 128, 19, 1);
        store.save(&base).unwrap();
        let mut mapped = Store::open(&base).unwrap();
        mapped.remove(2);
        let dir = mapped.upsert(root, "Photos", DIRECTORY, 0, 2);
        mapped.upsert(dir, "Été.JPG", 128, 7, 3);
        mapped.save_changes(&delta).unwrap();
        let mut restored = Store::open(&base).unwrap();
        restored.load_changes(&delta).unwrap();
        assert_eq!(restored.record(root).size, 7);
        assert_eq!(restored.record(root).files, 1);
        assert!(restored.find(r"X:\old.txt").is_none());
        assert_eq!(
            restored
                .hit(restored.find(r"x:\photos\été.jpg").unwrap())
                .size,
            7
        );
        restored.save(&base).unwrap();
        let mut compact = Store::open(&base).unwrap();
        assert!(compact.load_changes(&delta).is_err());
        assert_eq!(compact.record(root).size, 7);
        drop(mapped);
        drop(restored);
        drop(compact);
        std::fs::remove_file(&base).unwrap();
        std::fs::remove_file(&delta).unwrap();
    }

    #[test]
    fn streamed_volume_replacement_keeps_other_volumes_and_old_readers() {
        let directory =
            std::env::temp_dir().join(format!("mypaper-index-stream-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("snapshot.bin");
        let mut original = Store::new();
        let c = original.insert(0, "C:", DIRECTORY, 0, 0);
        original.upsert(c, "old.txt", 128, 3, 1);
        let d = original.insert(0, "D:", DIRECTORY, 0, 0);
        original.upsert(d, "keep.txt", 128, 5, 1);
        original.save(&path).unwrap();
        let old_reader = Store::open(&path).unwrap();
        let mut source = Store::temporary(&directory.join("scan")).unwrap();
        let c = source.insert(0, "C:", DIRECTORY, 0, 0);
        let folder = source.insert(c, "folder", DIRECTORY, 0, 0);
        source.insert(folder, "new.txt", 128, 7, 2);
        source.roll_up();
        old_reader
            .save_replacing_volume(&path, &source, Path::new(r"C:\"))
            .unwrap();
        let latest = Store::open(&path).unwrap();
        assert_eq!((latest.record(0).size, latest.record(0).files), (12, 2));
        assert!(latest.find(r"C:\old.txt").is_none());
        assert!(latest.find(r"C:\folder\new.txt").is_some());
        assert!(latest.find(r"D:\keep.txt").is_some());
        assert!(old_reader.find(r"C:\old.txt").is_some());
        assert!(old_reader.find(r"C:\folder").is_none());
        Store::new()
            .save_replacing_volume(&directory.join("empty.bin"), &source, Path::new(r"C:\"))
            .unwrap();
        let empty = Store::open(&directory.join("empty.bin")).unwrap();
        assert_eq!(empty.record(0).size, 7);
        drop(empty);
        drop(source);
        assert_eq!(
            std::fs::read_dir(directory.join("scan")).unwrap().count(),
            0
        );
        drop(latest);
        drop(old_reader);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(directory.join("empty.bin")).unwrap();
        std::fs::remove_dir(directory.join("scan")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn forked_reconciliation_keeps_the_previous_mapping_queryable_during_atomic_replace() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-fork-{}.bin", std::process::id()));
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        store.upsert(root, "before.txt", 128, 3, 1);
        store.save(&path).unwrap();
        let original = Store::open(&path).unwrap();
        let mut replacement = original.fork().unwrap();
        replacement.upsert(root, "after.txt", 128, 5, 2);
        replacement.save(&path).unwrap();
        assert!(original.find(r"X:\after.txt").is_none());
        assert_eq!(original.record(root).size, 3);
        let latest = Store::open(&path).unwrap();
        assert_eq!(latest.record(root).size, 8);
        assert!(latest.find(r"X:\after.txt").is_some());
        drop(original);
        drop(replacement);
        drop(latest);
        std::fs::remove_file(&path).unwrap();
    }
}
