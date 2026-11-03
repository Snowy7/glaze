//! Feed documents to entries: RSS 2.0, RSS 1.0 (RDF) and Atom, through `quick_xml` events.
//! Element names are matched loosely (lowercased, namespace prefix ignored), so odd prefixes and
//! mixed-in modules don't matter. No I/O here, so all of it is unit-tested.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::date::parse_date;
use crate::text::{entity, plain_text, truncate};

/// Longest summary, in characters (the cut one ends with `…`).
pub const SUMMARY_MAX: usize = 280;

/// One entry of a feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    /// An `http(s)` URL, or empty when the entry has none (or an unsafe one).
    pub link: String,
    /// Unix seconds, if the entry has a date we could read.
    pub published: Option<i64>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    /// The feed's own title, if it has one.
    pub title: Option<String>,
    pub entries: Vec<Entry>,
}

/// Child elements of an item/entry that we read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Link,
    Guid,
    PubDate,
    Published,
    DcDate,
    Updated,
    Summary,
    Content,
}

impl Field {
    const COUNT: usize = 9;

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "title" => Self::Title,
            "link" => Self::Link,
            "guid" => Self::Guid,
            "pubdate" => Self::PubDate,
            "published" => Self::Published,
            "date" => Self::DcDate,
            "updated" => Self::Updated,
            "description" | "summary" => Self::Summary,
            "encoded" | "content" => Self::Content,
            _ => return None,
        })
    }

    /// Markup-bearing fields are reduced to plain text; the rest are used verbatim.
    fn is_text(self) -> bool {
        matches!(self, Self::Title | Self::Summary | Self::Content)
    }
}

/// Namespaces whose `title`/`description`/`content` describe media, not the entry.
const FOREIGN_PREFIXES: &[&str] = &["media", "itunes", "yt", "googleplay", "gd", "georss"];

/// What the pending text belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    FeedTitle,
    Entry(Field),
}

/// Text being collected for one element; `depth` is the element's own nesting depth.
struct Capture {
    target: Target,
    depth: usize,
    buf: String,
}

/// An entry being read. The first non-empty value of each field wins.
#[derive(Default)]
struct Draft {
    fields: [String; Field::COUNT],
    /// Atom `<link rel="alternate" href>`, preferred over RSS's `<link>` text.
    href: String,
}

impl Draft {
    fn field(&self, field: Field) -> &str {
        &self.fields[field as usize]
    }

    fn set(&mut self, field: Field, value: String) {
        let slot = &mut self.fields[field as usize];
        if slot.is_empty() {
            *slot = value;
        }
    }

    fn finish(self) -> Option<Entry> {
        let link = [self.href.as_str(), self.field(Field::Link)]
            .into_iter()
            .chain(Some(self.field(Field::Guid)))
            .find_map(web_url)
            .unwrap_or_default();
        let title = match self.field(Field::Title) {
            "" => link.clone(),
            title => title.to_owned(),
        };
        if title.is_empty() {
            return None;
        }
        let published = [
            Field::PubDate,
            Field::Published,
            Field::DcDate,
            Field::Updated,
        ]
        .into_iter()
        .find_map(|f| parse_date(self.field(f)));
        let summary = [Field::Summary, Field::Content]
            .into_iter()
            .map(|f| self.field(f))
            .find(|text| !text.is_empty())
            .map(|text| truncate(text, SUMMARY_MAX));
        Some(Entry {
            title,
            link,
            published,
            summary,
        })
    }
}

/// The URL if it is `http(s)`: links come from the wild and end up in `href`s.
fn web_url(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme_ok = ["http://", "https://"].iter().any(|s| {
        url.get(..s.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(s))
    });
    scheme_ok.then(|| url.to_owned())
}

/// Lowercased `(prefix, local name)` of an element.
fn names(tag: &BytesStart<'_>) -> (String, String) {
    split_name(tag.name().as_ref())
}

fn split_name(raw: &str) -> (String, String) {
    let full = raw.to_ascii_lowercase();
    match full.split_once(':') {
        Some((prefix, local)) => (prefix.to_owned(), local.to_owned()),
        None => (String::new(), full),
    }
}

