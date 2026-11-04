//! The flyouts' sections of `bars.json` (docs/bars.md, "Flyouts"): the launcher, the tray host,
//! the calendar's extra clocks and audio output switching. Part of the library (core and Studio
//! read `bars.json` too); `config.rs` holds them as top-level fields.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Pinned;

/// `launcher`: the Start menu replacement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct LauncherConfig {
    /// A lone press of the Windows key opens the launcher instead of the native Start menu. Only
    /// while the native taskbar is hidden (`taskbar.mode: hide`) and no fullscreen app is in front;
    /// Windows key combinations always pass through.
    pub win_key: bool,
    pub layout: LauncherLayout,
    /// The pinned grid. Missing: the dock's pins.
    pub pinned: Option<Vec<Pinned>>,
    /// Search recent files (`%APPDATA%\Microsoft\Windows\Recent`) too.
    pub files: bool,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            win_key: true,
            layout: LauncherLayout::Win11,
            pinned: None,
            files: true,
        }
    }
}

/// How the launcher is laid out; each is a page layout in `bars/shell/src/flyouts/launcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LauncherLayout {
    /// Search, pinned and recommended (recent files), like Windows 11.
    #[default]
    Win11,
    /// A big icon grid of every app.
    Grid,
    /// A search bar and its results only.
    Compact,
    /// Search and pinned, a row of widgets, and the clipboard history.
    Dashboard,
}

/// `tray`: where the notification-area icons come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrayConfig {
    /// Be the tray host (our own `Shell_TrayWnd`) even while the native taskbar is kept. It
    /// always is with `taskbar.mode: hide`, while a tray module is on screen.
    pub host: bool,
}

/// `calendar`: the calendar flyout.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct CalendarConfig {
    /// Extra clocks under the month, at most 4.
    pub zones: Vec<Zone>,
    /// The first day of the week. Missing: the locale's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_day: Option<Weekday>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    /// An IANA time zone, e.g. `America/New_York`.
    pub zone: String,
    /// Missing: the city part of the zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Sunday,
    Saturday,
}

/// `audio`: the mixer flyout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioConfig {
    /// Let the mixer switch the default output device. This uses `IPolicyConfig`, an
    /// **undocumented** COM interface (what the Sound settings page uses); without it the mixer
    /// lists the devices and links to Sound settings.
    pub switch_output: bool,
}

/// Checks the flyout sections of a parsed config.
pub fn validate(launcher: &LauncherConfig, calendar: &CalendarConfig) -> Result<()> {
    if let Some(pins) = &launcher.pinned
        && pins.iter().any(|p| p.path.trim().is_empty())
    {
        bail!("launcher.pinned: every item needs a path");
    }
    if calendar.zones.len() > 4 {
        bail!("calendar.zones: at most 4");
    }
    if calendar.zones.iter().any(|z| z.zone.trim().is_empty()) {
        bail!("calendar.zones: every zone needs a name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BarsConfig;

    #[test]
    fn the_flyout_sections_parse_with_defaults() {
        let config = BarsConfig::parse("{}").unwrap();
        assert!(config.launcher.win_key);
        assert_eq!(config.launcher.layout, LauncherLayout::Win11);
        assert_eq!(config.launcher.pinned, None);
        assert!(!config.tray.host);
        assert!(!config.audio.switch_output);

        let config = BarsConfig::parse(
            r#"{ "launcher": { "winKey": false, "layout": "dashboard", "pinned": [ { "path": "a.exe" } ] },
                 "tray": { "host": true },
                 "calendar": { "zones": [ { "zone": "Asia/Tokyo", "label": "Studio" } ], "firstDay": "sunday" },
                 "audio": { "switchOutput": true } }"#,
        )
        .unwrap();
        assert!(!config.launcher.win_key);
        assert_eq!(config.launcher.layout, LauncherLayout::Dashboard);
        assert!(config.tray.host);
        assert_eq!(config.calendar.first_day, Some(Weekday::Sunday));
        assert_eq!(config.calendar.zones[0].label.as_deref(), Some("Studio"));
        assert!(config.audio.switch_output);
        // The page gets camelCase keys.
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["launcher"]["winKey"], false);
        assert_eq!(json["audio"]["switchOutput"], true);
    }

    #[test]
    fn bad_flyout_sections_are_refused() {
        for bad in [
            r#"{ "launcher": { "layout": "start10" } }"#,
            r#"{ "launcher": { "pinned": [ { "path": "" } ] } }"#,
            r#"{ "launcher": { "winkey": true } }"#,
            r#"{ "tray": { "host": "yes" } }"#,
            r#"{ "calendar": { "zones": [ { "zone": "" } ] } }"#,
            r#"{ "calendar": { "zones": [ {"zone":"a"}, {"zone":"b"}, {"zone":"c"}, {"zone":"d"}, {"zone":"e"} ] } }"#,
            r#"{ "audio": { "policyConfig": true } }"#,
        ] {
            assert!(BarsConfig::parse(bad).is_err(), "accepted {bad}");
        }
    }
}
