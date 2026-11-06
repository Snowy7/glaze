//! Rule selectors: which XAML elements a rule styles. CSS-like and deliberately small:
//!
//! ```text
//! Grid                 any element whose type is `Grid` (last segment) or that full name
//! #RootGrid            any element named RootGrid (`x:Name`)
//! Grid#RootGrid        both
//! *                    any element
//! A > B                B whose parent is A
//! A B                  B anywhere below A
//! ```
//!
//! Matching a type by its last segment lets one rule cover system XAML
//! (`Windows.UI.Xaml.Controls.Grid`) and WinUI 3 (`Microsoft.UI.Xaml.Controls.Grid`); a full
//! name (with dots) matches only itself.

use std::fmt;

/// One compound step: a type, a name, or both (`*` is neither).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    kind: Option<String>,
    name: Option<String>,
    /// Joined to the step before it with `>`: that one must be the direct parent.
    child_of_previous: bool,
}

/// A parsed selector. The last step is the element itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    steps: Vec<Step>,
}

/// An element as the matcher sees it: its type, its name and its parent. Implemented by small
/// copyable views (a reference, or a map plus a key).
pub trait Node: Sized {
    fn kind(&self) -> &str;
    fn name(&self) -> &str;
    fn parent(&self) -> Option<Self>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

impl std::str::FromStr for Selector {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, ParseError> {
        let bad = |why: &str| ParseError(format!("selector {text:?}: {why}"));
        let mut steps = Vec::new();
        let mut child = false;
        for token in text.replace('>', " > ").split_whitespace() {
            if token == ">" {
                if steps.is_empty() || child {
                    return Err(bad("`>` needs an element on both sides"));
                }
                child = true;
                continue;
            }
            let (kind, name) = match token.split_once('#') {
                Some((kind, name)) => (kind, Some(name)),
                None => (token, None),
            };
            let valid = |s: &str| {
                !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "._-".contains(c))
            };
            if name.is_some_and(|n| !valid(n)) || (kind != "*" && !kind.is_empty() && !valid(kind))
            {
                return Err(bad(&format!(
                    "{token:?} is not `Type`, `#Name` or `Type#Name`"
                )));
            }
            steps.push(Step {
                kind: (!kind.is_empty() && kind != "*").then(|| kind.to_owned()),
                name: name.map(str::to_owned),
                child_of_previous: child,
            });
            child = false;
        }
        if child {
            return Err(bad("`>` needs an element on both sides"));
        }
        if steps.is_empty() {
            return Err(bad("empty"));
        }
        Ok(Self { steps })
    }
}

impl Step {
    fn matches(&self, node: &impl Node) -> bool {
        self.name.as_ref().is_none_or(|n| n == node.name())
            && self
                .kind
                .as_ref()
                .is_none_or(|k| type_matches(k, node.kind()))
    }
}

/// `Grid` matches `Windows.UI.Xaml.Controls.Grid`; a dotted name only matches exactly.
fn type_matches(wanted: &str, kind: &str) -> bool {
    if wanted.contains('.') {
        wanted == kind
    } else {
        kind.rsplit('.').next() == Some(wanted)
    }
}

impl Selector {
    /// Does the selector match `node`, looking up its ancestors as needed?
    pub fn matches<N: Node>(&self, node: &N) -> bool {
        let Some((last, rest)) = self.steps.split_last() else {
            return false;
        };
        last.matches(node) && Self::ancestors_match(rest, last.child_of_previous, node)
    }

    /// How specific the selector is, CSS-style: names, then types, then steps. Rules apply least
    /// specific first, so the most specific value wins.
    pub fn specificity(&self) -> (usize, usize, usize) {
        let names = self.steps.iter().filter(|s| s.name.is_some()).count();
        let types = self.steps.iter().filter(|s| s.kind.is_some()).count();
        (names, types, self.steps.len())
    }

