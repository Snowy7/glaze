//! Persisted names-only blocks. A 1024-bit trigram union rejects impossible blocks before
//! paging their names. False positives are harmless: every surviving name is checked exactly.

use crate::backing::Backing;
use anyhow::{Result, ensure};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io::Write;
use std::ops::Range;
use std::path::Path;

pub(crate) const BLOCK_NAMES: usize = 32;
const BLOOM_WORDS: usize = 16;
const HEADER: usize = 8 + BLOOM_WORDS * 8;
pub(crate) type Bloom = [u64; BLOOM_WORDS];
pub(crate) type Candidate = (u8, u32, Reverse<u32>);
const PREFIX_LIMIT: usize = 100;
type PrefixHeaps = HashMap<Vec<u8>, [BinaryHeap<Reverse<Candidate>>; 2]>;
const PREFIX_KEYS: usize = 8192;

struct PrefixSketch(Vec<u8>);
impl PrefixSketch {
    const WIDTH: usize = 1 << 20;
    fn new() -> Self {
        Self(vec![0; Self::WIDTH * 4])
    }
    fn slots(hash: u32) -> impl Iterator<Item = usize> {
        (0..4).map(move |row| {
            row * Self::WIDTH + (hash.rotate_left((row * 7) as u32) as usize & (Self::WIDTH - 1))
        })
    }
    fn observe(&mut self, name: &[u8]) {
        if !name.first().is_some_and(u8::is_ascii_alphabetic) {
            return;
        }
        let mut hash = 2_166_136_261u32;
        for (i, byte) in name.iter().take(6).enumerate() {
            hash = (hash ^ u32::from(*byte)).wrapping_mul(16_777_619);
            if i >= 2 && (i + 1 == name.len() || name[i + 1] & 0xc0 != 0x80) {
                for slot in Self::slots(hash) {
                    self.0[slot] = self.0[slot].saturating_add(1);
                }
            }
        }
    }
    fn frequent(&self, prefix: &[u8]) -> bool {
        let hash = prefix.iter().fold(2_166_136_261u32, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(16_777_619)
        });
        Self::slots(hash).all(|slot| self.0[slot] >= 50)
    }
}

fn prefix_push(
    heaps: &mut PrefixHeaps,
    id: u32,
    directory: bool,
    modified: u32,
    name_len: usize,
    prefix: &[u8],
) {
    if !heaps.contains_key(prefix) {
        if heaps.len() >= PREFIX_KEYS {
            return;
        }
        heaps.insert(prefix.to_vec(), Default::default());
    }
    let candidate = (
        if name_len == prefix.len() { 3 } else { 2 },
        modified,
        Reverse(id),
    );
    let heap = &mut heaps.get_mut(prefix).expect("known prefix")[usize::from(directory)];
    if heap.len() < PREFIX_LIMIT {
        heap.push(Reverse(candidate));
    } else if heap.peek().is_some_and(|worst| candidate > worst.0) {
        *heap.peek_mut().expect("full prefix heap") = Reverse(candidate);
    }
}

fn prefix_insert(heaps: &mut PrefixHeaps, id: u32, directory: bool, modified: u32, name: &str) {
    for n in 1..=2.min(name.len()) {
        if !name.is_char_boundary(n) {
            continue;
        }
        prefix_push(
            heaps,
            id,
            directory,
            modified,
            name.len(),
            &name.as_bytes()[..n],
        );
    }
}

pub(crate) fn bloom<const N: usize>(bytes: &[u8]) -> [u64; N] {
    let mut result = [0; N];
    for triple in bytes.windows(3) {
        let value =
            u32::from(triple[0]) | (u32::from(triple[1]) << 8) | (u32::from(triple[2]) << 16);
        let bit = (value.wrapping_mul(0x9e37_79b1) >> (32 - (N * 64).trailing_zeros())) as usize;
        result[bit / 64] |= 1 << (bit % 64);
    }
    result
}

pub(crate) fn union<const N: usize>(into: &mut [u64; N], from: &[u64; N]) {
    for (a, b) in into.iter_mut().zip(from) {
        *a |= b;
    }
}

