//! Theme variables (docs/bars.md, "Theme variables"): a theme's bars CSS declares user-tunable
//! CSS variables, with a type, a label and a range, in one comment block; Studio renders a
//! control for each and writes the values to `bars.json` → `vars`; the host checks them against
//! the declarations and the page sets them on `:root`.
//!
//! ```css
//! /* @glaze-vars
//! {
//!   "--glow": { "type": "color", "label": "Glow", "default": "#ff2bd6" },
//!   "--glow-size": { "type": "length", "label": "Glow size", "min": 0, "max": 40, "default": 14 },
//!   "--labels": { "type": "bool", "label": "Show labels", "default": true },
//!   "--shape": { "type": "select", "label": "Shape", "options": ["soft", "sharp"], "default": "soft" }
//! }
//! */
//! ```

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::style::ColorRef;

/// What starts the declaration comment.
pub const MARKER: &str = "@glaze-vars";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VarType {
    /// `#rrggbb` or `#rrggbbaa`.
    Color,
    Number,
    /// A number with a CSS unit (`unit`, default `px`).
    Length,
    /// One of `options` (CSS keywords).
    Select,
    /// `1` or `0` in CSS, so `calc()` and `@container style(--x: 1)` can use it.
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Unit {
    #[default]
    #[serde(rename = "px")]
    Px,
    #[serde(rename = "em")]
    Em,
    #[serde(rename = "rem")]
    Rem,
    #[serde(rename = "%")]
    Percent,
    #[serde(rename = "ms")]
    Ms,
    #[serde(rename = "deg")]
    Deg,
}

impl Unit {
    fn css(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Em => "em",
            Self::Rem => "rem",
            Self::Percent => "%",
            Self::Ms => "ms",
            Self::Deg => "deg",
        }
    }
}

/// One declared variable: what Studio needs to draw its control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VarDecl {
    #[serde(rename = "type")]
    pub kind: VarType,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default)]
    pub unit: Unit,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

