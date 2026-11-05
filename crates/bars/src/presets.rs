//! Top/dock designs and preset pairings, layered as typed configuration for the native shell.
//! Layout, metrics, modules, look and motion are data; there is no companion CSS.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A starting point for both bars: which design each starts from, which bars are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// Black glass over the live wallpaper, white type, a floating dock: the default.
    #[default]
    Monochrome,
    /// The macOS menu bar and a magnifying dock.
    Macos,
    /// One Windows 11 taskbar (no top bar). With `taskbar.mode: hide` it replaces the native one.
    Windows11,
    /// The Windows 10 taskbar (no top bar).
    Windows10,
    /// The ChromeOS shelf (no top bar).
    Chromeos,
    /// A transparent rail with one pill per module group, and a floating dock.
    Islands,
    /// Dark glass, neon gradient edges and glow.
    NeonGlass,
    /// A detached, rounded top bar and dock.
    Floating,
    /// A floating capsule hugging its modules up top, a floating pill of a dock.
    Pill,
    /// Full-width bars flush with both edges: a strip up top, a taskbar below.
    Edge,
}

impl Preset {
    pub const ALL: [Preset; 10] = [
        Preset::Monochrome,
        Preset::Macos,
        Preset::Windows11,
        Preset::Windows10,
        Preset::Chromeos,
        Preset::Islands,
        Preset::NeonGlass,
        Preset::Floating,
        Preset::Pill,
        Preset::Edge,
    ];

    fn source(self) -> &'static str {
        match self {
            Preset::Monochrome => include_str!("../presets/monochrome.json"),
            Preset::Macos => include_str!("../presets/macos.json"),
            Preset::Windows11 => include_str!("../presets/windows11.json"),
            Preset::Windows10 => include_str!("../presets/windows10.json"),
            Preset::Chromeos => include_str!("../presets/chromeos.json"),
            Preset::Islands => include_str!("../presets/islands.json"),
            Preset::NeonGlass => include_str!("../presets/neon-glass.json"),
            Preset::Floating => include_str!("../presets/floating.json"),
            Preset::Pill => include_str!("../presets/pill.json"),
            Preset::Edge => include_str!("../presets/edge.json"),
        }
    }

    /// The preset's partial config. The files are checked by the tests, so this can't fail at
    /// run time.
    pub fn bundle(self) -> Value {
        parse_bundle(self.source(), &self)
    }
}

/// A top bar's design (`top.design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TopDesign {
    /// A black wallpaper-glass strip with white type.
    #[default]
    Monochrome,
    /// The macOS menu bar: translucent, the app's name in bold, status icons and the clock right.
    Macos,
    /// A Windows 11 bar: Mica-like, Segoe Fluent Icons, Fluent hover and press states.
    Fluent,
    /// A floating capsule hugging its modules, iPadOS and visionOS style.
    Capsule,
    /// No surface: the clock in the middle, a few status icons.
    Minimal,
    /// A transparent rail with one pill per group of modules.
    Islands,
    /// Dark glass with a neon gradient edge and glow.
    Neon,
    /// A detached, rounded bar across the top.
    Floating,
}

impl TopDesign {
    pub const ALL: [TopDesign; 8] = [
        TopDesign::Monochrome,
        TopDesign::Macos,
        TopDesign::Fluent,
        TopDesign::Capsule,
        TopDesign::Minimal,
        TopDesign::Islands,
        TopDesign::Neon,
        TopDesign::Floating,
    ];

    fn source(self) -> &'static str {
        match self {
            TopDesign::Monochrome => include_str!("../presets/top/monochrome.json"),
            TopDesign::Macos => include_str!("../presets/top/macos.json"),
            TopDesign::Fluent => include_str!("../presets/top/fluent.json"),
            TopDesign::Capsule => include_str!("../presets/top/capsule.json"),
            TopDesign::Minimal => include_str!("../presets/top/minimal.json"),
            TopDesign::Islands => include_str!("../presets/top/islands.json"),
            TopDesign::Neon => include_str!("../presets/top/neon.json"),
            TopDesign::Floating => include_str!("../presets/top/floating.json"),
        }
    }

    /// The design's partial `top` (with its `look` and `animation`).
    pub fn bundle(self) -> Value {
        parse_bundle(self.source(), &self)
    }
}