fn contains<const N: usize>(available: &[u64; N], required: &[u64; N]) -> bool {
    available.iter().zip(required).all(|(a, r)| a & r == *r)
}

pub(crate) struct Builder {
    data: Vec<u8>,
    count: usize,
    filter: Bloom,
    pub bytes: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            data: Vec::with_capacity(4096),
            count: 0,
            filter: [0; BLOOM_WORDS],
            bytes: 0,
        }
    }
}

impl Builder {
    pub fn push(
        &mut self,
        out: &mut impl Write,
        id: u32,
        parent: u32,
        attributes: u32,
        modified: u32,
        name: &[u8],
    ) -> Result<()> {
        self.data.extend_from_slice(&id.to_le_bytes());
        self.data.extend_from_slice(&parent.to_le_bytes());
        self.data.extend_from_slice(&modified.to_le_bytes());
        let kind = (u32::from(attributes & crate::store::DIRECTORY != 0) << 31)
            | (u32::from(attributes & crate::store::EXCLUDED != 0) << 30);
        self.data
            .extend_from_slice(&(name.len() as u32 | kind).to_le_bytes());
        self.data.extend_from_slice(name);
        union(&mut self.filter, &bloom(name));
        self.count += 1;
        if self.count == BLOCK_NAMES {
            self.flush(out)?;
        }
        Ok(())
    }
    pub fn flush(&mut self, out: &mut impl Write) -> Result<()> {
        if self.count == 0 {
            return Ok(());
        }
        let length = HEADER + self.data.len();
        out.write_all(&(length as u32).to_le_bytes())?;
        out.write_all(&(self.count as u32).to_le_bytes())?;
        out.write_all(bytemuck::cast_slice(&self.filter))?;
        out.write_all(&self.data)?;
        self.bytes += length;
        self.count = 0;
        self.filter.fill(0);
        self.data.clear();
        Ok(())
    }
}

pub(crate) struct Filter {
    small: Vec<(usize, u64)>,
    wide: Vec<(usize, u64)>,
    pub extensions: u64,
    pub directory: Option<bool>,
}

