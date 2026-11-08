//! `desktop_menu` in `config.json`: Glaze's own right-click menu for the
//! bare desktop (docs/desktop-extras.md). On by default; `"enabled": false` turns it off.
//!
//! ```json
//! "desktop_menu": {
//!   "enabled": true,
//!   "items": ["view", "sort", "refresh", "separator", "new", "paste", "separator",
//!             "wallpaper", "widgets", "display", "personalize", "terminal", "separator",
//!             { "label": "Notes", "command": "notepad.exe", "args": ["D:\\notes.txt"] },
//!             "more"]
//! }
//! ```
//!
//! `items` is the menu top to bottom: leave an entry out to hide it, reorder
//! to move it. A string is a built-in ([`Builtin`]); an object is a custom
//! command ([`Custom`]). Without `items` you get [`default_items`].

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// The menu's built-in entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Builtin {
    /// A line between groups.
    Separator,
    /// Icon size (large, medium, small) and "Show desktop icons".
    View,
    /// Name, size, type, date modified.
    Sort,
    Refresh,
    /// New folder, new text document.
    New,
    Paste,
    /// Opens Glaze Studio.
    Wallpaper,
    /// Toggles widget edit mode.
    Widgets,
    /// Opens Studio's Add a widget page.
    #[serde(rename = "add-widget")]
    AddWidget,
    /// Opens Glaze Studio's Home page.
    Studio,
    /// `ms-settings:display`.
    Display,
    /// `ms-settings:personalization`.
    Personalize,
    /// Windows Terminal in the Desktop folder.
    Terminal,
    /// "Show more options": the native Windows menu.
    More,
}

/// A user command: runs `command` with `args` (no shell, no quoting rules to learn).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custom {
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Builtin(Builtin),
    Custom(Custom),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DesktopMenu {
    pub enabled: bool,
    /// Written to the file only when it differs from [`default_items`].
    #[serde(skip_serializing_if = "is_default_items")]
    pub items: Vec<Entry>,
}

fn is_default_items(items: &[Entry]) -> bool {
    items == default_items()
}

impl Default for DesktopMenu {
    fn default() -> Self {
        Self {
            enabled: true,
            items: default_items(),
        }
    }
}

/// The menu you get without `items`.
pub fn default_items() -> Vec<Entry> {
    use Builtin::*;
    [
        View,
        Sort,
        Refresh,
        Separator,
        Paste,
        Separator,
        Terminal,
        More,
        Separator,
        New,
        Separator,
        Display,
        Personalize,
        Separator,
        Wallpaper,
        Widgets,
        AddWidget,
        Studio,
    ]
    .into_iter()
    .map(Entry::Builtin)
    .collect()
}

impl DesktopMenu {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// What serde cannot say: custom commands need a label and a command.
    pub fn validate(&self) -> Result<()> {
        for entry in &self.items {
            if let Entry::Custom(c) = entry
                && (c.label.trim().is_empty() || c.command.trim().is_empty())
            {
                bail!("desktop_menu: a custom item needs a non-empty label and command");
            }
        }
        Ok(())
    }