/// A dock's design (`dock.design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DockDesign {
    /// A black glass pill with dots.
    #[default]
    Monochrome,
    /// The macOS dock: a frosted plate, magnify, running dots, the trash past a divider.
    Macos,
    /// The Windows 11 taskbar: centered, Fluent icons, pill indicators, two-line clock.
    Windows11,
    /// The Windows 10 taskbar: left-aligned, square buttons, underline indicators.
    Windows10,
    /// The ChromeOS shelf: a launcher circle, centered apps, a status pill.
    Chromeos,
    /// A floating Windows 11 dock: rounded acrylic, pill indicators.
    Fluent,
    /// One plate per group (pinned, running, extras), bar indicators.
    Islands,
    /// Dark glass, glow indicators.
    Neon,
    /// A detached, rounded pill.
    Floating,
    /// No plate: the icons on the wallpaper, dots under the running ones.
    Minimal,
    /// A full-width bar flush with the edge: Start, tasks, status and the clock.
    Edge,
}

impl DockDesign {
    pub const ALL: [DockDesign; 11] = [
        DockDesign::Monochrome,
        DockDesign::Macos,
        DockDesign::Windows11,
        DockDesign::Windows10,
        DockDesign::Chromeos,
        DockDesign::Fluent,
        DockDesign::Islands,
        DockDesign::Neon,
        DockDesign::Floating,
        DockDesign::Minimal,
        DockDesign::Edge,
    ];

    fn source(self) -> &'static str {
        match self {
            DockDesign::Monochrome => include_str!("../presets/dock/monochrome.json"),
            DockDesign::Macos => include_str!("../presets/dock/macos.json"),
            DockDesign::Windows11 => include_str!("../presets/dock/windows11.json"),
            DockDesign::Windows10 => include_str!("../presets/dock/windows10.json"),
            DockDesign::Chromeos => include_str!("../presets/dock/chromeos.json"),
            DockDesign::Fluent => include_str!("../presets/dock/fluent.json"),
            DockDesign::Islands => include_str!("../presets/dock/islands.json"),
            DockDesign::Neon => include_str!("../presets/dock/neon.json"),
            DockDesign::Floating => include_str!("../presets/dock/floating.json"),
            DockDesign::Minimal => include_str!("../presets/dock/minimal.json"),
            DockDesign::Edge => include_str!("../presets/dock/edge.json"),
        }
    }

    /// The design's partial `dock` (with its `look` and `animation`).
    pub fn bundle(self) -> Value {
        parse_bundle(self.source(), &self)
    }
}

fn parse_bundle(source: &str, name: &dyn std::fmt::Debug) -> Value {
    serde_json::from_str(source).unwrap_or_else(|e| {
        log::error!("bundle {name:?} is broken: {e}");
        Value::Object(Default::default())
    })
}

/// The JSON a user's file resolves from, before parsing: each bar's design, then the preset,
/// then the file with its shared `look` and `animation` folded under each bar's own. A design
/// named in the file (`top.design`) beats the preset's; an unknown name is an error.
pub fn layered(raw: &Map<String, Value>) -> Result<Value, serde_json::Error> {
    let preset = match raw.get("preset") {
        Some(name) => Preset::deserialize(name)?,
        None => Preset::default(),
    };
    let pairing = preset.bundle();
    let named = |bar: &str| {
        raw.get(bar)
            .and_then(|b| b.get("design"))
            .or_else(|| pairing.get(bar).and_then(|b| b.get("design")))
    };
    let top = named("top").map_or(Ok(TopDesign::default()), TopDesign::deserialize)?;
    let dock = named("dock").map_or(Ok(DockDesign::default()), DockDesign::deserialize)?;
    let mut value = serde_json::json!({ "top": top.bundle(), "dock": dock.bundle() });
    merge(&mut value, &pairing);
    merge(&mut value, &Value::Object(fold_shared(raw)));
    Ok(value)
}

