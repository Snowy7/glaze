//! Browser state independent of HWNDs: history, ordering, visible ranges and selection.
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeSet};

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub directory: bool,
    pub size: Option<Size>,
    pub modified: u64,
    pub kind: String,
    pub hidden: bool,
}

/// Index aggregates can be lower bounds; an excluded tree has no measured byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Exact(u64),
    LowerBound(u64),
    Excluded,
}
impl Size {
    /// Search hits are current index metadata; directory listings use their latest size reply.
    pub fn resolve(entry: Option<Self>, cached: Option<Self>, indexed: bool) -> Option<Self> {
        if indexed { entry } else { cached.or(entry) }
    }
    pub fn indexed(bytes: u64, unknown: bool, excluded: bool) -> Self {
        if excluded {
            Self::Excluded
        } else if unknown {
            Self::LowerBound(bytes)
        } else {
            Self::Exact(bytes)
        }
    }
    pub fn exact(self) -> Option<u64> {
        match self {
            Self::Exact(bytes) => Some(bytes),
            _ => None,
        }
    }
    pub fn bytes(self) -> u64 {
        match self {
            Self::Exact(bytes) | Self::LowerBound(bytes) => bytes,
            Self::Excluded => 0,
        }
    }
    pub fn label(self) -> String {
        match self {
            Self::Exact(bytes) => size_label(bytes),
            Self::LowerBound(bytes) => format!("≥ {}", size_label(bytes)),
            Self::Excluded => "—".into(),
        }
    }
    pub fn total(sizes: impl IntoIterator<Item = Option<Self>>) -> Self {
        let (mut bytes, mut unknown) = (0u64, false);
        let (mut any, mut all_excluded) = (false, true);
        for size in sizes {
            any = true;
            all_excluded &= size == Some(Self::Excluded);
            let value = size.map_or(0, Self::bytes);
            unknown |= bytes.checked_add(value).is_none();
            bytes = bytes.saturating_add(value);
            unknown |= size.is_none_or(|size| size.exact().is_none());
        }
        if any && all_excluded {
            Self::Excluded
        } else if unknown {
            Self::LowerBound(bytes)
        } else {
            Self::Exact(bytes)
        }
    }
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
        // Size completeness never reverses with direction: unknowns always follow exact sizes.
        let completeness = if self.column == Column::Size {
            a.size
                .and_then(Size::exact)
                .is_none()
                .cmp(&b.size.and_then(Size::exact).is_none())
        } else {
            Ordering::Equal
        };
        completeness
            .then_with(|| b.directory.cmp(&a.directory))
            .then_with(|| {
                let order = match self.column {
                    Column::Name => natural(&a.name, &b.name),
                    Column::Modified => a.modified.cmp(&b.modified),
                    Column::Type => a.kind.cmp(&b.kind),
                    Column::Size => a.size.map(Size::bytes).cmp(&b.size.map(Size::bytes)),
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

#[derive(Default, Clone)]
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

#[derive(Default, Clone)]
pub struct Selection {
    pub items: BTreeSet<usize>,
    pub focus: Option<usize>,
    anchor: Option<usize>,
}
impl Selection {
    /// Preserve selection identities when a directory, sort or search replaces its rows.
    pub fn remap(&mut self, old: &[Entry], new: &[Entry]) {
        let positions: std::collections::HashMap<_, _> = new
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.as_str(), i))
            .collect();
        let remap = |index: usize| {
            old.get(index)
                .and_then(|e| positions.get(e.path.as_str()))
                .copied()
        };
        self.items = self.items.iter().filter_map(|&i| remap(i)).collect();
        self.focus = self.focus.and_then(remap);
        self.anchor = self.anchor.and_then(remap);
    }
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
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub preview: bool,
}
impl Preferences {
    /// Keep old preferences valid and bound restore work from hand-edited files.
    pub fn session(&self) -> (Vec<String>, usize) {
        let paths: Vec<_> = self
            .tabs
            .iter()
            .filter(|p| !p.trim().is_empty())
            .take(32)
            .cloned()
            .collect();
        if paths.is_empty() {
            (vec!["shell:profile".into()], 0)
        } else {
            let active = self.active_tab.min(paths.len() - 1);
            (paths, active)
        }
    }
    pub fn load() -> Self {
        glaze_config::dir()
            .ok()
            .and_then(|dir| std::fs::read(dir.join("files.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let dir = glaze_config::dir()?;
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
    fn selection_replacement_preserves_focus_and_range_by_path() {
        let rows = |names: &[&str]| {
            names
                .iter()
                .map(|name| Entry {
                    path: format!(r"C:\scratch\{name}"),
                    name: (*name).into(),
                    directory: false,
                    size: None,
                    modified: 0,
                    kind: String::new(),
                    hidden: false,
                })
                .collect::<Vec<_>>()
        };
        let old = rows(&["a.txt", "b.exe", "c.txt"]);
        let refreshed = rows(&["0setup.msi", "a.txt", "b.exe", "c.txt"]);
        let mut selection = Selection::default();
        selection.choose(2, false, false);
        selection.remap(&old, &refreshed);
        assert_eq!(selection.items, BTreeSet::from([3]));
        assert_eq!(
            selection.focus.map(|i| refreshed[i].name.as_str()),
            Some("c.txt"),
            "Enter and preview must follow the highlighted path"
        );
        selection.choose(2, false, true);
        assert_eq!(
            selection.items,
            BTreeSet::from([2, 3]),
            "Shift range must start at c.txt after insertion"
        );

        // Sort/search can reorder the focus independently of the first selected row.
        let sorted = rows(&["c.txt", "b.exe", "a.txt", "0setup.msi"]);
        selection.remap(&refreshed, &sorted);
        assert_eq!(selection.focus, Some(1));
        selection.choose(2, false, true);
        assert_eq!(selection.items, BTreeSet::from([0, 1, 2]));

        let filtered = rows(&["b.exe"]);
        selection.remap(&sorted, &filtered);
        assert_eq!(selection.items, BTreeSet::from([0]));
        assert_eq!(
            selection.focus, None,
            "a removed focus must never open another row"
        );
        assert_eq!(selection.anchor, None);
        selection.choose(0, false, true);
        assert_eq!(selection.items, BTreeSet::from([0]));
    }
    #[test]
    fn indexed_sizes_labels_bars_and_selection_totals_preserve_uncertainty() {
        assert_eq!(
            Size::resolve(Some(Size::LowerBound(10)), Some(Size::Exact(10)), true),
            Some(Size::LowerBound(10))
        );
        assert_eq!(
            Size::resolve(Some(Size::Exact(10)), Some(Size::Excluded), false),
            Some(Size::Excluded)
        );
        assert_eq!(Size::indexed(1024, false, false), Size::Exact(1024));
        assert_eq!(Size::indexed(1024, true, false).label(), "≥ 1.0 KB");
        assert_eq!(Size::indexed(999, false, true), Size::Excluded);
        assert_eq!(Size::Excluded.label(), "—");
        assert_eq!(Size::LowerBound(1024).exact(), None);
        assert_eq!(Size::Excluded.exact(), None);
        assert_eq!(Size::Exact(0).exact(), Some(0));
        assert_eq!(
            Size::total([Some(Size::Exact(10)), Some(Size::Exact(20))]),
            Size::Exact(30)
        );
        assert_eq!(
            Size::total([
                Some(Size::Exact(10)),
                Some(Size::LowerBound(20)),
                Some(Size::Excluded)
            ]),
            Size::LowerBound(30)
        );
        assert_eq!(
            Size::total([Some(Size::Exact(10)), None]),
            Size::LowerBound(10)
        );
        assert_eq!(Size::total([Some(Size::Excluded)]), Size::Excluded);
        assert_eq!(
            Size::total([Some(Size::Exact(u64::MAX)), Some(Size::Exact(1))]),
            Size::LowerBound(u64::MAX)
        );
    }

    #[test]
    fn size_sort_keeps_unknowns_after_exact_sizes_in_both_directions() {
        let entry = |name: &str, directory, size| Entry {
            path: name.into(),
            name: name.into(),
            directory,
            size,
            modified: 0,
            kind: String::new(),
            hidden: false,
        };
        for descending in [false, true] {
            let sort = Sort {
                column: Column::Size,
                descending,
            };
            let mut entries = [
                entry("excluded", true, Some(Size::Excluded)),
                entry("lower", true, Some(Size::LowerBound(100))),
                entry("missing", true, None),
                entry("small", false, Some(Size::Exact(1))),
                entry("large", false, Some(Size::Exact(5))),
            ];
            entries.sort_by(|a, b| sort.compare(a, b));
            assert!(
                entries[..2]
                    .iter()
                    .all(|e| e.size.and_then(Size::exact).is_some())
            );
            assert!(
                entries[2..]
                    .iter()
                    .all(|e| e.size.and_then(Size::exact).is_none())
            );
            assert_eq!(entries[0].name, if descending { "large" } else { "small" });
        }
    }
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

    #[test]
    fn old_preferences_restore_home_and_sessions_are_bounded() {
        let old: Preferences = serde_json::from_str(r#"{"pins":[],"view":"Details"}"#).unwrap();
        assert_eq!(old.session(), (vec!["shell:profile".into()], 0));
        let p = Preferences {
            tabs: (0..50).map(|n| format!("folder-{n}")).collect(),
            active_tab: 99,
            ..Default::default()
        };
        let (tabs, active) = p.session();
        assert_eq!((tabs.len(), active), (32, 31));
    }
}
