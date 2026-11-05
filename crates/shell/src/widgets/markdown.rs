//! The old notes Markdown subset, parsed as text only. Source offsets keep checkbox edits exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Plain,
    Bold,
    Italic,
    Code,
    Strike,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span<'a> {
    pub text: &'a str,
    pub emphasis: Emphasis,
}

pub fn inline(mut text: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    while !text.is_empty() {
        let matched = text.char_indices().find_map(|(at, _)| {
            let rest = &text[at..];
            for (mark, emphasis) in [
                ("**", Emphasis::Bold),
                ("`", Emphasis::Code),
                ("~~", Emphasis::Strike),
                ("*", Emphasis::Italic),
                ("_", Emphasis::Italic),
            ] {
                if mark == "_"
                    && text[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                {
                    continue;
                }
                if let Some(after) = rest.strip_prefix(mark)
                    && let Some(end) = after.find(mark).filter(|&end| end > 0)
                {
                    let tail = &after[end + mark.len()..];
                    if mark == "_"
                        && tail
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphanumeric())
                    {
                        continue;
                    }
                    return Some((at, mark, emphasis, end));
                }
            }
            None
        });
        let Some((at, mark, emphasis, end)) = matched else {
            out.push(Span {
                text,
                emphasis: Emphasis::Plain,
            });
            break;
        };
        if at > 0 {
            out.push(Span {
                text: &text[..at],
                emphasis: Emphasis::Plain,
            });
        }
        let start = at + mark.len();
        out.push(Span {
            text: &text[start..start + end],
            emphasis,
        });
        text = &text[start + end + mark.len()..];
    }
    out
}

/// Byte offset of `[ ]`'s value for both bullet kinds and indented tasks.
pub fn task_box(line: &str) -> Option<usize> {
    let leading = line.len() - line.trim_start().len();
    let text = &line[leading..];
    let bytes = text.as_bytes();
    (matches!(bytes.first(), Some(b'-' | b'*'))
        && bytes.get(1..3) == Some(b" [")
        && matches!(bytes.get(3), Some(b' ' | b'x' | b'X'))
        && bytes.get(4) == Some(&b']'))
    .then_some(leading + 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_offsets_and_inline_markers_match_old_notes() {
        for (line, offset) in [("- [ ] work", 3), ("  * [x] work", 5), ("\t- [X] work", 4)] {
            assert_eq!(task_box(line), Some(offset));
        }
        assert_eq!(task_box("text [ ]"), None);
        let spans = inline("plain **bold** *italic* `code` ~~strike~~ snake_case_word");
        assert!(
            spans
                .iter()
                .any(|s| s.emphasis == Emphasis::Bold && s.text == "bold")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.emphasis == Emphasis::Strike && s.text == "strike")
        );
        assert!(spans.iter().any(|s| s.text.contains("snake_case_word")));
    }
}