    /// The selector as written, normalised (`A > B#c`).
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                out.push_str(if step.child_of_previous { " > " } else { " " });
            }
            match (&step.kind, &step.name) {
                (None, None) => out.push('*'),
                (kind, name) => {
                    out.push_str(kind.as_deref().unwrap_or(""));
                    if let Some(name) = name {
                        out.push('#');
                        out.push_str(name);
                    }
                }
            }
        }
        out
    }

    /// The last step is the element itself: the type it needs, to index rules by type.
    pub fn element_type(&self) -> Option<&str> {
        let kind = self.steps.last()?.kind.as_deref()?;
        Some(kind.rsplit('.').next().unwrap_or(kind))
    }

    /// `steps` must match `node`'s ancestors, right to left. `direct`: the last of them must be
    /// `node`'s parent.
    fn ancestors_match<N: Node>(steps: &[Step], direct: bool, node: &N) -> bool {
        let Some((last, rest)) = steps.split_last() else {
            return true;
        };
        let mut ancestor = node.parent();
        while let Some(a) = ancestor {
            if last.matches(&a) && Self::ancestors_match(rest, last.child_of_previous, &a) {
                return true;
            }
            if direct {
                return false;
            }
            ancestor = a.parent();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One (type, name) pair with no parent.
    struct Lone<'a>(&'a str, &'a str);

    impl Node for &Lone<'_> {
        fn kind(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            self.1
        }
        fn parent(&self) -> Option<Self> {
            None
        }
    }

    /// Owns each level so `parent` can hand out references.
    struct Tree {
        kind: String,
        name: String,
        parent: Option<Box<Tree>>,
    }

    impl Node for &Tree {
        fn kind(&self) -> &str {
            &self.kind
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn parent(&self) -> Option<Self> {
            self.parent.as_deref()
        }
    }

    fn tree(chain: &[(&str, &str)]) -> Tree {
        let mut node = None;
        for (kind, name) in chain {
            node = Some(Box::new(Tree {
                kind: (*kind).into(),
                name: (*name).into(),
                parent: node,
            }));
        }
        *node.expect("a non-empty chain")
    }

    fn sel(text: &str) -> Selector {
        text.parse().unwrap()
    }

    const PAGE: &[(&str, &str)] = &[
        ("Windows.UI.Xaml.Controls.Frame", "RootFrame"),
        ("Windows.UI.Xaml.Controls.Grid", "RootGrid"),
        ("Windows.UI.Xaml.Controls.Border", ""),
        ("Windows.UI.Xaml.Controls.TextBlock", "Title"),
    ];

    #[test]
    fn types_match_by_last_segment_or_full_name_and_names_exactly() {
        let title = &tree(PAGE);
        assert!(sel("TextBlock").matches(&title));
        assert!(sel("Windows.UI.Xaml.Controls.TextBlock").matches(&title));
        assert!(!sel("Microsoft.UI.Xaml.Controls.TextBlock").matches(&title));
        assert!(sel("#Title").matches(&title));
        assert!(sel("TextBlock#Title").matches(&title));
        assert!(!sel("TextBlock#Other").matches(&title));
        assert!(sel("*").matches(&title));
        assert!(!sel("Block").matches(&title), "a segment, not a suffix");
    }

    #[test]
    fn ancestors_descendant_and_child() {
        let title = &tree(PAGE);
        assert!(sel("Grid TextBlock").matches(&title));
        assert!(sel("#RootFrame #Title").matches(&title));
        assert!(sel("Border > TextBlock").matches(&title));
        assert!(
            !sel("Grid > TextBlock").matches(&title),
            "Grid is the grandparent"
        );
        assert!(sel("Frame > Grid > Border > TextBlock").matches(&title));
        assert!(sel("Frame > Grid TextBlock").matches(&title));
        assert!(
            !sel("Grid Frame TextBlock").matches(&title),
            "order matters"
        );
        // `A > B C` must backtrack past a first B that isn't A's child.
        let deep = &tree(&[
            ("Grid", "A"),
            ("Border", ""),
            ("Grid", ""),
            ("Border", ""),
            ("TextBlock", ""),
        ]);
        assert!(sel("#A > Border TextBlock").matches(&deep));
        // A lone node has no ancestors to satisfy.
        let lone = &Lone("Windows.UI.Xaml.Controls.TextBlock", "Title");
        assert!(!sel("Grid TextBlock").matches(&lone));
        assert!(sel("TextBlock").matches(&lone));
    }

    #[test]
    fn bad_selectors_are_errors_and_the_element_type_is_indexed() {
        for bad in [
            "",
            "  ",
            "> Grid",
            "Grid >",
            "Grid > > Border",
            "Grid#",
            "Gr(id",
            "#a#b",
        ] {
            assert!(bad.parse::<Selector>().is_err(), "{bad:?}");
        }
        assert_eq!(sel("Frame Grid#Root").element_type(), Some("Grid"));
        assert_eq!(
            sel("Windows.UI.Xaml.Controls.Grid").element_type(),
            Some("Grid")
        );
        assert_eq!(sel("#Root").element_type(), None);
        assert_eq!(
            sel("Grid>Border").steps.len(),
            2,
            "no spaces needed around >"
        );
        assert_eq!(sel("Frame  Grid>#Root *").text(), "Frame Grid > #Root *");
    }
}