/// `raw` with its shared `look` and `animation` also under each bar, beneath the bar's own:
/// `top.look` becomes `look` merged with `top.look`.
fn fold_shared(raw: &Map<String, Value>) -> Map<String, Value> {
    let mut out = raw.clone();
    for field in ["look", "animation"] {
        let Some(shared) = raw.get(field) else {
            continue;
        };
        for bar in ["top", "dock"] {
            let mut layered = shared.clone();
            if let Some(own) = raw.get(bar).and_then(|b| b.get(field)) {
                merge(&mut layered, own);
            }
            // A bar that isn't an object is the strict parse's to report.
            if let Value::Object(section) =
                out.entry(bar).or_insert_with(|| Value::Object(Map::new()))
            {
                section.insert(field.to_owned(), layered);
            }
        }
    }
    out
}

/// Deep-merges `over` into `base`: objects merge key by key, anything else (arrays included) is
/// replaced. An object whose `type` tag differs from the base's (a tagged union switching
/// variants, e.g. a `look.background` going from `wallpaper` to `solid`) replaces it whole: the
/// old variant's fields mean nothing to the new one.
pub fn merge(base: &mut Value, over: &Value) {
    let switches_variant = over
        .get("type")
        .is_some_and(|tag| base.get("type") != Some(tag));
    if switches_variant {
        *base = over.clone();
        return;
    }
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
    use crate::config::{DockConfig, TopConfig};
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
    fn a_different_variant_replaces_the_tagged_object_whole() {
        let mut base =
            json!({ "look": { "background": { "type": "wallpaper", "brightness": 0.9 } } });
        merge(
            &mut base,
            &json!({ "look": { "background": { "type": "solid", "color": "#101010" } } }),
        );
        assert_eq!(
            base,
            json!({ "look": { "background": { "type": "solid", "color": "#101010" } } })
        );
        // Same variant: still a deep merge.
        merge(
            &mut base,
            &json!({ "look": { "background": { "type": "solid", "opacity": 0.5 } } }),
        );
        assert_eq!(base["look"]["background"]["color"], "#101010");
        assert_eq!(base["look"]["background"]["opacity"], 0.5);
    }

    #[test]
    fn shared_look_sits_under_each_bars_own() {
        let raw = json!({
            "look": { "tint": 0.3, "noise": 0.1 },
            "top": { "look": { "tint": 0.9 } },
            "animation": { "duration": 100 }
        });
        let folded = fold_shared(raw.as_object().unwrap());
        assert_eq!(folded["top"]["look"], json!({ "tint": 0.9, "noise": 0.1 }));
        assert_eq!(folded["dock"]["look"], json!({ "tint": 0.3, "noise": 0.1 }));
        assert_eq!(folded["dock"]["animation"]["duration"], 100);
        assert_eq!(
            folded["look"], raw["look"],
            "the shared layer stays as written"
        );
    }

    #[test]
    fn every_design_switches_to_any_background_type() {
        let backgrounds = [
            json!({ "type": "solid", "color": "#202020" }),
            json!({ "type": "none" }),
            json!({ "type": "blur" }),
            json!({ "type": "wallpaper" }),
        ];
        for background in &backgrounds {
            for design in TopDesign::ALL {
                let mut bar = design.bundle();
                merge(&mut bar, &json!({ "look": { "background": background } }));
                let parsed = serde_json::from_value::<TopConfig>(bar);
                assert!(
                    parsed.is_ok(),
                    "{design:?} + {background}: {:?}",
                    parsed.err()
                );
            }
            for design in DockDesign::ALL {
                let mut bar = design.bundle();
                merge(&mut bar, &json!({ "look": { "background": background } }));
                let parsed = serde_json::from_value::<DockConfig>(bar);
                assert!(
                    parsed.is_ok(),
                    "{design:?} + {background}: {:?}",
                    parsed.err()
                );
            }
        }
    }

    /// macOS's top bar is transparent, as Tahoe's menu bar; every design of either bar can be
    /// made transparent or given its glass back by the user's own `look.transparent`.
    #[test]
    fn transparency_is_the_macos_top_bars_default_and_every_designs_option() {
        let parse = |raw: Value| BarsConfig::parse(&raw.to_string()).unwrap();
        let macos = parse(json!({ "preset": "macos" }));
        assert!(macos.top.look.is_transparent());
        assert!(!macos.dock.look.is_transparent());
        let glass = parse(json!({ "preset": "macos", "top": { "look": { "transparent": false } } }));
        assert!(!glass.top.look.is_transparent());
        for design in TopDesign::ALL {
            let name = serde_json::to_value(design).unwrap();
            for on in [true, false] {
                let bar = json!({ "design": name, "look": { "transparent": on } });
                assert_eq!(parse(json!({ "top": bar })).top.look.is_transparent(), on, "{design:?}");
            }
        }
        for design in DockDesign::ALL {
            let name = serde_json::to_value(design).unwrap();
            let bar = json!({ "design": name, "look": { "transparent": true } });
            assert!(parse(json!({ "dock": bar })).dock.look.is_transparent(), "{design:?}");
        }
    }

    /// Designs are looks: which bars show, where and with which pins stays the user's (or the
    /// preset's) choice.
    #[test]
    fn designs_parse_and_leave_placement_alone() {
        let owned = [
            "design", "enabled", "monitors", "position", "pinned", "autohide",
        ];
        let check = |name: String, bundle: Value| {
            let object = bundle
                .as_object()
                .unwrap_or_else(|| panic!("{name} is not an object"));
            assert!(!object.is_empty(), "{name} is empty or broken");
            for key in owned {
                assert!(!object.contains_key(key), "{name} sets {key}");
            }
            assert!(object.contains_key("look"), "{name} has no look");
        };
        for design in TopDesign::ALL {
            check(format!("top {design:?}"), design.bundle());
            let name = serde_json::to_value(design).unwrap();
            let config = BarsConfig::parse(&json!({ "top": { "design": name } }).to_string())
                .unwrap_or_else(|e| panic!("top {design:?}: {e:#}"));
            assert_eq!(config.top.design, design);
        }
        for design in DockDesign::ALL {
            check(format!("dock {design:?}"), design.bundle());
            let name = serde_json::to_value(design).unwrap();
            let config = BarsConfig::parse(&json!({ "dock": { "design": name } }).to_string())
                .unwrap_or_else(|e| panic!("dock {design:?}: {e:#}"));
            assert_eq!(config.dock.design, design);
        }
    }

    #[test]
    fn every_preset_names_designs_and_leaves_the_users_settings_alone() {
        for preset in Preset::ALL {
            let bundle = preset.bundle();
            let object = bundle.as_object().expect("a preset is an object");
            for owned in [
                "version",
                "preset",
                "taskbar",
                "overrides",
                "vars",
                "look",
                "animation",
            ] {
                assert!(!object.contains_key(owned), "{preset:?} sets {owned}");
            }
            assert!(
                bundle.pointer("/dock/pinned").is_none(),
                "{preset:?} sets pins"
            );
            for bar in ["top", "dock"] {
                assert!(
                    bundle.pointer(&format!("/{bar}/design")).is_some(),
                    "{preset:?} names no {bar} design"
                );
            }
            let name = serde_json::to_value(preset).unwrap();
            let config = BarsConfig::parse(&json!({ "preset": name }).to_string())
                .unwrap_or_else(|e| panic!("{preset:?}: {e:#}"));
            assert_eq!(config.preset, preset);
        }
    }
}
