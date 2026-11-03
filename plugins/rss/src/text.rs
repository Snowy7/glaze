//! Turning feed text (often HTML, sometimes double-escaped) into short plain text.

/// Tags that separate words; removing them must not glue the neighbours together.
const BREAKING_TAGS: &[&str] = &[
    "p",
    "br",
    "div",
    "li",
    "ul",
    "ol",
    "tr",
    "td",
    "th",
    "table",
    "blockquote",
    "pre",
    "hr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "figure",
    "figcaption",
    "section",
    "article",
];

/// Plain text from markup: tags stripped (`script`/`style` bodies and comments dropped), entities
/// decoded, whitespace collapsed. A `<` that doesn't start a tag stays as it is.
pub fn plain_text(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let starts_tag = after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'));
        if !starts_tag {
            out.push('<');
            rest = after;
            continue;
        }

        if let Some(comment) = after.strip_prefix("!--") {
            // A comment ends at `-->`; an unterminated one swallows the rest.
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let Some(end) = after.find('>') else {
            // An unterminated tag: keep it as text.
            out.push('<');
            rest = after;
            continue;
        };
        let name: String = after
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &after[end + 1..];

        if matches!(name.as_str(), "script" | "style") && !after.starts_with('/') {
            // Skip to the closing tag, wherever its case or spacing puts it.
            let lower = rest.to_ascii_lowercase();
            rest = lower
                .find(&format!("</{name}"))
                .and_then(|close| rest[close..].find('>').map(|gt| &rest[close + gt + 1..]))
                .unwrap_or("");
        }
        if BREAKING_TAGS.contains(&name.as_str()) {
            out.push(' ');
        }
    }
    out.push_str(rest);
    collapse_whitespace(&decode_entities(&out))
}

/// Single spaces between words, nothing at the ends (non-breaking spaces count as whitespace).
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replaces `&name;`, `&#123;` and `&#x1F;` with the character; anything unknown stays as is.
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after
            .find(';')
            .filter(|semi| *semi <= 10)
            .and_then(|semi| entity(&after[..semi]).map(|c| (c, semi)));
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character for an entity name (without `&` and `;`): numeric, or one of the common named ones.
pub fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "bull" => '\u{2022}',
        "middot" => '\u{b7}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        "euro" => '\u{20ac}',
        "pound" => '\u{a3}',
        "yen" => '\u{a5}',
        "cent" => '\u{a2}',
        "deg" => '\u{b0}',
        "times" => '\u{d7}',
        "sect" => '\u{a7}',
        _ => return None,
    })
}

/// At most `max` characters; longer text is cut (at a character boundary) and ends with `…`,
/// which counts towards the limit.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}\u{2026}", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_decodes_entities() {
        assert_eq!(
            plain_text("<p>Hello <b>big</b> &amp; <a href=\"x?a=1&b=2\">small</a> world</p>"),
            "Hello big & small world"
        );
        // Paragraphs and breaks separate words; inline tags don't.
        assert_eq!(plain_text("<p>one</p><p>two</p>a<br/>b"), "one two a b");
        assert_eq!(plain_text("un<i>believ</i>able"), "unbelievable");
        assert_eq!(
            plain_text("caf&eacute; &#233; &#xE9; &hellip; &unknown; &"),
            "caf&eacute; \u{e9} \u{e9} \u{2026} &unknown; &"
        );
    }

    #[test]
    fn drops_scripts_styles_and_comments() {
        assert_eq!(
            plain_text("a<script>var x = '<b>';</script>b<style>p{}</style>c<!-- hidden -->d"),
            "abcd"
        );
        assert_eq!(plain_text("a<SCRIPT >1</Script>b"), "ab");
        // Unterminated: everything after is dropped rather than leaked.
        assert_eq!(plain_text("a<script>1"), "a");
    }

    #[test]
    fn leaves_stray_angle_brackets_alone() {
        assert_eq!(plain_text("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
        assert_eq!(plain_text("broken <b"), "broken <b");
    }

    #[test]
    fn escaped_markup_is_decoded_once() {
        // `&lt;b&gt;` in the document is a literal "<b>" in the text, after XML decoding it is
        // markup; `&amp;lt;` stays text.
        assert_eq!(plain_text("&lt;not a tag&gt;"), "<not a tag>");
    }

    #[test]
    fn collapses_whitespace_including_nbsp() {
        assert_eq!(plain_text("  a \n\t b&nbsp;&nbsp;c  "), "a b c");
    }

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly10!", 10), "exactly10!");
        assert_eq!(truncate("0123456789x", 10), "012345678\u{2026}");
        // Multi-byte characters count as one, and a trailing space is not left before the dots.
        assert_eq!(
            truncate("\u{e9}\u{e9}\u{e9}\u{e9} \u{e9}\u{e9}", 5),
            "\u{e9}\u{e9}\u{e9}\u{e9}\u{2026}"
        );
        let long = "word ".repeat(100);
        assert_eq!(truncate(long.trim(), 280).chars().count(), 280);
    }
}
