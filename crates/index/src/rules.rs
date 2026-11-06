//! Case-insensitive, component-aware glob rules. Include overrides both built-in and user
//! exclusions; ancestors needed to reach an include are retained without scanning siblings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use mypaper_config::IndexConfig;

const DEFAULTS: &[&str] = &[
    "**/node_modules",
    "**/.git",
    "**/__pycache__",
    "**/.venv",
    "**/venv",
    "**/.gradle",
    "**/$recycle.bin",
    "**/system volume information",
    "*/windows/winsxs",
    "*/windows/installer",
    "**/appdata/local/temp",
    "**/appdata/local/**/cache*",
    "**/npm-cache",
    "**/pnpm-store",
    "**/.cargo/registry",
    "**/.rustup/toolchains",
    "c:/mypaper-targets",
];

#[derive(Clone)]
pub struct Rules {
    exclude: GlobSet,
    include: GlobSet,
    include_prefixes: Vec<String>,
    cache: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Index,
    Excluded,
    Traverse,
}

/// Notification batches reuse each directory's marker decision. Full scans instead pass
/// the same boolean down their work queue, so they never retain a volume-sized cache.
#[derive(Default)]
pub(crate) struct Markers(HashMap<PathBuf, bool>);

impl Markers {
    pub(crate) fn directory(&mut self, path: &Path) -> bool {
        if let Some(value) = self.0.get(path) {
            return *value;
        }
        let inherited = path.parent().is_some_and(|parent| self.directory(parent));
        let excluded = inherited || marker_directory(path, None);
        // A wide notification batch cannot grow this cache without bound.
        if self.0.len() >= 8192 {
            self.0.clear();
        }
        self.0.insert(path.to_owned(), excluded);
        excluded
    }
}

fn marker_directory(path: &Path, has_project: Option<bool>) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.eq_ignore_ascii_case("target") {
        path.join("CACHEDIR.TAG").exists() || path.join(".rustc_info.json").exists()
    } else if name.eq_ignore_ascii_case("bin") || name.eq_ignore_ascii_case("obj") {
        has_project.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| std::fs::read_dir(p).ok())
                .is_some_and(|entries| {
                    entries.filter_map(Result::ok).any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
                    })
                })
        })
    } else {
        false
    }
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

fn patterns<'a>(values: impl Iterator<Item = &'a str>) -> Result<GlobSet> {
    let mut set = GlobSetBuilder::new();
    for value in values {
        let value = value.replace('\\', "/").to_lowercase();
        // A directory rule also matches the directory itself, not only its children.
        let value = value.trim_end_matches('/').trim_end_matches("/**");
        set.add(GlobBuilder::new(value).literal_separator(true).build()?);
    }
    Ok(set.build()?)
}

fn prefix_match(set: &GlobSet, text: &str) -> bool {
    std::iter::once(text.len())
        .chain(text.match_indices('/').map(|(at, _)| at))
        .any(|end| set.is_match(&text[..end]))
}

impl Rules {
    pub fn new(config: &IndexConfig, cache: &Path) -> Result<Self> {
        Ok(Self {
            exclude: patterns(
                DEFAULTS
                    .iter()
                    .copied()
                    .chain(config.exclude.iter().map(String::as_str)),
            )?,
            include: patterns(config.include.iter().map(String::as_str))?,
            include_prefixes: config
                .include
                .iter()
                .map(|value| {
                    let value = value.replace('\\', "/").to_lowercase();
                    let at = value.find(['*', '?', '[', '{']).unwrap_or(value.len());
                    value[..at].trim_end_matches('/').to_owned()
                })
                .collect(),
            cache: normalized(cache),
        })
    }

    /// Cheap notification filter: no filesystem I/O on the kernel-buffer draining thread.
    /// Conditional Rust/.NET exclusions are resolved by scan/apply outside that thread.
    pub fn notify(&self, path: &Path) -> bool {
        let text = normalized(path);
        if !self.cache.is_empty()
            && (text == self.cache || text.starts_with(&(self.cache.clone() + "/")))
        {
            return false;
        }
        // Keep the boundary notification (create/remove of node_modules itself), so its
        // parent gains/loses the unknown-size aggregate. Drop only events below it.
        !text
            .rsplit_once('/')
            .is_some_and(|(parent, _)| prefix_match(&self.exclude, parent))
            || prefix_match(&self.include, &text)
            || self.may_include(&text)
    }

    fn may_include(&self, text: &str) -> bool {
        self.include_prefixes.iter().any(|prefix| {
            prefix.is_empty()
                || prefix == text
                || prefix.starts_with(&(text.to_owned() + "/"))
                || text.starts_with(prefix)
        })
    }

    pub fn decide(&self, path: &Path, directory: bool) -> Decision {
        self.decide_cached(path, directory, &mut Markers::default())
    }