    /// The items with leading, trailing and doubled separators removed, so a
    /// hidden entry never leaves a stray line behind.
    pub fn tidy_items(&self) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = Vec::new();
        for entry in &self.items {
            let separator = matches!(entry, Entry::Builtin(Builtin::Separator));
            let after_separator = matches!(out.last(), Some(Entry::Builtin(Builtin::Separator)));
            if separator && (out.is_empty() || after_separator) {
                continue;
            }
            out.push(entry);
        }
        while matches!(out.last(), Some(Entry::Builtin(Builtin::Separator))) {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<DesktopMenu> {
        let menu: DesktopMenu = serde_json::from_str(json)?;
        menu.validate()?;
        Ok(menu)
    }

    #[test]
    fn on_by_default_with_the_standard_items() {
        let menu = parse("{}").unwrap();
        assert!(menu.enabled);
        assert_eq!(menu.items, default_items());
        assert_eq!(menu.items.first(), Some(&Entry::Builtin(Builtin::View)));
        assert_eq!(menu.items.last(), Some(&Entry::Builtin(Builtin::Studio)));
        // Items without `enabled` (a file edited before the default changed) are on too.
        assert!(parse(r#"{ "items": ["refresh"] }"#).unwrap().enabled);
        assert!(!parse(r#"{ "enabled": false }"#).unwrap().enabled);
    }

    #[test]
    fn items_are_builtins_by_name_or_custom_commands_in_the_order_written() {
        let menu = parse(
            r#"{ "enabled": true, "items": [
                "refresh", "separator",
                { "label": "Notes", "command": "notepad.exe", "args": ["a b.txt"] },
                { "label": "Calc", "command": "calc.exe" },
                "view"
            ] }"#,
        )
        .unwrap();
        assert_eq!(
            menu.items,
            [
                Entry::Builtin(Builtin::Refresh),
                Entry::Builtin(Builtin::Separator),
                Entry::Custom(Custom {
                    label: "Notes".into(),
                    command: "notepad.exe".into(),
                    args: vec!["a b.txt".into()],
                }),
                Entry::Custom(Custom {
                    label: "Calc".into(),
                    command: "calc.exe".into(),
                    args: vec![],
                }),
                Entry::Builtin(Builtin::View),
            ]
        );
    }

    #[test]
    fn typos_and_empty_commands_are_rejected() {
        assert!(
            parse(r#"{ "items": ["vew"] }"#).is_err(),
            "unknown built-in"
        );
        assert!(
            parse(r#"{ "enabled": true, "item": [] }"#).is_err(),
            "unknown field"
        );
        assert!(parse(r#"{ "items": [{ "label": "x", "comand": "y" }] }"#).is_err());
        assert!(parse(r#"{ "items": [{ "label": " ", "command": "y" }] }"#).is_err());
        assert!(parse(r#"{ "items": [{ "label": "x", "command": "" }] }"#).is_err());
        assert!(parse(r#"{ "enabled": "yes" }"#).is_err());
    }

    #[test]
    fn hiding_items_never_leaves_stray_separators() {
        use Builtin::*;
        let menu = DesktopMenu {
            enabled: true,
            items: [
                Separator, Separator, Refresh, Separator, Separator, Paste, Separator,
            ]
            .into_iter()
            .map(Entry::Builtin)
            .collect(),
        };
        let tidy: Vec<_> = menu.tidy_items().into_iter().cloned().collect();
        assert_eq!(
            tidy,
            [Refresh, Separator, Paste].map(Entry::Builtin).to_vec()
        );
        assert!(
            DesktopMenu {
                enabled: true,
                items: vec![]
            }
            .tidy_items()
            .is_empty()
        );
    }

    #[test]
    fn defaults_are_not_written_back_to_the_file() {
        let untouched = serde_json::to_value(DesktopMenu::default()).unwrap();
        assert_eq!(untouched, serde_json::json!({ "enabled": true }));
        let off = DesktopMenu {
            enabled: false,
            ..DesktopMenu::default()
        };
        assert!(
            !off.is_default(),
            "turned off is a setting, so it's written"
        );
        let custom = DesktopMenu {
            enabled: true,
            items: vec![Entry::Builtin(Builtin::Sort)],
        };
        assert_eq!(
            serde_json::to_value(&custom).unwrap(),
            serde_json::json!({ "enabled": true, "items": ["sort"] })
        );
        assert!(DesktopMenu::default().is_default() && !custom.is_default());
    }

    #[test]
    fn round_trips_through_json() {
        let menu = parse(
            r#"{ "enabled": true, "items": ["sort", { "label": "A", "command": "b.exe", "args": ["c"] }] }"#,
        )
        .unwrap();
        let again: DesktopMenu =
            serde_json::from_str(&serde_json::to_string(&menu).unwrap()).unwrap();
        assert_eq!(again, menu);
    }
}
