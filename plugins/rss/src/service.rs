//! The plugin's logic: validate the request, load each feed through a time-limited cache, merge
//! the entries newest-first. Network access is behind [`Fetcher`] and time behind [`Clock`], so
//! this is tested with fakes.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::feed::{self, Entry};
use crate::rpc::RpcError;

/// Feeds a call may ask for.
pub const MAX_URLS: usize = 12;
const DEFAULT_LIMIT: usize = 30;
const MAX_LIMIT: usize = 100;

/// Where feed documents come from.
pub trait Fetcher: Send + Sync {
    /// The document at an `http(s)` URL, or a message for the widget author on failure.
    fn fetch(&self, url: &str) -> Result<String, String>;
}

/// Time source for the cache.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// One headline in the result of `feeds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub title: String,
    /// `http(s)` URL of the article; empty when the entry has none.
    pub link: String,
    /// The feed's title, or its host when it has none.
    pub source: String,
    /// Unix seconds; absent when the entry has no readable date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<i64>,
    /// Plain text, at most 280 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// A feed that couldn't be loaded (and had nothing cached).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedError {
    pub url: String,
    pub message: String,
}

/// The result of `feeds`.
#[derive(Debug, Serialize)]
pub struct Reply {
    pub items: Vec<Item>,
    pub errors: Vec<FeedError>,
}

struct Request {
    urls: Vec<String>,
    limit: usize,
}

/// `{ urls: string[1..=12], limit?: number }`. Duplicate URLs are loaded once; `limit` is clamped
/// to 1..=100 (default 30).
fn parse_request(params: &Value) -> Result<Request, RpcError> {
    let invalid = |what: &str| RpcError::invalid_params(what.to_owned());
    let list = params
        .get("urls")
        .and_then(Value::as_array)
        .filter(|list| (1..=MAX_URLS).contains(&list.len()))
        .ok_or_else(|| {
            invalid(&format!(
                "\"urls\" must be an array of 1 to {MAX_URLS} URLs"
            ))
        })?;

    let mut urls: Vec<String> = Vec::with_capacity(list.len());
    for value in list {
        let url = value
            .as_str()
            .map(str::trim)
            .filter(|url| host(url).is_some())
            .ok_or_else(|| invalid("every URL must be an http(s) URL"))?;
        if !urls.iter().any(|seen| seen == url) {
            urls.push(url.to_owned());
        }
    }

    let limit = match params.get("limit").filter(|v| !v.is_null()) {
        None => DEFAULT_LIMIT,
        Some(value) => value
            .as_f64()
            .map(|n| n.clamp(1.0, MAX_LIMIT as f64) as usize)
            .ok_or_else(|| invalid("\"limit\" must be a number"))?,
    };
    Ok(Request { urls, limit })
}

/// The host of an `http(s)` URL, or `None` for anything else.
fn host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, _)| h);
    (!host.is_empty()).then_some(host)
}

fn into_items(url: &str, feed: feed::Feed) -> Vec<Item> {
    let source = feed
        .title
        .or_else(|| host(url).map(|h| h.strip_prefix("www.").unwrap_or(h).to_owned()))
        .unwrap_or_else(|| url.to_owned());
    feed.entries
        .into_iter()
        .map(
            |Entry {
                 title,
                 link,
                 published,
                 summary,
             }| Item {
                title,
                link,
                source: source.clone(),
                published,
                summary,
            },
        )
        .collect()
}

struct Cached {
    items: Arc<Vec<Item>>,
    fetched: Instant,
}

pub struct Service<F: Fetcher, C: Clock> {
    fetcher: F,
    clock: C,
    ttl: Duration,
    cache: Mutex<HashMap<String, Cached>>,
    /// One lock per URL, so concurrent calls for the same feed make one request.
    flights: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl<F: Fetcher, C: Clock> Service<F, C> {
    pub fn new(fetcher: F, clock: C, ttl: Duration) -> Self {
        Self {
            fetcher,
            clock,
            ttl,
            cache: Mutex::default(),
            flights: Mutex::default(),
        }
    }

    /// Handles `feeds`.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method != "feeds" {
            return Err(RpcError::method_not_found(method));
        }
        let Request { urls, limit } = parse_request(params)?;