/// The `href` of an Atom `<link>` if it points at the entry's page (`rel` missing or `alternate`).
fn alternate_href(tag: &BytesStart<'_>) -> Option<String> {
    let mut href = None;
    let mut rel = None;
    for attr in tag.attributes().with_checks(false).flatten() {
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_or_else(|_| attr.value.to_string(), |v| v.into_owned());
        match attr.key.local_name().as_ref() {
            "href" => href = Some(value),
            "rel" => rel = Some(value),
            _ => {}
        }
    }
    rel.is_none_or(|rel| rel.eq_ignore_ascii_case("alternate"))
        .then_some(href)
        .flatten()
}

/// Parses a feed document. `Err` when it is not an RSS/Atom/RDF document. A document that turns
/// malformed (or is cut off) after some entries yields those entries.
pub fn parse(xml: &str) -> Result<Feed, String> {
    const NOT_A_FEED: &str = "not an RSS or Atom feed";
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));

    let mut feed = Feed {
        title: None,
        entries: Vec::new(),
    };
    // Lowercase local names of the open elements.
    let mut stack: Vec<String> = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut draft_depth = 0;
    let mut capture: Option<Capture> = None;
    let mut saw_root = false;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(e) if !saw_root => return Err(format!("{NOT_A_FEED} ({e})")),
            Err(e) => {
                if feed.entries.is_empty() {
                    return Err(format!("malformed feed: {e}"));
                }
                eprintln!("rss: feed ends in a parse error, keeping what was read: {e}");
                break;
            }
        };

        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                let is_start = matches!(event, Event::Start(_));
                let (prefix, name) = names(tag);
                let depth = stack.len() + 1;
                if depth == 1 {
                    if !is_start || !matches!(name.as_str(), "rss" | "feed" | "rdf") {
                        return Err(format!("{NOT_A_FEED} (root element <{name}>)"));
                    }
                    saw_root = true;
                }
                if is_start {
                    stack.push(name.clone());
                }

                if let Some(open) = capture.as_mut() {
                    // Markup inside an element we read (XHTML content): keep it as tags so
                    // `plain_text` treats it like any other markup.
                    open.buf
                        .push_str(&format!("<{name}{}>", if is_start { "" } else { "/" }));
                    continue;
                }

                let foreign = FOREIGN_PREFIXES.contains(&prefix.as_str());
                match draft.as_mut() {
                    None if matches!(name.as_str(), "item" | "entry") && is_start => {
                        draft = Some(Draft::default());
                        draft_depth = depth;
                    }
                    None => {
                        let parent = depth
                            .checked_sub(2)
                            .and_then(|i| stack.get(i))
                            .map(String::as_str);
                        let titled = name == "title"
                            && is_start
                            && !foreign
                            && matches!(parent, Some("channel" | "feed"))
                            && feed.title.is_none();
                        if titled {
                            capture = Some(Capture {
                                target: Target::FeedTitle,
                                depth,
                                buf: String::new(),
                            });
                        }
                    }
                    Some(open) if depth == draft_depth + 1 && !foreign => {
                        let href = (name == "link").then(|| alternate_href(tag)).flatten();
                        if let Some(href) = href {
                            if open.href.is_empty() {
                                open.href = href;
                            }
                        } else if is_start && let Some(field) = Field::from_name(&name) {
                            capture = Some(Capture {
                                target: Target::Entry(field),
                                depth,
                                buf: String::new(),
                            });
                        }
                    }
                    Some(_) => {}
                }
            }
            Event::End(tag) => {
                let depth = stack.len();
                let (_, name) = split_name(tag.name().as_ref());
                stack.pop();

                if let Some(open) = capture.as_mut() {
                    if depth > open.depth {
                        open.buf.push_str(&format!("</{name}>"));
                        continue;
                    }
                    if let Some(done) = capture.take() {
                        match (done.target, draft.as_mut()) {
                            (Target::FeedTitle, _) => {
                                let title = plain_text(&done.buf);
                                feed.title = Some(title).filter(|t| !t.is_empty());
                            }
                            (Target::Entry(field), Some(open)) => {
                                let value = if field.is_text() {
                                    plain_text(&done.buf)
                                } else {
                                    done.buf.trim().to_owned()
                                };
                                open.set(field, value);
                            }
                            (Target::Entry(_), None) => {}
                        }
                    }
                }
                if depth == draft_depth
                    && let Some(done) = draft.take()
                {
                    feed.entries.extend(done.finish());
                }
            }
            Event::Text(text) => {
                if let Some(open) = capture.as_mut() {
                    open.buf.push_str(&text.xml10_content());
                }
            }
            Event::CData(data) => {
                if let Some(open) = capture.as_mut() {
                    open.buf.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(name) => {
                if let Some(open) = capture.as_mut() {
                    // `&nbsp;`-style names aren't valid XML, but feeds use them; keep unknown
                    // ones as they were written.
                    match entity(name) {
                        Some(c) => open.buf.push(c),
                        None => open.buf.push_str(&format!("&{};", &**name)),
                    }
                }
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }

    if !saw_root {
        // Reached the end without ever opening a root element (empty or plain text body).
        return Err(NOT_A_FEED.to_owned());
    }
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:atom="http://www.w3.org/2005/Atom"
     xmlns:media="http://search.yahoo.com/mrss/">
  <channel>
    <title>Example &amp; Co. News</title>
    <link>https://example.com/</link>
    <atom:link href="https://example.com/feed.xml" rel="self" type="application/rss+xml"/>
    <item>
      <title><![CDATA[Cats <em>and</em> dogs: a "love" story]]></title>
      <link>https://example.com/cats?a=1&amp;b=2</link>
      <pubDate>Tue, 03 Nov 2026 10:00:00 +0100</pubDate>
      <description><![CDATA[<p>Hello&nbsp;<b>world</b> &amp; friends.</p><p>Second paragraph.</p>]]></description>
      <media:title>Wrong title</media:title>
    </item>
    <item>
      <title>Entities &amp; &#8212; &lt;stuff&gt;</title>
      <guid isPermaLink="true">https://example.com/guid-link</guid>
      <dc:date>2026-11-02T08:00:00Z</dc:date>
      <description>   </description>
      <content:encoded><![CDATA[<div>Full <i>content</i> only.</div><script>alert(1)</script>]]></content:encoded>
    </item>
    <item>
      <title>No date, escaped markup</title>
      <link>https://example.com/nodate</link>
      <description>&lt;p&gt;Escaped &lt;b&gt;markup&lt;/b&gt;&lt;/p&gt;</description>
    </item>
    <item>
      <link>javascript:alert(1)</link>
    </item>
  </channel>
</rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">
  <title type="html">Atom &amp;amp; Friends</title>
  <link rel="self" href="https://example.org/atom.xml"/>
  <updated>2026-11-03T12:00:00Z</updated>
  <entry>
    <title>First</title>
    <link rel="replies" href="https://example.org/first#comments"/>
    <link rel="alternate" href="https://example.org/first?a=1&amp;b=2"/>
    <published>2026-11-03T09:00:00.500+01:00</published>
    <updated>2026-11-03T12:00:00Z</updated>
    <summary>Short &amp;amp; sweet</summary>
    <media:group><media:title>Wrong</media:title><media:description>Nope</media:description></media:group>
  </entry>
  <entry>
    <title type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml">Second <b>bold</b> title</div></title>
    <link href="https://example.org/second"/>
    <updated>2026-11-01T00:00:00Z</updated>
    <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Para one.</p><p>Para two.</p></div></content>
  </entry>
</feed>"#;

    const RDF: &str = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel rdf:about="https://example.net/"><title>Old School</title></channel>
  <item rdf:about="https://example.net/1">
    <title>RDF item</title><link>https://example.net/1</link>
    <dc:date>2026-10-30T00:00:00+00:00</dc:date>
  </item>
</rdf:RDF>"#;

    #[test]
    fn parses_rss_2() {
        let feed = parse(RSS).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Example & Co. News"));
        // The item with only an unsafe link has no title and no usable link: dropped.
        assert_eq!(feed.entries.len(), 3);

        let first = &feed.entries[0];
        assert_eq!(first.title, "Cats and dogs: a \"love\" story");
        assert_eq!(first.link, "https://example.com/cats?a=1&b=2");
        assert_eq!(first.published, Some(1_793_696_400)); // 09:00 UTC
        assert_eq!(
            first.summary.as_deref(),
            Some("Hello world & friends. Second paragraph.")
        );

        let second = &feed.entries[1];
        assert_eq!(second.title, "Entities & \u{2014}");
        // An empty description falls back to content:encoded; the guid stands in for the link.
        assert_eq!(second.summary.as_deref(), Some("Full content only."));
        assert_eq!(second.link, "https://example.com/guid-link");
        assert_eq!(second.published, Some(1_793_606_400));

        let third = &feed.entries[2];
        assert_eq!(third.published, None);
        assert_eq!(third.summary.as_deref(), Some("Escaped markup"));
    }

    #[test]
    fn parses_atom() {
        let feed = parse(ATOM).unwrap();
        // `&amp;amp;` is a double-escaped ampersand.
        assert_eq!(feed.title.as_deref(), Some("Atom & Friends"));
        assert_eq!(feed.entries.len(), 2);

        let first = &feed.entries[0];
        assert_eq!(first.title, "First");
        assert_eq!(first.link, "https://example.org/first?a=1&b=2");
        // `published` wins over `updated`.
        assert_eq!(first.published, Some(1_793_692_800));
        assert_eq!(first.summary.as_deref(), Some("Short & sweet"));

        let second = &feed.entries[1];
        assert_eq!(second.title, "Second bold title");
        assert_eq!(second.link, "https://example.org/second");
        assert_eq!(second.published, Some(1_793_491_200));
        assert_eq!(second.summary.as_deref(), Some("Para one. Para two."));
    }

    #[test]
    fn parses_rdf() {
        let feed = parse(RDF).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Old School"));
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title, "RDF item");
        assert_eq!(feed.entries[0].link, "https://example.net/1");
        assert_eq!(feed.entries[0].published, Some(1_793_318_400));
    }

    #[test]
    fn an_empty_feed_is_still_a_feed() {
        let feed =
            parse(r#"<rss version="2.0"><channel><title>Quiet</title></channel></rss>"#).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Quiet"));
        assert!(feed.entries.is_empty());
    }

    #[test]
    fn summaries_are_cut_at_280_characters() {
        let long = "word ".repeat(200);
        let xml = format!(
            "<rss><channel><item><title>t</title><description>&lt;p&gt;{long}&lt;/p&gt;</description></item></channel></rss>"
        );
        let summary = parse(&xml).unwrap().entries.remove(0).summary.unwrap();
        assert_eq!(summary.chars().count(), 280);
        assert!(summary.ends_with('\u{2026}'));
    }

    #[test]
    fn rejects_documents_that_are_not_feeds() {
        for (doc, why) in [
            ("", "empty"),
            ("just some text", "text"),
            ("<!DOCTYPE html><html><body><p>Hi</p></body></html>", "html"),
            ("<?xml version=\"1.0\"?><svg xmlns=\"x\"/>", "other xml"),
            ("<rss><channel><title>x</titl", "cut before any entry"),
        ] {
            assert!(parse(doc).is_err(), "{why}");
        }
    }

    #[test]
    fn keeps_entries_read_before_the_document_breaks() {
        let cut = "<rss><channel><item><title>One</title></item><item><title>Tw";
        let feed = parse(cut).unwrap();
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title, "One");
        // A mismatched end tag is a parse error too.
        let broken = "<rss><channel><item><title>One</title></item><item><title>Two</b></item></channel></rss>";
        assert_eq!(parse(broken).unwrap().entries.len(), 1);
    }

    #[test]
    fn tolerates_a_bom_and_leading_whitespace_before_the_root() {
        let doc = "\u{feff}\n  <rss><channel><item><title>x</title></item></channel></rss>";
        assert_eq!(parse(doc).unwrap().entries.len(), 1);
    }
}