impl Filter {
    pub fn new(terms: &[String], extensions: &[String], directory: Option<bool>) -> Self {
        let mut small = [0; 16];
        let mut wide = [0; 64];
        for term in terms {
            union(&mut small, &bloom(term.as_bytes()));
            union(&mut wide, &bloom(term.as_bytes()));
        }
        Self {
            small: small
                .into_iter()
                .enumerate()
                .filter(|(_, word)| *word != 0)
                .collect(),
            wide: wide
                .into_iter()
                .enumerate()
                .filter(|(_, word)| *word != 0)
                .collect(),
            extensions: extensions
                .iter()
                .fold(0, |mask, ext| mask | extension_mask(ext.as_bytes())),
            directory,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Directory {
    mask: [u64; 2],
    id: u32,
    tail: [u8; 2],
    tail_len: u8,
    _padding: u8,
}

impl Directory {
    fn boundary(&self, name: &[u8]) -> [u64; 2] {
        let mut bytes = [0; 5];
        let n = usize::from(self.tail_len);
        bytes[..n].copy_from_slice(&self.tail[..n]);
        bytes[n] = b'\\';
        let head = name.len().min(2);
        bytes[n + 1..n + 1 + head].copy_from_slice(&name[..head]);
        bloom(&bytes[..n + 1 + head])
    }
    fn with_name(&self, name: &[u8]) -> [u64; 2] {
        let mut mask = self.mask;
        union(&mut mask, &bloom(name));
        union(&mut mask, &self.boundary(name));
        mask
    }
    fn new(id: u32, name: &[u8], parent: Option<&Self>, root: bool) -> Self {
        let mask = if root {
            bloom(name)
        } else {
            parent.map_or([u64::MAX; 2], |parent| parent.with_name(name))
        };
        let (tail, tail_len) = if name.len() >= 2 {
            ([name[name.len() - 2], name[name.len() - 1]], 2)
        } else if name.len() == 1 && !root {
            ([b'\\', name[0]], 2)
        } else if name.len() == 1 {
            ([name[0], 0], 1)
        } else {
            ([0, 0], 0)
        };
        Self {
            id,
            mask,
            tail,
            tail_len,
            _padding: 0,
        }
    }
}

/// One bit is enough for an extension summary: a block usually has only one or two
/// extensions. Collisions only admit extra blocks, never suppress a matching extension.
pub(crate) fn extension_mask(extension: &[u8]) -> u64 {
    let hash = extension.iter().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(16_777_619)
    });
    1 << (hash % 64)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Block {
    pub offset: usize,
    filter: Bloom,
    extensions: u64,
    wide: u32,
    kinds: u32,
}

impl Block {
    fn may_match(&self, filter: &Filter, wide: &[[u64; 64]]) -> bool {
        (filter.extensions == 0 || self.extensions & filter.extensions != 0)
            && filter
                .directory
                .is_none_or(|directory| self.kinds & if directory { 2 } else { 1 } != 0)
            && if self.wide == u32::MAX {
                filter
                    .small
                    .iter()
                    .all(|(i, required)| self.filter[*i] & required == *required)
            } else {
                filter
                    .wide
                    .iter()
                    .all(|(i, required)| wide[self.wide as usize][*i] & required == *required)
            }
    }
}

#[derive(Default)]
pub(crate) struct Column {
    pub blocks: Backing<Block>,
    wide: Backing<[u64; 64]>,
    directories: Backing<Directory>,
    prefixes: HashMap<Vec<u8>, [Range<usize>; 2]>,
    prefix_rows: Backing<PrefixRow>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PrefixRow {
    modified: u32,
    id: u32,
    score: u8,
    _padding: [u8; 3],
}
impl PrefixRow {
    fn new(value: Candidate) -> Self {
        Self {
            modified: value.1,
            id: value.2.0,
            score: value.0,
            _padding: [0; 3],
        }
    }
    fn candidate(&self) -> Candidate {
        (self.score, self.modified, Reverse(self.id))
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated column"))
}

impl Column {
    pub fn open(
        bytes: &[u8],
        count: usize,
        quiet: Option<&crate::quiet::Quiet>,
        backing: Option<&Path>,
        mut touch: impl FnMut(usize, usize),
    ) -> Result<Self> {
        let mut blocks = Vec::with_capacity(count.div_ceil(BLOCK_NAMES));
        let mut directories: Vec<Directory> = Vec::new();
        let mut wide = Vec::new();
        let mut prefix_heaps = PrefixHeaps::new();
        let mut sketch = PrefixSketch::new();
        let mut at = 0;
        let mut expected = 0;
        while at < bytes.len() {
            if blocks.len() % 256 == 0
                && let Some(quiet) = quiet
            {
                quiet.wait();
            }
            ensure!(bytes.len() - at >= HEADER, "truncated name block");
            touch(at, HEADER);
            let length = u32_at(bytes, at) as usize;
            let records = u32_at(bytes, at + 4) as usize;
            ensure!(
                (1..=BLOCK_NAMES).contains(&records)
                    && length >= HEADER
                    && length <= bytes.len() - at,
                "bad name block"
            );
            touch(at, length);
            let block = &bytes[at..at + length];
            let mut cursor = HEADER;
            let mut actual = [0; BLOOM_WORDS];
            let mut extensions = 0;
            let mut kinds = 0;
            for _ in 0..records {
                ensure!(block.len() - cursor >= 16, "truncated name entry");
                ensure!(
                    u32_at(block, cursor) as usize == expected,
                    "unordered name IDs"
                );
                let parent = u32_at(block, cursor + 4);
                ensure!(
                    (expected == 0 && parent == crate::store::NONE) || (parent as usize) < expected,
                    "bad names column parent"
                );
                let flags = u32_at(block, cursor + 12);
                let len = (flags & 0x3fff_ffff) as usize;
                cursor += 16;
                ensure!(len <= block.len() - cursor, "bad folded name length");
                let text = std::str::from_utf8(&block[cursor..cursor + len])?;
                union(&mut actual, &bloom(&block[cursor..cursor + len]));
                if flags & 0x8000_0000 != 0 {
                    let inherited = directories
                        .binary_search_by_key(&parent, |dir| dir.id)
                        .ok()
                        .map(|i| &directories[i]);
                    let dir = Directory::new(
                        expected as u32,
                        &block[cursor..cursor + len],
                        inherited,
                        parent == 0 || parent == crate::store::NONE,
                    );
                    directories.push(dir);
                }
                if flags & 0x4000_0000 == 0 {
                    if expected != 0 {
                        sketch.observe(text.as_bytes());
                    }
                    if expected != 0 {
                        prefix_insert(
                            &mut prefix_heaps,
                            expected as u32,
                            flags & 0x8000_0000 != 0,
                            u32_at(block, cursor - 8),
                            text,
                        );
                    }
                    if flags & 0x8000_0000 != 0 {
                        kinds |= 2;
                    } else {
                        kinds |= 1;
                        let name = &block[cursor..cursor + len];
                        if let Some(dot) = memchr::memrchr(b'.', name) {
                            extensions |= extension_mask(&name[dot + 1..]);
                        }
                    }
                }
                cursor += len;
                expected += 1;
            }
            ensure!(
                cursor == block.len() && Self::filter(block) == actual,
                "bad name block filter"
            );
            let wide_index = if actual.iter().map(|word| word.count_ones()).sum::<u32>() > 512 {
                let mut bits = [0; 64];
                Self::visit(block, |_, _, _, _, name| union(&mut bits, &bloom(name)));
                let index = wide.len() as u32;
                wide.push(bits);
                index
            } else {
                u32::MAX
            };
            blocks.push(Block {
                offset: at,
                filter: actual,
                extensions,
                kinds,
                wide: wide_index,
            });
            at += length;
        }
        ensure!(expected == count, "incomplete names column");
        // Count-min overestimates only consume cache space. Every allocated prefix is
        // collected from the entire column; an omitted key always falls back to exact scan.
        for (i, entry) in blocks.iter().enumerate() {
            if i % 256 == 0
                && let Some(quiet) = quiet
            {
                quiet.wait();
            }
            let end = blocks.get(i + 1).map_or(bytes.len(), |next| next.offset);
            touch(entry.offset, end - entry.offset);
            Self::visit(&bytes[entry.offset..end], |id, _, attrs, modified, name| {
                if id == 0
                    || attrs & crate::store::EXCLUDED != 0
                    || !name.first().is_some_and(u8::is_ascii_alphabetic)
                {
                    return;
                }
                for n in 3..=6.min(name.len()) {
                    if (n == name.len() || name[n] & 0xc0 != 0x80) && sketch.frequent(&name[..n]) {
                        prefix_push(
                            &mut prefix_heaps,
                            id,
                            attrs & crate::store::DIRECTORY != 0,
                            modified,
                            name.len(),
                            &name[..n],
                        );
                    }
                }
            });
        }
        drop(sketch);
        let mut prefix_rows = Vec::new();
        let prefixes = prefix_heaps
            .into_iter()
            .map(|(key, heaps)| {
                let ranges = heaps.map(|heap| {
                    let mut values: Vec<_> = heap.into_iter().map(|value| value.0).collect();
                    values.sort_unstable_by(|a, b| b.cmp(a));
                    let start = prefix_rows.len();
                    prefix_rows.extend(values.into_iter().map(PrefixRow::new));
                    start..prefix_rows.len()
                });
                (key, ranges)
            })
            .collect();
        Ok(Self {
            blocks: Backing::freeze(blocks, backing, quiet)?,
            wide: Backing::freeze(wide, backing, quiet)?,
            directories: Backing::freeze(directories, backing, quiet)?,
            prefix_rows: Backing::freeze(prefix_rows, backing, quiet)?,
            prefixes,
        })
    }

    pub fn may_match(&self, index: usize, filter: &Filter) -> bool {
        self.blocks[index].may_match(filter, &self.wide)
    }

    /// Prefix matches outrank every word/substring match. Use this small top-k only when
    /// its surviving rows prove the result complete; otherwise keep the full exact search.
    pub fn short_prefix(
        &self,
        name: &[u8],
        directory: Option<bool>,
        limit: usize,
        extra: &[Candidate],
        mut replaced: impl FnMut(u32) -> bool,
    ) -> Option<Vec<Candidate>> {
        if name.is_empty() || name.len() > 6 {
            return None;
        }
        let kinds = self.prefixes.get(name)?;
        let mut values = Vec::new();
        let mut bounds = Vec::new();
        for (kind, range) in kinds.iter().enumerate() {
            if directory.is_some_and(|d| usize::from(d) != kind) {
                continue;
            }
            let candidates = &self.prefix_rows[range.clone()];
            if candidates.len() == PREFIX_LIMIT {
                bounds.push(candidates.last().expect("full prefix list").candidate());
            }
            values.extend(
                candidates
                    .iter()
                    .filter(|candidate| !replaced(candidate.id))
                    .map(PrefixRow::candidate),
            );
        }
        values.extend_from_slice(extra);
        values.sort_unstable_by(|a, b| b.cmp(a));
        let worst = *values.get(limit - 1)?;
        if worst.0 < 2 || bounds.iter().any(|bound| *bound > worst) {
            return None;
        }
        values.truncate(limit);
        Some(values)
    }

    pub fn path_possible(&self, parent: u32, name: &[u8], required: &[[u64; 2]]) -> bool {
        if parent == 0 {
            return true;
        }
        let Ok(i) = self.directories.binary_search_by_key(&parent, |dir| dir.id) else {
            return true;
        };
        let mask = self.directories[i].with_name(name);
        required.iter().all(|needed| contains(&mask, needed))
    }

    fn filter(block: &[u8]) -> Bloom {
        std::array::from_fn(|i| {
            u64::from_le_bytes(block[8 + i * 8..16 + i * 8].try_into().expect("filter"))
        })
    }

    pub fn visit(bytes: &[u8], mut visit: impl FnMut(u32, u32, u32, u32, &[u8])) {
        let mut at = HEADER;
        for _ in 0..u32_at(bytes, 4) {
            let id = u32_at(bytes, at);
            let parent = u32_at(bytes, at + 4);
            let modified = u32_at(bytes, at + 8);
            let kind = u32_at(bytes, at + 12);
            let attributes =
                128 | if kind & 0x8000_0000 != 0 {
                    crate::store::DIRECTORY
                } else {
                    0
                } | if kind & 0x4000_0000 != 0 {
                    crate::store::EXCLUDED
                } else {
                    0
                };
            let length = (kind & 0x3fff_ffff) as usize;
            at += 16;
            visit(id, parent, attributes, modified, &bytes[at..at + length]);
            at += length;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_filter_preserves_unicode_and_cross_separator_matches() {
        let root = Directory::new(1, b"c:", None, true);
        let project = Directory::new(2, b"project", Some(&root), false);
        let folder = Directory::new(3, "東京".as_bytes(), Some(&project), false);
        let mask = folder.with_name("é-报告.txt".as_bytes());
        let full = "c:\\project\\東京\\é-报告.txt".as_bytes();
        for start in 0..=full.len() {
            for end in start..=full.len() {
                assert!(contains(&mask, &bloom(&full[start..end])));
            }
        }
    }
    #[test]
    fn prefilter_never_drops_a_substring_including_utf8() {
        for name in [
            "report-index.txt",
            "İstanbul-東京-ملف",
            "aaaaa",
            "éTÉ",
            "x",
            "",
        ] {
            let folded = name.to_lowercase();
            let available = bloom::<16>(folded.as_bytes());
            let wide = bloom::<64>(folded.as_bytes());
            for start in 0..=folded.len() {
                for end in start..=folded.len() {
                    assert!(contains(&available, &bloom(&folded.as_bytes()[start..end])));
                    assert!(contains(&wide, &bloom(&folded.as_bytes()[start..end])));
                }
            }
        }
        let mut builder = Builder::default();
        let mut bytes = Vec::new();
        for id in 0..75 {
            builder
                .push(
                    &mut bytes,
                    id,
                    if id == 0 { crate::store::NONE } else { 0 },
                    128,
                    0,
                    format!("name-{id}").as_bytes(),
                )
                .unwrap();
        }
        builder.flush(&mut bytes).unwrap();
        let column = Column::open(&bytes, 75, None, None, |_, _| {}).unwrap();
        assert_eq!(column.blocks.len(), 3);
        bytes[8] ^= 1;
        assert!(Column::open(&bytes, 75, None, None, |_, _| {}).is_err());
    }
}