fn css_ident(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 32
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl VarDecl {
    /// The CSS value for `value`, or why it doesn't fit this declaration.
    pub fn css(&self, value: &Value) -> Result<String> {
        match self.kind {
            VarType::Color => {
                let text = value.as_str().context("expected a color string")?;
                match ColorRef::parse(text) {
                    Some(color) if color.as_str().starts_with('#') => Ok(color.as_str().to_owned()),
                    _ => bail!("expected #rrggbb or #rrggbbaa, got {text:?}"),
                }
            }
            VarType::Number | VarType::Length => {
                let n = value.as_f64().context("expected a number")?;
                let (lo, hi) = (self.min.unwrap_or(f64::MIN), self.max.unwrap_or(f64::MAX));
                if !(lo..=hi).contains(&n) {
                    bail!("{n} is outside {lo}..{hi}");
                }
                let unit = if self.kind == VarType::Length {
                    self.unit.css()
                } else {
                    ""
                };
                Ok(format!("{}{unit}", trim_number(n)))
            }
            VarType::Select => {
                let text = value.as_str().context("expected one of the options")?;
                if !self.options.iter().any(|o| o == text) {
                    bail!("{text:?} isn't one of {:?}", self.options);
                }
                Ok(text.to_owned())
            }
            VarType::Bool => {
                let on = value.as_bool().context("expected true or false")?;
                Ok(if on { "1" } else { "0" }.to_owned())
            }
        }
    }

    fn validate(&self, name: &str) -> Result<()> {
        if self.label.trim().is_empty() {
            bail!("{name}: a label is required");
        }
        if let (Some(lo), Some(hi)) = (self.min, self.max)
            && lo > hi
        {
            bail!("{name}: min is above max");
        }
        if self.kind == VarType::Select
            && (self.options.is_empty() || !self.options.iter().all(|o| css_ident(o)))
        {
            bail!("{name}: a select needs options that are CSS keywords");
        }
        self.css(&self.default)
            .with_context(|| format!("{name}: bad default"))
            .map(drop)
    }
}

fn trim_number(n: f64) -> String {
    let text = format!("{n:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The variables a style sheet declares (none without a [`MARKER`] comment).
pub fn declared(css: &str) -> Result<BTreeMap<String, VarDecl>> {
    let Some(start) = css.find(MARKER) else {
        return Ok(BTreeMap::new());
    };
    let body = &css[start + MARKER.len()..];
    let end = body
        .find("*/")
        .context("the @glaze-vars comment isn't closed")?;
    let decls: BTreeMap<String, VarDecl> =
        serde_json::from_str(body[..end].trim()).context("in the @glaze-vars block")?;
    for (name, decl) in &decls {
        let bare = name.strip_prefix("--").unwrap_or_default();
        if !css_ident(bare) {
            bail!("{name:?} isn't a CSS variable name (--name)");
        }
        decl.validate(name)?;
    }
    Ok(decls)
}

/// The CSS value of every declared variable: the user's (when it fits) or the default. Values
/// that don't fit and values for undeclared variables are reported, not applied.
pub fn resolve(
    decls: &BTreeMap<String, VarDecl>,
    values: &BTreeMap<String, Value>,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut problems = Vec::new();
    let resolved = decls
        .iter()
        .filter_map(|(name, decl)| {
            let user = values.get(name).and_then(|value| {
                decl.css(value)
                    .inspect_err(|e| problems.push(format!("vars.{name}: {e:#}")))
                    .ok()
            });
            let css = user.or_else(|| decl.css(&decl.default).ok())?;
            Some((name.clone(), css))
        })
        .collect();
    problems.extend(
        values
            .keys()
            .filter(|name| !decls.contains_key(*name))
            .map(|name| format!("vars.{name}: the theme doesn't declare it")),
    );
    (resolved, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CSS: &str = r##"
        /* @glaze-vars
        {
          "--glow": { "type": "color", "label": "Glow", "default": "#ff2bd6" },
          "--glow-size": { "type": "length", "label": "Glow size", "min": 0, "max": 40, "default": 14 },
          "--gap": { "type": "number", "label": "Gap", "min": 0, "max": 2, "step": 0.1, "default": 0.5 },
          "--labels": { "type": "bool", "label": "Labels", "group": "Dock", "default": true },
          "--shape": { "type": "select", "label": "Shape", "options": ["soft", "sharp"], "default": "soft" }
        }
        */
        .dock { box-shadow: 0 0 var(--glow-size) var(--glow); }
    "##;

    #[test]
    fn declarations_come_from_the_marked_comment() {
        let decls = declared(CSS).unwrap();
        assert_eq!(decls.len(), 5);
        assert_eq!(decls["--labels"].group.as_deref(), Some("Dock"));
        assert_eq!(decls["--glow-size"].unit, Unit::Px);
        assert!(declared(".a { color: red }").unwrap().is_empty());
        for bad in [
            r#"/* @glaze-vars { "--a": { "type": "color", "label": "A", "default": "red" } } */"#,
            r#"/* @glaze-vars { "--a": { "type": "number", "label": "A", "default": 9, "max": 1 } } */"#,
            r#"/* @glaze-vars { "--a": { "type": "select", "label": "A", "default": "x", "options": [] } } */"#,
            r#"/* @glaze-vars { "a": { "type": "bool", "label": "A", "default": true } } */"#,
            r#"/* @glaze-vars { "--a": { "type": "bool", "label": "A", "default": true, "colour": 1 } } */"#,
            r#"/* @glaze-vars { "--a": [1 "#,
        ] {
            assert!(declared(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn values_are_checked_against_their_declaration() {
        let decls = declared(CSS).unwrap();
        let values = BTreeMap::from([
            ("--glow".to_owned(), json!("#00ffaa80")),
            ("--glow-size".to_owned(), json!(99)),
            ("--labels".to_owned(), json!(false)),
            ("--gap".to_owned(), json!(1.25)),
            ("--nope".to_owned(), json!(1)),
        ]);
        let (css, problems) = resolve(&decls, &values);
        assert_eq!(css["--glow"], "#00ffaa80");
        assert_eq!(css["--glow-size"], "14px", "out of range: the default");
        assert_eq!(css["--labels"], "0");
        assert_eq!(css["--gap"], "1.25");
        assert_eq!(css["--shape"], "soft");
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
