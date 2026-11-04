//! Built-in presets (docs/bars.md, "Presets"): each is a partial `bars.json` (layout, look,
//! animation, dock behavior) that the user's file is merged over, plus CSS the page bundles
//! (`bars/shell/src/presets/<name>.css`, scoped by `data-preset`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// Black glass over the live wallpaper, white type, a floating dock: the default.
    #[default]
    Monochrome,
    /// A translucent menu bar and a magnifying dock with springy motion.
    Macos,
    /// One full-width taskbar: Start, search, task buttons, tray and clock (no top bar). With
    /// `taskbar.mode: hide` it replaces the native taskbar.
    Windows11,
    /// A transparent rail with one pill per module group.
    Islands,
    /// Dark glass, neon gradient edges and glow.
    NeonGlass,
    /// A detached, rounded top bar and dock.
    Floating,
}

impl Preset {
    pub const ALL: [Preset; 6] = [
        Preset::Monochrome,
        Preset::Macos,
        Preset::Windows11,
        Preset::Islands,
        Preset::NeonGlass,
        Preset::Floating,
    ];

    fn source(self) -> &'static str {
        match self {
            Preset::Monochrome => include_str!("../presets/monochrome.json"),
            Preset::Macos => include_str!("../presets/macos.json"),
            Preset::Windows11 => include_str!("../presets/windows11.json"),
            Preset::Islands => include_str!("../presets/islands.json"),
            Preset::NeonGlass => include_str!("../presets/neon-glass.json"),
            Preset::Floating => include_str!("../presets/floating.json"),
        }
    }

    /// The preset's partial config. The files are checked by the tests, so this can't fail at
    /// run time.
    pub fn bundle(self) -> Value {
        serde_json::from_str(self.source()).unwrap_or_else(|e| {
            log::error!("preset {self:?} is broken: {e}");
            Value::Object(Default::default())
        })
    }
}

/// Deep-merges `over` into `base`: objects merge key by key, anything else (arrays included) is
/// replaced.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                match base.get_mut(key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, over) => *base = over.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BarsConfig;
    use serde_json::json;

    #[test]
    fn merging_is_deep_for_objects_and_replaces_the_rest() {
        let mut base =
            json!({ "top": { "height": 28, "left": ["start"] }, "look": { "noise": 0.1 } });
        merge(
            &mut base,
            &json!({ "top": { "left": ["clock"] }, "dock": { "size": 40 } }),
        );
        assert_eq!(
            base,
            json!({ "top": { "height": 28, "left": ["clock"] }, "look": { "noise": 0.1 }, "dock": { "size": 40 } })
        );
    }

    #[test]
    fn every_preset_parses_and_leaves_the_users_settings_alone() {
        for preset in Preset::ALL {
            let bundle = preset.bundle();
            let object = bundle.as_object().expect("a preset is an object");
            assert!(!object.is_empty(), "{preset:?} is empty or broken");
            for owned in ["version", "preset", "taskbar", "overrides", "vars"] {
                assert!(!object.contains_key(owned), "{preset:?} sets {owned}");
            }
            assert!(
                bundle.pointer("/dock/pinned").is_none(),
                "{preset:?} sets pins"
            );
            let name = serde_json::to_value(preset).unwrap();
            let config = BarsConfig::parse(&json!({ "preset": name }).to_string())
                .unwrap_or_else(|e| panic!("{preset:?}: {e:#}"));
            assert_eq!(config.preset, preset);
        }
    }
}
