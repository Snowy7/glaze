//! Browser state independent of HWNDs: history, ordering, visible ranges and selection.
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeSet};

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub directory: bool,
    pub size: Option<u64>,
    pub modified: u64,
    pub kind: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    #[default]
    Details,
    Icons,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Column {
    #[default]
    Name,
    Modified,
    Type,
    Size,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Sort {
    pub fn compare(self, a: &Entry, b: &Entry) -> Ordering {
        // Folders stay together in either direction, like Explorer.
        b.directory.cmp(&a.directory).then_with(|| {
            let order = match self.column {
                Column::Name => natural(&a.name, &b.name),
                Column::Modified => a.modified.cmp(&b.modified),
                Column::Type => a.kind.cmp(&b.kind),
                Column::Size => a.size.cmp(&b.size),
            }
            .then_with(|| natural(&a.name, &b.name));
            if self.descending {
                order.reverse()
            } else {
                order
            }
        })
    }
}

/// Natural, case-insensitive comparison without allocating a lowercased name per comparison.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a, b);
    while !left.is_empty() && !right.is_empty() {
        let (ac, bc) = (left.chars().next().unwrap(), right.chars().next().unwrap());
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let al = left.bytes().take_while(u8::is_ascii_digit).count();
            let bl = right.bytes().take_while(u8::is_ascii_digit).count();
            let (av, bv) = (
                left[..al].trim_start_matches('0'),
                right[..bl].trim_start_matches('0'),
            );
            let order = av
                .len()
                .cmp(&bv.len())
                .then_with(|| av.cmp(bv))
                .then(al.cmp(&bl));
            if order != Ordering::Equal {
                return order;
            }
            left = &left[al..];
            right = &right[bl..];
        } else {
            let order = ac.to_lowercase().cmp(bc.to_lowercase());
            if order != Ordering::Equal {
                return order;
            }
            left = &left[ac.len_utf8()..];
            right = &right[bc.len_utf8()..];
        }
    }
    left.len().cmp(&right.len())
}

#[derive(Default)]
pub struct History {
    paths: Vec<String>,
    at: usize,
}
impl History {
    pub fn visit(&mut self, path: String) {
        if self.current() == Some(path.as_str()) {
            return;
        }
        self.paths.truncate((self.at + 1).min(self.paths.len()));
        self.paths.push(path);
        self.at = self.paths.len() - 1;
    }
    pub fn current(&self) -> Option<&str> {
        self.paths.get(self.at).map(String::as_str)
    }
    pub fn can_back(&self) -> bool {
        self.at > 0
    }
    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.paths.len()
    }
    pub fn go(&mut self, forward: bool) -> Option<String> {
        if forward && self.can_forward() {
            self.at += 1;
        } else if !forward && self.can_back() {
            self.at -= 1;
        } else {
            return None;
        }
        self.current().map(str::to_owned)
    }
}

#[derive(Default)]
pub struct Selection {
    pub items: BTreeSet<usize>,
    pub focus: Option<usize>,
    anchor: Option<usize>,
}
impl Selection {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn choose(&mut self, index: usize, control: bool, shift: bool) {
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            if !control {
                self.items.clear();
            }
            self.items.extend(anchor.min(index)..=anchor.max(index));
        } else {
            if !control {
                self.items.clear();
            }
            if !self.items.insert(index) && control {
                self.items.remove(&index);
            }
            self.anchor = Some(index);
        }
        self.focus = Some(index);
    }
}

/// Only paint the intersecting rows, plus one row at each edge. Cost is independent of count.
pub fn visible_range(count: usize, offset: f32, height: f32, row: f32) -> std::ops::Range<usize> {
    let start = (offset / row).floor().max(0.0) as usize;
    let end = ((offset + height) / row).ceil().max(0.0) as usize + 1;
    start.saturating_sub(1).min(count)..end.min(count)
}

pub fn size_label(size: u64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = size as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub pins: Vec<String>,
    pub view: View,
    pub widths: Vec<f32>,
}
impl Preferences {
    pub fn load() -> Self {
        mypaper_config::dir()
            .ok()
            .and_then(|dir| std::fs::read(dir.join("files.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let dir = mypaper_config::dir()?;
        std::fs::create_dir_all(&dir)?;
        // Distinct temp names allow multiple independent browser windows to save safely.
        let temp = dir.join(format!("files.{}.tmp", std::process::id()));
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, dir.join("files.json"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_branches_and_selection_follows_control_shift() {
        let mut h = History::default();
        for path in ["a", "b", "c"] {
            h.visit(path.into());
        }
        assert_eq!(h.go(false).as_deref(), Some("b"));
        h.visit("d".into());
        assert!(!h.can_forward());
        assert_eq!(h.go(false).as_deref(), Some("b"));
        let mut s = Selection::default();
        s.choose(2, false, false);
        s.choose(5, false, true);
        assert_eq!(s.items, BTreeSet::from([2, 3, 4, 5]));
        s.choose(3, true, false);
        assert!(!s.items.contains(&3));
        s.choose(8, true, false);
        assert!(s.items.contains(&8) && s.items.contains(&2));
    }
    #[test]
    fn hundred_thousand_rows_paint_only_the_viewport_and_names_sort_naturally() {
        assert!(visible_range(100_000, 1_600_000.0, 720.0, 32.0).len() <= 26);
        assert_eq!(natural("file2.txt", "file10.txt"), Ordering::Less);
        assert_eq!(natural("HELLO", "hello"), Ordering::Equal);
        assert_eq!(natural("f0002", "f2"), Ordering::Greater);
        assert_eq!(size_label(1024), "1.0 KB");
    }
}
