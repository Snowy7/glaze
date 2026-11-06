//! Persisted names-only blocks. A 1024-bit trigram union rejects impossible blocks before
//! paging their names. False positives are harmless: every surviving name is checked exactly.

use anyhow::{Result, ensure};
use std::io::Write;

pub(crate) const BLOCK_NAMES: usize = 32;
const BLOOM_WORDS: usize = 16;
const HEADER: usize = 8 + BLOOM_WORDS * 8;
pub(crate) type Bloom = [u64; BLOOM_WORDS];

pub(crate) fn bloom(bytes: &[u8]) -> Bloom {
    let mut result = [0; BLOOM_WORDS];
    for triple in bytes.windows(3) {
        let value =
            u32::from(triple[0]) | (u32::from(triple[1]) << 8) | (u32::from(triple[2]) << 16);
        let bit = (value.wrapping_mul(0x9e37_79b1) >> 22) as usize;
        result[bit / 64] |= 1 << (bit % 64);
    }
    result
}

pub(crate) fn union(into: &mut Bloom, from: &Bloom) {
    for (a, b) in into.iter_mut().zip(from) {
        *a |= b;
    }
}

fn contains(available: &Bloom, required: &Bloom) -> bool {
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

#[derive(Default)]
pub(crate) struct Filter {
    pub trigrams: Bloom,
    pub extensions: u64,
    pub directory: Option<bool>,
}

/// One bit is enough for an extension summary: a block usually has only one or two
/// extensions. Collisions only admit extra blocks, never suppress a matching extension.
pub(crate) fn extension_mask(extension: &[u8]) -> u64 {
    let hash = extension.iter().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(16_777_619)
    });
    1 << (hash % 64)
}

pub(crate) struct Block {
    pub offset: usize,
    filter: Bloom,
    extensions: u64,
    kinds: u8,
}

impl Block {
    pub fn may_match(&self, filter: &Filter) -> bool {
        contains(&self.filter, &filter.trigrams)
            && (filter.extensions == 0 || self.extensions & filter.extensions != 0)
            && filter
                .directory
                .is_none_or(|directory| self.kinds & if directory { 2 } else { 1 } != 0)
    }
}

#[derive(Default)]
pub(crate) struct Column {
    pub blocks: Vec<Block>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated column"))
}

impl Column {
    pub fn open(bytes: &[u8], count: usize, mut touch: impl FnMut(usize, usize)) -> Result<Self> {
        let mut blocks = Vec::with_capacity(count.div_ceil(BLOCK_NAMES));
        let mut at = 0;
        let mut expected = 0;
        while at < bytes.len() {
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
                std::str::from_utf8(&block[cursor..cursor + len])?;
                union(&mut actual, &bloom(&block[cursor..cursor + len]));
                if flags & 0x4000_0000 == 0 {
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
            blocks.push(Block {
                offset: at,
                filter: actual,
                extensions,
                kinds,
            });
            at += length;
        }
        ensure!(expected == count, "incomplete names column");
        Ok(Self { blocks })
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
            let available = bloom(folded.as_bytes());
            for start in 0..=folded.len() {
                for end in start..=folded.len() {
                    assert!(contains(&available, &bloom(&folded.as_bytes()[start..end])));
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
        let column = Column::open(&bytes, 75, |_, _| {}).unwrap();
        assert_eq!(column.blocks.len(), 3);
        bytes[8] ^= 1;
        assert!(Column::open(&bytes, 75, |_, _| {}).is_err());
    }
}
