//! App compatibility data, adapted only from the MIT application list (see applications.LICENSE).
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Exe,
    Class,
    Title,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Equals,
    DoesNotEqual,
    StartsWith,
    EndsWith,
    Contains,
    Regex,
    Legacy,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Matcher {
    pub field: Field,
    pub value: String,
    pub strategy: Strategy,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub float: bool,
    pub ignore: bool,
    pub layered: bool,
    pub force: bool,
    pub tray: bool,
    pub late_title: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub matches: Vec<Matcher>,
    pub options: Options,
}
pub struct Rules(Vec<(Rule, Vec<Option<Regex>>)>);
impl Rules {
    /// User rules follow the shipped defaults. Regexes are compiled once on enable.
    pub fn load() -> anyhow::Result<Self> {
        let mut rules: Vec<Rule> = serde_json::from_str(include_str!("applications.json"))?;
        let path = glaze_config::dir()?.join("tiling-rules.json");
        if path.exists() {
            rules.extend(serde_json::from_slice::<Vec<Rule>>(&std::fs::read(path)?)?);
        }
        Self::compile(rules)
    }
    fn compile(rules: Vec<Rule>) -> anyhow::Result<Self> {
        let mut compiled = Vec::new();
        for rule in rules {
            let regex = rule
                .matches
                .iter()
                .map(|m| {
                    matches!(m.strategy, Strategy::Regex)
                        .then(|| Regex::new(&m.value))
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            compiled.push((rule, regex));
        }
        Ok(Self(compiled))
    }
    pub fn resolve(&self, exe: &str, class: &str, title: &str) -> Options {
        let mut result = Options::default();
        for (rule, regex) in &self.0 {
            let matched = rule.matches.iter().zip(regex).all(|(m, regex)| {
                let haystack = match m.field {
                    Field::Exe => exe,
                    Field::Class => class,
                    Field::Title => title,
                };
                let pattern = &m.value;
                match m.strategy {
                    Strategy::Equals => haystack.eq_ignore_ascii_case(pattern),
                    Strategy::DoesNotEqual => !haystack.eq_ignore_ascii_case(pattern),
                    Strategy::StartsWith => {
                        haystack.to_lowercase().starts_with(&pattern.to_lowercase())
                    }
                    Strategy::EndsWith => {
                        haystack.to_lowercase().ends_with(&pattern.to_lowercase())
                    }
                    Strategy::Contains | Strategy::Legacy => {
                        haystack.to_lowercase().contains(&pattern.to_lowercase())
                    }
                    Strategy::Regex => regex.as_ref().is_some_and(|r| r.is_match(haystack)),
                }
            });
            if matched {
                result.float |= rule.options.float;
                result.ignore |= rule.options.ignore;
                result.layered |= rule.options.layered;
                result.force |= rule.options.force;
                result.tray |= rule.options.tray;
                result.late_title |= rule.options.late_title;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_rules_preserve_conjunctions_and_app_specific_exceptions() {
        let rules =
            Rules::compile(serde_json::from_str(include_str!("applications.json")).unwrap())
                .unwrap();
        assert!(
            rules
                .resolve("firefox.exe", "MozillaWindowClass", "Picture-in-Picture")
                .float
        );
        assert!(
            !rules
                .resolve("unrelated.exe", "NormalWindow", "Picture-in-Picture")
                .float
        );
        assert!(rules.resolve("acad.exe", "Afx:123", "Drawing").float);
        assert!(!rules.resolve("unrelated.exe", "Afx:123", "Drawing").float);
    }
}