    pub(crate) fn decide_cached(
        &self,
        path: &Path,
        directory: bool,
        markers: &mut Markers,
    ) -> Decision {
        let conditional = if directory {
            markers.directory(path)
        } else {
            path.parent()
                .is_some_and(|parent| markers.directory(parent))
        };
        self.decide_known(path, directory, conditional)
    }

    pub(crate) fn child_condition(
        path: &Path,
        directory: bool,
        inherited: bool,
        has_project: bool,
    ) -> bool {
        inherited || (directory && marker_directory(path, Some(has_project)))
    }

    pub(crate) fn decide_known(&self, path: &Path, directory: bool, conditional: bool) -> Decision {
        let text = normalized(path);
        if !self.cache.is_empty()
            && (text == self.cache || text.starts_with(&(self.cache.clone() + "/")))
        {
            return Decision::Excluded;
        }
        if prefix_match(&self.include, &text) {
            return Decision::Index;
        }
        let excluded = prefix_match(&self.exclude, &text) || conditional;
        if !excluded {
            Decision::Index
        } else if directory && self.may_include(&text) {
            Decision::Traverse
        } else {
            Decision::Excluded
        }
    }

    /// A marker changing can turn an existing ordinary folder into a build tree or back.
    pub fn marker_root(path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?.to_str()?;
        if name.eq_ignore_ascii_case("CACHEDIR.TAG")
            || name.eq_ignore_ascii_case(".rustc_info.json")
        {
            let parent = path.parent()?;
            return parent
                .file_name()?
                .eq_ignore_ascii_case("target")
                .then(|| parent.to_owned());
        }
        path.extension()
            .filter(|ext| ext.eq_ignore_ascii_case("csproj"))?;
        path.parent().map(Path::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_decisions_are_reused_per_directory_and_inherited_by_children() {
        let root = std::env::temp_dir().join(format!("mypaper-markers-{}", std::process::id()));
        let target = root.join("target");
        let bin = root.join("bin");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let tag = target.join("CACHEDIR.TAG");
        let project = root.join("app.csproj");
        std::fs::write(&tag, b"").unwrap();
        std::fs::write(&project, b"").unwrap();
        let rules = Rules::new(&IndexConfig::default(), Path::new("")).unwrap();
        let mut markers = Markers::default();
        assert_eq!(
            rules.decide_cached(&target.join("first.txt"), false, &mut markers),
            Decision::Excluded
        );
        assert_eq!(
            rules.decide_cached(&bin.join("first.dll"), false, &mut markers),
            Decision::Excluded
        );
        std::fs::remove_file(tag).unwrap();
        std::fs::remove_file(project).unwrap();
        // If each file probed the filesystem again these would now be Index. A fresh
        // batch/scan observes marker removal, while a directory's children share its decision.
        for i in 0..1000 {
            for dir in [&target, &bin] {
                let path = dir.join(format!("{i}.txt"));
                assert_eq!(
                    rules.decide_cached(&path, false, &mut markers),
                    Decision::Excluded
                );
                let inherited = Rules::child_condition(&path, false, true, false);
                assert_eq!(
                    rules.decide_known(&path, false, inherited),
                    Decision::Excluded
                );
            }
        }
        assert_eq!(
            rules.decide(&target.join("fresh.txt"), false),
            Decision::Index
        );
        assert_eq!(rules.decide(&bin.join("fresh.dll"), false), Decision::Index);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn defaults_components_overrides_and_routes() {
        let config = IndexConfig {
            exclude: vec!["D:/secret/**".into()],
            include: vec!["D:/p/node_modules/keep/**".into()],
        };
        let rules = Rules::new(&config, Path::new("C:/cache")).unwrap();
        for path in [
            "D:/p/node_modules/other/a.js",
            "C:/Windows/WinSxS/a",
            "C:/Users/me/AppData/Local/Browser/Cache_Data/x",
            "D:/secret/a",
            "C:/mypaper-targets/index/a",
        ] {
            assert_eq!(
                rules.decide(Path::new(path), false),
                Decision::Excluded,
                "{path}"
            );
        }
        assert_eq!(
            rules.decide(Path::new("D:/p/NODE_MODULES"), true),
            Decision::Traverse
        );
        assert_eq!(
            rules.decide(Path::new("D:/p/node_modules/keep/a.js"), false),
            Decision::Index
        );
        assert_eq!(
            rules.decide(Path::new("D:/p/my_node_modules/a"), false),
            Decision::Index
        );
        assert_eq!(
            rules.decide(Path::new("D:/p/target/a"), false),
            Decision::Index
        );
        assert!(!rules.notify(Path::new("C:/cache/x")));
        assert!(rules.notify(Path::new("D:/other/node_modules")));
        assert!(!rules.notify(Path::new("D:/other/node_modules/file.js")));
    }
}
