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
            } else if !name[at - 1].is_ascii_alphanumeric() && name[at - 1].is_ascii() {
                1
            } else {
                0
            });
        }
        Some(score)
    }
}

type Candidate = (u8, u32, std::cmp::Reverse<u32>);

fn shard(store: &Store, query: &Query, start: usize, end: usize, limit: usize) -> Vec<Candidate> {
    // A min-heap keeps common suffix queries bounded without shifting 50 records for every
    // match (notably when enumeration is already ordered by modification time).
    let mut top = std::collections::BinaryHeap::with_capacity(limit);
    let finders: Vec<_> = query
        .terms
        .iter()
        .map(|term| memchr::memmem::Finder::new(term.as_bytes()))
        .collect();
    let mut parents = std::collections::HashMap::<u32, String>::new();
    for (id, record) in store.reverse_range(start, end) {
        if id == 0 || !record.live() || query.directory.is_some_and(|d| d != record.directory()) {
            continue;
        }
        let name = store.folded(record);
        if !query.extensions.is_empty() {
            if record.directory() {
                continue;
            }
            let Some(dot) = memchr::memrchr(b'.', name) else {
                continue;
            };
            if !query
                .extensions
                .iter()
                .any(|ext| ext.as_bytes() == &name[dot + 1..])
            {
                continue;
            }
        }
        let Some(score) = query.score(name, &finders) else {
            continue;
        };
        let candidate = (score, record.modified, std::cmp::Reverse(id));
        if top.len() == limit
            && top
                .peek()
                .is_some_and(|worst: &std::cmp::Reverse<Candidate>| candidate <= worst.0)
        {
            continue;
        }
        if !query.paths.is_empty() {
            // Cache directory paths, not one allocated full path per file. A match lies in
            // the parent, in the name, or spans their one separator. This also makes a
            // nonmatching path filter cheap across a million siblings.
            let parent = parents
                .entry(record.parent)
                .or_insert_with(|| fold(&store.path(record.parent)));
            if !query.paths.iter().all(|part| {
                parent.contains(part)
                    || memchr::memmem::find(name, part.as_bytes()).is_some()
                    || (record.parent != 0
                        && part.rsplit_once('\\').is_some_and(|(before, after)| {
                            parent.trim_end_matches('\\').ends_with(before)
                                && name.starts_with(after.as_bytes())
                        }))
            }) {
                continue;
            }
        }
        if top.len() < limit {
            top.push(std::cmp::Reverse(candidate));
        } else {
            *top.peek_mut().expect("full heap") = std::cmp::Reverse(candidate);
        }
    }
    top.into_iter().map(|c| c.0).collect()
}

pub fn search(store: &Store, input: &str, limit: usize) -> Vec<Hit> {
    let query = Query::parse(input);
    let limit = limit.clamp(1, 100);
    let count = store.len();
    let workers = if count >= 250_000 {
        std::thread::available_parallelism().map_or(2, |n| n.get().min(8))
    } else {
        1
    };
    let mut candidates = if workers == 1 {
        shard(store, &query, 0, count, limit)
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
    }
}
