//! Optional per-user Windows preferences. Absent/null means leave Windows alone.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_extensions: Option<bool>,
    /// Hidden files, without changing protected operating-system file visibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_hidden: Option<bool>,
    /// Explorer folder windows in a separate process; not a speed guarantee.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub separate_process: Option<bool>,
    /// Windows' user power mode for both AC and battery, preserving each original.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_mode: Option<PowerMode>,
    /// Publish the conservative fullscreen/presentation signal. No watcher while off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_activity: Option<bool>,
}

impl SystemConfig {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerMode {
    Efficiency,
    Balanced,
    Performance,
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn system_is_opt_in_checked_and_round_trips() {
        let empty = Config::parse("{}").unwrap();
        assert!(empty.system.is_empty());
        assert!(!serde_json::to_string(&empty).unwrap().contains("\"system\""));
        let config = Config::parse(r#"{"system":{"game_mode":false,"show_hidden":true,"power_mode":"efficiency","game_activity":true}}"#).unwrap();
        assert_eq!(Config::parse(&serde_json::to_string(&config).unwrap()).unwrap(), config);
        assert!(Config::parse(r#"{"system":{"game_mdoe":true}}"#).is_err());
        assert!(Config::parse(r#"{"system":{"power_mode":"ultimate"}}"#).is_err());
        assert!(Config::parse(r#"{"system":{"background_capture":false}}"#).is_err());
        assert!(Config::parse(r#"{"system":{"show_hidden":null}}"#).unwrap().system.is_empty());
    }
}
