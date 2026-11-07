//! Name substring/word-prefix search and bounded ranking. Paths are reconstructed only for
//! candidates needing a path filter or the final top results, never for the whole index.

use crate::Hit;
use crate::store::{Store, fold};
use rayon::prelude::*;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub terms: Vec<String>,
    pub extensions: Vec<String>,
    pub paths: Vec<String>,
    pub directory: Option<bool>,
}

impl Query {
    /// Quotes may appear after a filter (`path:"Program Files"`). Terms are ANDed;
    /// comma/semicolon-separated extensions are ORed. `folder:photos` also adds a name term.
    pub fn parse(input: &str) -> Self {
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut quoted = false;
        for c in input.chars() {
            if c == '"' {
                quoted = !quoted;
            } else if c.is_whitespace() && !quoted {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(c);
            }
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        let mut result = Self::default();
        for token in tokens {
            let token = fold(&token);
            if let Some(value) = token.strip_prefix("ext:") {
                result.extensions.extend(
                    value
                        .split([',', ';'])
                        .filter(|s| !s.is_empty())
                        .map(|s| s.trim_start_matches('.').to_owned()),
                );
            } else if let Some(value) = token.strip_prefix("path:") {
                if !value.is_empty() {
                    result.paths.push(value.replace('/', "\\"));
                }
            } else if let Some(value) = token.strip_prefix("folder:") {
                result.directory = Some(true);
                if !value.is_empty() {
                    result.terms.push(value.into());
                }
            } else if let Some(value) = token.strip_prefix("file:") {
                result.directory = Some(false);
                if !value.is_empty() {
                    result.terms.push(value.into());
                }
            } else {
                result.terms.push(token);
            }
        }
        result
    }

    fn score(&self, name: &[u8], finders: &[memchr::memmem::Finder<'_>]) -> Option<u8> {
        let mut score = 3;
        for (term, finder) in self.terms.iter().zip(finders) {
            let needle = term.as_bytes();
            let at = finder.find(name)?;
            score = score.min(if name == needle {
                3
            } else if at == 0 {
                2
            } else if word_start(name, at)
                || finder
                    .find_iter(&name[at + 1..])
                    .any(|next| word_start(name, at + 1 + next))
            {
                1
            } else {
                0
            });
        }
        Some(score)
    }
}

fn word_start(name: &[u8], at: usize) -> bool {
    at == 0 || (name[at - 1].is_ascii() && !name[at - 1].is_ascii_alphanumeric())
}

type Candidate = crate::names::Candidate;

fn shard(
    store: &Store,
    query: &Query,
    start: usize,
    end: usize,
    limit: usize,
    lane: usize,
) -> Vec<Candidate> {
    // A min-heap keeps common suffix queries bounded without shifting 50 records for every
    // match (notably when enumeration is already ordered by modification time).
    let mut top = std::collections::BinaryHeap::with_capacity(limit);
    let finders: Vec<_> = query
        .terms
        .iter()
        .map(|term| memchr::memmem::Finder::new(term.as_bytes()))
        .collect();
    let path_finders: Vec<_> = query
        .paths
        .iter()
        .map(|part| memchr::memmem::Finder::new(part.as_bytes()))
        .collect();
    let mut last_parent = crate::store::NONE;
    let mut parent_path = String::new();
    let mut missing_filters = Vec::new();
    let mut visibility_parent = crate::store::NONE;
    let mut parent_online = true;
    let filter = crate::names::Filter::new(&query.terms, &query.extensions, query.directory);
    let path_masks: Vec<_> = query
        .paths
        .iter()
        .map(|path| crate::names::bloom::<2>(path.as_bytes()))
        .collect();
    store.visit_names(
        start,
        end,
        &filter,
        lane,
        |id, parent, attributes, modified, name, base| {
            let directory = attributes & crate::store::DIRECTORY != 0;
            if id == 0
                || attributes == 0
                || attributes & crate::store::EXCLUDED != 0
                || query.directory.is_some_and(|d| d != directory)
            {
                return;
            }
            if !query.extensions.is_empty() {
                if directory {
                    return;
                }
                let Some(dot) = memchr::memrchr(b'.', name) else {
                    return;
                };
                if !query
                    .extensions
                    .iter()
                    .any(|ext| ext.as_bytes() == &name[dot + 1..])
                {
                    return;
                }
            }
            let Some(score) = query.score(name, &finders) else {
                return;
            };
            if base && store.overridden(id) {
                return;
            }
            let candidate = (score, modified, std::cmp::Reverse(id));
            if top.len() == limit
                && top
                    .peek()
                    .is_some_and(|worst: &std::cmp::Reverse<Candidate>| candidate <= worst.0)
            {
                return;
            }
            // Only candidates that can enter top-k need ancestry reads. Adjacent siblings
            // share their volume status, including when an offline volume is retained.
            if parent == 0 {
                if !store.online(id) {
                    return;
                }
            } else {
                if visibility_parent != parent {
                    visibility_parent = parent;
                    parent_online = store.online(parent);
                }
                if !parent_online {
                    return;
                }
            }
            if !query.paths.is_empty() {
                if !store.path_possible(parent, name, &path_masks) {
                    return;
                }
                // Reuse adjacent siblings' parent, not one allocated full path per file. A match lies in
                // the parent, in the name, or spans their one separator. This also makes a
                // nonmatching path filter cheap across a million siblings.
                if last_parent != parent {
                    last_parent = parent;
                    parent_path = fold(&store.path(parent));
                    missing_filters.clear();
                    missing_filters.extend(
                        query
                            .paths
                            .iter()
                            .enumerate()
                            .filter_map(|(i, part)| (!parent_path.contains(part)).then_some(i)),
                    );
                }
                if !missing_filters.iter().all(|i| {
                    let part = &query.paths[*i];
                    path_finders[*i].find(name).is_some()
                        || (parent != 0
                            && part.rsplit_once('\\').is_some_and(|(before, after)| {
                                parent_path.trim_end_matches('\\').ends_with(before)
                                    && name.starts_with(after.as_bytes())
                            }))
                }) {
                    return;
                }
            }
            if top.len() < limit {
                top.push(std::cmp::Reverse(candidate));
            } else {
                *top.peek_mut().expect("full heap") = std::cmp::Reverse(candidate);
            }
        },
    );
    top.into_iter().map(|c| c.0).collect()
}

pub fn search(store: &Store, input: &str, limit: usize) -> Vec<Hit> {
    let query = Query::parse(input);
    let limit = limit.clamp(1, 100);
    let count = store.search_blocks();
    let workers = if store.len() >= 250_000 {
        std::thread::available_parallelism().map_or(2, |n| n.get().min(8))
    } else {
        1
    };
    let prefix = if query.terms.len() == 1
        && query.terms[0].len() <= 6
        && query.extensions.is_empty()
        && query.paths.is_empty()
    {
        let extra = shard(store, &query, count, count, limit, 0);
        store.short_prefix(query.terms[0].as_bytes(), query.directory, limit, &extra)
    } else {
        None
    };
    let mut candidates = if let Some(prefix) = prefix {
        prefix
    } else if workers == 1 {
        shard(store, &query, 0, count, limit, 0)
    } else {
        // Created only by a large query in the enabled helper. Workers park between requests;
        // repeated keystrokes pay neither native thread creation nor a periodic wakeup.
        static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
        let pool = POOL.get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .thread_name(|i| format!("index-search-{i}"))
                .build()
                .expect("index search workers")
        });
        pool.install(|| {
            (0..workers)
                .into_par_iter()
                .flat_map_iter(|worker| {
                    shard(
                        store,
                        &query,
                        count * worker / workers,
                        count * (worker + 1) / workers,
                        limit,
                        worker,
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    candidates.sort_unstable_by(|a, b| b.cmp(a));
    candidates
        .into_iter()
        .take(limit)
        .map(|(_, _, std::cmp::Reverse(id))| store.hit(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DIRECTORY;

    #[test]
    fn short_prefix_cache_cannot_hide_better_rows_after_deletions() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-short-{}.bin", std::process::id()));
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        for i in 0..400 {
            store.insert(root, &format!("r-file-{i}"), 128, 1, 10_000 + i);
            store.insert(root, &format!("r-dir-{i}"), DIRECTORY, 0, i);
        }
        store.save(&path).unwrap();
        let mut mapped = Store::open(&path).unwrap();
        for s in [&mut store, &mut mapped] {
            for i in 300..400 {
                let id = s.child(root, &format!("r-file-{i}")).unwrap();
                s.remove(id);
            }
        }
        for text in ["r", "r-", "folder:r", "file:r"] {
            let expected = search(&store, text, 50)
                .into_iter()
                .map(|h| h.path)
                .collect::<Vec<_>>();
            let actual = search(&mapped, text, 50)
                .into_iter()
                .map(|h| h.path)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{text}");
        }
        drop(mapped);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn persisted_prefilter_and_overlay_match_unfiltered_search() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-column-{}.bin", std::process::id()));
        let mut store = Store::new();
        let root = store.insert(0, "X:", DIRECTORY, 0, 0);
        let folder = store.insert(root, "Project 東京", DIRECTORY, 0, 0);
        for i in 0..4200 {
            store.insert(folder, &format!("Report-{i}-İstanbul.rs"), 128, 7, i);
        }
        let mut seed = 17u32;
        for i in 0..256 {
            let mut name = String::new();
            for _ in 0..180 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                name.push(char::from(b'a' + ((seed >> 24) % 26) as u8));
            }
            name.push_str("-needle.rs");
            store.insert(folder, &name, 128, 1, 10_000 + i);
        }
        store.roll_up();
        store.save(&path).unwrap();
        let mut mapped = Store::open(&path).unwrap();
        for s in [&mut store, &mut mapped] {
            let old = s.child(folder, "Report-200-İstanbul.rs").unwrap();
            s.remove(old);
            s.upsert(folder, "new report.md", 128, 9, 9000);
            s.upsert(folder, "REPORT-10-İSTANBUL.RS", 128, 9, 8000);
        }
        for query in [
            "report",
            "東京",
            "report 2",
            "İSTANBUL",
            "ext:rs",
            "ext:rs,md report",
            "folder:project",
            "file:r",
            "path:project report",
            "path:missing report",
            "new",
            "200-",
            "r",
            "",
            "🦀",
            "needle",
            "needle ext:rs",
            "path:東京 needle",
            "path:project/ needle",
        ] {
            let expected = search(&store, query, 100)
                .into_iter()
                .map(|h| (h.path, h.modified))
                .collect::<Vec<_>>();
            let actual = search(&mapped, query, 100)
                .into_iter()
                .map(|h| (h.path, h.modified))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{query}");
        }
        drop(mapped);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parser_quotes_filters_and_ranking() {
        let q = Query::parse(r#"Report ext:PDF;txt path:"Program Files" file:"#);
        assert_eq!(q.terms, ["report"]);
        assert_eq!(q.extensions, ["pdf", "txt"]);
        assert_eq!(q.paths, ["program files"]);
        assert_eq!(q.directory, Some(false));
        let mut s = Store::new();
        let root = s.insert(0, "C:", DIRECTORY, 0, 0);
        let dir = s.insert(root, "Program Files", DIRECTORY, 0, 0);
        for (name, modified) in [
            ("xreport.txt", 99),
            ("My report.pdf", 90),
            ("report.txt", 50),
            ("Report", 1),
            ("report-new.txt", 80),
        ] {
            s.insert(dir, name, 128, 1, modified);
        }
        s.roll_up();
        let names: Vec<_> = search(&s, "report", 10)
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert_eq!(
            names,
            [
                "Report",
                "report-new.txt",
                "report.txt",
                "My report.pdf",
                "xreport.txt"
            ]
        );
        assert_eq!(
            search(&s, r#"report ext:pdf path:"program files" file:"#, 10).len(),
            1
        );
        assert_eq!(search(&s, r#"ext:pdf path:"files/my report""#, 10).len(), 1);
        assert!(search(&s, "report path:nonexistent", 10).is_empty());
        assert_eq!(search(&s, "folder:program", 10)[0].size, 5);
        assert!(search(&s, "report ext:jpg", 10).is_empty());
        s.insert(dir, "xreport Report", 128, 1, 89);
        let words: Vec<_> = search(&s, "report", 10)
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert!(
            words.iter().position(|s| s == "xreport Report").unwrap()
                < words.iter().position(|s| s == "xreport.txt").unwrap()
        );
    }
}
