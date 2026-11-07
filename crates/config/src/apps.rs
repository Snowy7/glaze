//! How the app styler restyles the XAML inside File Explorer and Settings (experimental,
//! docs/windows-styling.md, "App styler"). A theme's `theme.json` › `apps` is the layer below the
//! user's `config.json` › `look.apps`, field by field, like the `windows` section.
//!
//! The shape is checked here; selectors, property names and values are XAML's, so they are only
//! checked by the styler itself, which logs and skips a rule it can't use instead of failing the
//! whole config.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The apps the styler may touch, each by the name its rules are filed under.
pub const APPS: [&str; 2] = ["explorer", "settings"];

/// `selector` -> `property` -> value. A `null` value takes a lower layer's value away.
pub type Rules = BTreeMap<String, BTreeMap<String, Option<String>>>;

/// The `apps` section: a preset to start from, then rules per app.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppsStyle {
    /// A built-in preset as the base layer (`calm`, `glass`, `compact`), or `"none"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// File Explorer (`explorer.exe`).
    #[serde(skip_serializing_if = "AppRules::is_empty")]
    pub explorer: AppRules,
    /// Settings (`SystemSettings.exe`).
    #[serde(skip_serializing_if = "AppRules::is_empty")]
    pub settings: AppRules,
}

/// One app's rules and resource overrides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppRules {
    /// Whether the styler touches this app at all while `experimental.app_styler` is on. Absent:
    /// Explorer yes (it also needs `system.separate_process = true`), Settings no (it ends itself
    /// on one page while attached; docs/windows-styling.md, "App styler"). Not part of the rules
    /// file the styled app reads ([`AppRules::settled`] leaves it out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Properties to set on the elements a selector matches.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: Rules,
    /// Theme resources (brushes, colours, sizes) to override, by key.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, Option<String>>,
}

impl AppsStyle {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// One app's rules by its name in [`APPS`].
    pub fn app(&self, name: &str) -> Option<&AppRules> {
        match name {
            "explorer" => Some(&self.explorer),
            "settings" => Some(&self.settings),
            _ => None,
        }
    }

    /// `self` over `base`: the preset and every rule property and resource set here win,
    /// `null`s included.
    pub fn over(&self, base: &AppsStyle) -> AppsStyle {
        AppsStyle {
            preset: self.preset.clone().or_else(|| base.preset.clone()),
            explorer: self.explorer.over(&base.explorer),
            settings: self.settings.over(&base.settings),
        }
    }
}

impl AppRules {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `self` over `base`, per selector and property, and per resource key.
    pub fn over(&self, base: &AppRules) -> AppRules {
        let mut rules = base.rules.clone();
        for (selector, properties) in &self.rules {
            let merged = rules.entry(selector.clone()).or_default();
            for (property, value) in properties {
                merged.insert(property.clone(), value.clone());
            }
        }
        let mut resources = base.resources.clone();
        for (key, value) in &self.resources {
            resources.insert(key.clone(), value.clone());
        }
        AppRules {
            enabled: self.enabled.or(base.enabled),
            rules,
            resources,
        }
    }

    /// The rules with every `null` dropped (and selectors left with nothing to set), as the
    /// styled app reads them: without `enabled`, which only the helper reads.
    pub fn settled(&self) -> AppRules {
        let rules = self
            .rules
            .iter()
            .filter_map(|(selector, properties)| {
                let set: BTreeMap<_, _> = properties
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .map(|(p, v)| (p.clone(), v.clone()))
                    .collect();
                (!set.is_empty()).then(|| (selector.clone(), set))
            })
            .collect();
        let resources = self
            .resources
            .iter()
            .filter(|(_, value)| value.is_some())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        AppRules {
            enabled: None,
            rules,
            resources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> AppsStyle {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_users_layer_wins_per_property_and_null_takes_a_value_away() {
        let theme = parse(
            r##"{ "preset": "glass", "settings": {
                "rules": { "Grid#RootGrid": { "Background": "#202020", "Padding": "4" } },
                "resources": { "CardBackground": "#101010" } } }"##,
        );
        let user = parse(
            r##"{ "settings": {
                "rules": { "Grid#RootGrid": { "Background": "Transparent", "Padding": null } },
                "resources": { "CardBackground": null } } }"##,
        );
        let merged = user.over(&theme);
        assert_eq!(merged.preset.as_deref(), Some("glass"), "absent: the theme's");
        let settled = merged.settings.settled();
        let root = &settled.rules["Grid#RootGrid"];
        assert_eq!(root["Background"].as_deref(), Some("Transparent"));
        assert!(!root.contains_key("Padding"), "null: not set");
        assert!(settled.resources.is_empty());
        // Round trip keeps the nulls.
        let text = serde_json::to_string(&user).unwrap();
        assert_eq!(parse(&text), user);
    }

    #[test]
    fn enabled_layers_like_the_rules_and_stays_out_of_the_rules_file() {
        let theme = parse(r#"{ "explorer": { "enabled": false } }"#);
        let user = parse(r#"{ "explorer": { "enabled": true }, "settings": { "enabled": false } }"#);
        let merged = user.over(&theme);
        assert_eq!(merged.explorer.enabled, Some(true), "the user's wins");
        assert_eq!(merged.settings.enabled, Some(false));
        assert_eq!(AppsStyle::default().over(&theme).explorer.enabled, Some(false));
        assert_eq!(merged.explorer.settled().enabled, None);
        assert_eq!(serde_json::to_string(&merged.settings.settled()).unwrap(), "{}");
    }

    #[test]
    fn empty_is_left_out_and_typos_fail() {
        assert_eq!(serde_json::to_string(&AppsStyle::default()).unwrap(), "{}");
        assert!(serde_json::from_str::<AppsStyle>(r#"{ "explorr": {} }"#).is_err());
        assert!(serde_json::from_str::<AppsStyle>(r#"{ "settings": { "rule": {} } }"#).is_err());
    }
}