        // Feeds load side by side: sequentially, a few slow hosts would eat the 30 s call budget.
        let loaded: Vec<_> = thread::scope(|scope| {
            let handles: Vec<_> = urls
                .iter()
                .map(|url| scope.spawn(|| self.load(url)))
                .collect();
            handles
                .into_iter()
                .zip(&urls)
                .map(|(handle, url)| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("internal error while loading the feed".into()))
                        .map_err(|message| FeedError {
                            url: url.clone(),
                            message,
                        })
                })
                .collect()
        });

        let mut items = Vec::new();
        let mut errors = Vec::new();
        for result in loaded {
            match result {
                Ok(feed_items) => items.extend(feed_items.iter().cloned()),
                Err(error) => errors.push(error),
            }
        }
        // Newest first; stable, so undated entries (`None` sorts below any date) keep feed order.
        items.sort_by_key(|item| Reverse(item.published));
        items.truncate(limit);

        serde_json::to_value(Reply { items, errors }).map_err(|e| RpcError::internal(e.to_string()))
    }

    fn fresh(&self, url: &str) -> Option<Arc<Vec<Item>>> {
        let cache = lock(&self.cache);
        let entry = cache.get(url)?;
        let age = self.clock.now().saturating_duration_since(entry.fetched);
        (age < self.ttl).then(|| Arc::clone(&entry.items))
    }

    /// The feed's entries from the cache, or fetched. When fetching fails, a stale cached copy is
    /// better than nothing and is served without an error.
    fn load(&self, url: &str) -> Result<Arc<Vec<Item>>, String> {
        if let Some(hit) = self.fresh(url) {
            return Ok(hit);
        }
        let flight = Arc::clone(lock(&self.flights).entry(url.to_owned()).or_default());
        let _turn = lock(&flight);
        // Whoever held the lock before us has probably filled the cache.
        if let Some(hit) = self.fresh(url) {
            return Ok(hit);
        }

        let fetched = self
            .fetcher
            .fetch(url)
            .and_then(|document| feed::parse(&document));
        match fetched {
            Ok(feed) => {
                let items = Arc::new(into_items(url, feed));
                lock(&self.cache).insert(
                    url.to_owned(),
                    Cached {
                        items: Arc::clone(&items),
                        fetched: self.clock.now(),
                    },
                );
                Ok(items)
            }
            Err(message) => match lock(&self.cache).get(url) {
                Some(stale) => {
                    eprintln!("rss: {url}: {message} (serving the cached copy)");
                    Ok(Arc::clone(&stale.items))
                }
                None => Err(message),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    /// Time that only moves when told to.
    struct FakeClock {
        start: Instant,
        elapsed: Mutex<Duration>,
    }

    impl FakeClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                start: Instant::now(),
                elapsed: Mutex::default(),
            })
        }
        fn advance(&self, by: Duration) {
            *lock(&self.elapsed) += by;
        }
    }

    impl Clock for Arc<FakeClock> {
        fn now(&self) -> Instant {
            self.start + *lock(&self.elapsed)
        }
    }

    /// Serves canned documents (or errors) and counts requests.
    #[derive(Default)]
    struct FakeFetcher {
        responses: Mutex<HashMap<String, Result<String, String>>>,
        requests: AtomicUsize,
    }

    impl FakeFetcher {
        fn serve(&self, url: &str, response: Result<String, String>) {
            lock(&self.responses).insert(url.to_owned(), response);
        }
        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    impl Fetcher for Arc<FakeFetcher> {
        fn fetch(&self, url: &str) -> Result<String, String> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            lock(&self.responses)
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err("HTTP 404".into()))
        }
    }

    const TTL: Duration = Duration::from_secs(15 * 60);
    const A: &str = "https://a.example/feed.xml";
    const B: &str = "https://www.b.example/rss";
    const C: &str = "http://c.example:8080/atom";

    /// An RSS document; `entries` are `(title, pubDate)`.
    fn rss(title: Option<&str>, entries: &[(&str, Option<&str>)]) -> String {
        let title = title.map_or(String::new(), |t| format!("<title>{t}</title>"));
        let items: String = entries
            .iter()
            .map(|(title, date)| {
                let date = date.map_or(String::new(), |d| format!("<pubDate>{d}</pubDate>"));
                format!("<item><title>{title}</title><link>https://x.example/{title}</link>{date}</item>")
            })
            .collect();
        format!("<rss version=\"2.0\"><channel>{title}{items}</channel></rss>")
    }

    struct Rig {
        service: Service<Arc<FakeFetcher>, Arc<FakeClock>>,
        fetcher: Arc<FakeFetcher>,
        clock: Arc<FakeClock>,
    }

    fn rig() -> Rig {
        let fetcher = Arc::new(FakeFetcher::default());
        let clock = FakeClock::new();
        Rig {
            service: Service::new(Arc::clone(&fetcher), Arc::clone(&clock), TTL),
            fetcher,
            clock,
        }
    }

    impl Rig {
        fn feeds(&self, params: Value) -> Value {
            self.service.call("feeds", &params).unwrap()
        }
    }

    fn titles(reply: &Value) -> Vec<&str> {
        reply["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["title"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn merges_feeds_newest_first_with_undated_entries_last() {
        let rig = rig();
        rig.fetcher.serve(
            A,
            Ok(rss(
                Some("Feed A"),
                &[
                    ("a-undated", None),
                    ("a-old", Some("Mon, 02 Nov 2026 10:00:00 GMT")),
                    ("a-new", Some("Tue, 03 Nov 2026 12:00:00 GMT")),
                ],
            )),
        );
        rig.fetcher.serve(
            B,
            Ok(rss(
                None,
                &[
                    ("b-undated", None),
                    ("b-mid", Some("Tue, 03 Nov 2026 08:00:00 GMT")),
                ],
            )),
        );
        let reply = rig.feeds(json!({ "urls": [A, B] }));
        assert_eq!(
            titles(&reply),
            ["a-new", "b-mid", "a-old", "a-undated", "b-undated"]
        );
        assert_eq!(reply["errors"], json!([]));

        // Source: the feed's title, else its host without `www.`.
        assert_eq!(reply["items"][0]["source"], "Feed A");
        assert_eq!(reply["items"][1]["source"], "b.example");
        // Optional fields are left out rather than null.
        let undated = &reply["items"][3];
        assert!(undated.get("published").is_none() && undated.get("summary").is_none());
        assert_eq!(reply["items"][0]["published"], 1_793_707_200);
        assert_eq!(reply["items"][0]["link"], "https://x.example/a-new");
    }

    #[test]
    fn limit_defaults_to_30_and_is_clamped() {
        let rig = rig();
        let many: Vec<(String, String)> = (0..150)
            .map(|i| {
                (
                    format!("t{i:03}"),
                    format!("Tue, 03 Nov 2026 {:02}:{:02}:00 GMT", i / 60, i % 60),
                )
            })
            .collect();
        let entries: Vec<(&str, Option<&str>)> = many
            .iter()
            .map(|(t, d)| (t.as_str(), Some(d.as_str())))
            .collect();
        rig.fetcher.serve(A, Ok(rss(Some("A"), &entries)));

        let count = |params: Value| rig.feeds(params)["items"].as_array().unwrap().len();
        assert_eq!(count(json!({ "urls": [A] })), 30);
        assert_eq!(count(json!({ "urls": [A], "limit": 5 })), 5);
        assert_eq!(count(json!({ "urls": [A], "limit": 0 })), 1);
        assert_eq!(count(json!({ "urls": [A], "limit": -4 })), 1);
        assert_eq!(count(json!({ "urls": [A], "limit": 5000 })), 100);
        assert_eq!(count(json!({ "urls": [A], "limit": null })), 30);
        // Newest first: the last entry has the latest time.
        let reply = rig.feeds(json!({ "urls": [A], "limit": 2 }));
        assert_eq!(titles(&reply), ["t149", "t148"]);
    }

    #[test]
    fn a_failing_feed_is_reported_without_failing_the_call() {
        let rig = rig();
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("one", None)])));
        rig.fetcher.serve(B, Err("connection refused".into()));
        rig.fetcher
            .serve(C, Ok("<html><body>Not a feed</body></html>".into()));
        let reply = rig.feeds(json!({ "urls": [A, B, C] }));
        assert_eq!(titles(&reply), ["one"]);
        let errors = reply["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors[0],
            json!({ "url": B, "message": "connection refused" })
        );
        assert_eq!(errors[1]["url"], C);
        assert!(
            errors[1]["message"]
                .as_str()
                .unwrap()
                .contains("not an RSS or Atom feed")
        );
    }

    #[test]
    fn when_every_feed_fails_the_call_still_succeeds() {
        let rig = rig();
        let reply = rig.feeds(json!({ "urls": [A, B] }));
        assert_eq!(reply["items"], json!([]));
        assert_eq!(reply["errors"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn feeds_are_cached_for_fifteen_minutes_and_loaded_once_per_call() {
        let rig = rig();
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("one", None)])));
        // The same URL twice in one call is one request.
        rig.feeds(json!({ "urls": [A, A] }));
        assert_eq!(rig.fetcher.requests(), 1);

        rig.clock.advance(TTL - Duration::from_secs(1));
        rig.feeds(json!({ "urls": [A] }));
        assert_eq!(rig.fetcher.requests(), 1);

        rig.clock.advance(Duration::from_secs(1));
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("two", None)])));
        let reply = rig.feeds(json!({ "urls": [A] }));
        assert_eq!(rig.fetcher.requests(), 2);
        assert_eq!(titles(&reply), ["two"]);
    }

    #[test]
    fn a_stale_copy_is_served_when_the_refresh_fails() {
        let rig = rig();
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("one", None)])));
        rig.feeds(json!({ "urls": [A] }));
        rig.clock.advance(TTL);

        // Network error, then a document that isn't a feed: both fall back, neither is an error.
        for broken in [Err("timed out".to_owned()), Ok("<html></html>".to_owned())] {
            rig.fetcher.serve(A, broken);
            let reply = rig.feeds(json!({ "urls": [A] }));
            assert_eq!(titles(&reply), ["one"]);
            assert_eq!(reply["errors"], json!([]));
        }
        // It really tried each time (the stale entry stays stale).
        assert_eq!(rig.fetcher.requests(), 3);

        // Recovery replaces it.
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("two", None)])));
        assert_eq!(titles(&rig.feeds(json!({ "urls": [A] }))), ["two"]);
    }

    #[test]
    fn failures_are_not_cached() {
        let rig = rig();
        rig.fetcher.serve(A, Err("boom".into()));
        rig.feeds(json!({ "urls": [A] }));
        rig.fetcher.serve(A, Ok(rss(Some("A"), &[("one", None)])));
        assert_eq!(titles(&rig.feeds(json!({ "urls": [A] }))), ["one"]);
    }

    #[test]
    fn rejects_bad_requests() {
        let rig = rig();
        let too_many: Vec<String> = (0..13)
            .map(|i| format!("https://h{i}.example/rss"))
            .collect();
        for params in [
            Value::Null,
            json!({}),
            json!({ "urls": [] }),
            json!({ "urls": "https://a.example/rss" }),
            json!({ "urls": too_many }),
            json!({ "urls": ["ftp://a.example/rss"] }),
            json!({ "urls": ["file:///etc/passwd"] }),
            json!({ "urls": ["javascript:alert(1)"] }),
            json!({ "urls": ["https://"] }),
            json!({ "urls": ["not a url"] }),
            json!({ "urls": [42] }),
            json!({ "urls": [A], "limit": "ten" }),
        ] {
            let err = rig.service.call("feeds", &params).unwrap_err();
            assert_eq!(
                (err.code, err.data_code),
                (-32602, Some("InvalidParams")),
                "{params}"
            );
        }
        assert_eq!(rig.fetcher.requests(), 0);

        // Twelve is fine.
        let twelve: Vec<String> = (0..12)
            .map(|i| format!("https://h{i}.example/rss"))
            .collect();
        assert!(
            rig.service
                .call("feeds", &json!({ "urls": twelve }))
                .is_ok()
        );

        let unknown = rig
            .service
            .call("headlines", &json!({ "urls": [A] }))
            .unwrap_err();
        assert_eq!(
            (unknown.code, unknown.data_code),
            (-32601, Some("NotFound"))
        );
    }

    #[test]
    fn extracts_hosts() {
        assert_eq!(host("https://example.com/a/b?c#d"), Some("example.com"));
        assert_eq!(
            host("HTTP://user:pw@example.com:8080/x"),
            Some("example.com")
        );
        assert_eq!(host("https://example.com?x=1"), Some("example.com"));
        assert_eq!(host("ftp://example.com"), None);
        assert_eq!(host("https:///path"), None);
        assert_eq!(host("example.com"), None);
    }
}
