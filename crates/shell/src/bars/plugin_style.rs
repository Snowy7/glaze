//! Plugin style patches on the shell's surfaces (docs/plugins.md, "Styling"). The widgets host
//! [`publish`]es the patches the plugin runtime reported; the bars see the [`generation`] move,
//! re-resolve their tokens and restyle in place (same geometry, no new window, no reservation).
//!
//! **Precedence.** The user's explicit fields in `bars.json` (what Studio writes) win over a
//! plugin's token; a plugin's token wins over the design, preset and theme defaults. Patches stack
//! in plugin load order, a later plugin over an earlier one.
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use glaze_bars::style::Look;
use glaze_theme::Appearance;
use glaze_ui::Rgba;
use serde::Deserialize;
use serde_json::{Map, Value};

/// A token's value, as the plugin API's `token-value` (`glaze_plugin_api::wire::TokenValue`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum TokenValue {
    Color(String),
    Number(f32),
    /// A theme role name (`accent`, `label`, …).
    Tone(String),
    Flag(bool),
}

/// One plugin's patch for one surface (`top-bar`, `dock`, `menu`, `panel`, `widget`, `glass`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Patch {
    pub surface: String,
    pub tokens: Vec<(String, TokenValue)>,
}

static PATCHES: Mutex<Vec<Patch>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Replaces the live patches with `patches` (the runtime's `wire::StylePatch`es, serialized), in
/// stacking order. Bumps [`generation`] only when they changed, so calling it every tick is free.
pub fn publish<T: serde::Serialize>(patches: impl IntoIterator<Item = T>) {
    let patches: Vec<Patch> = patches
        .into_iter()
        .filter_map(|p| serde_json::to_value(p).ok().and_then(|v| serde_json::from_value(v).ok()))
        .collect();
    let mut live = PATCHES.lock().expect("plugin styles");
    if *live != patches {
        *live = patches;
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Moves whenever the published patches change.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Whether the patches changed since the bars last asked (the bars are the one caller).
pub fn take_changed() -> bool {
    static SEEN: AtomicU64 = AtomicU64::new(0);
    let now = generation();
    SEEN.swap(now, Ordering::Relaxed) != now
}

/// The tokens plugins set on `surface`, later patches over earlier ones.
pub fn tokens(surface: &str) -> Vec<(String, TokenValue)> {
    let live = PATCHES.lock().expect("plugin styles");
    let mut out: Vec<(String, TokenValue)> = Vec::new();
    for patch in live.iter().filter(|p| p.surface == surface) {
        for (name, value) in &patch.tokens {
            out.retain(|(n, _)| n != name);
            out.push((name.clone(), value.clone()));
        }
    }
    out
}

/// Whether the user's own `bars.json` sets `path` for bar `bar` (`top`/`dock`), in the shared
/// `look` or the bar's own `look`. `path[0] == "appearance"` checks the shared appearance.
fn user_sets(raw: &Map<String, Value>, bar: &str, path: &[&str]) -> bool {
    let at = |root: Option<&Value>, path: &[&str]| root.and_then(|root| path.iter().try_fold(root, |v, key| v.get(key))).is_some();
    match path {
        ["appearance", rest @ ..] => at(raw.get("appearance"), rest),
        _ => at(raw.get("look"), path) || at(raw.get(bar).and_then(|b| b.get("look")), path),
    }
}

/// `#rrggbb` (or `#rrggbbaa`) as bytes and alpha.
fn rgb(text: &str) -> Option<([u8; 3], f32)> {
    let h = text.trim().trim_start_matches('#');
    let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    match h.len() {
        6 => Some(([byte(0)?, byte(2)?, byte(4)?], 1.0)),
        8 => Some(([byte(0)?, byte(2)?, byte(4)?], byte(6)? as f32 / 255.0)),
        _ => None,
    }
}

/// A token colour: `#rrggbb` or `#rrggbbaa`.
pub fn hex(text: &str) -> Option<Rgba> {
    rgb(text).map(|(c, a)| Rgba::rgb8(c, a))
}

/// The plugin tokens of `surface` (plus `glass`, under it) for bar `bar` (`top`/`dock`), folded
/// into the bar's `look` and `appearance` wherever the user's `raw` file leaves the field unset.
/// Theme colour roles come back separately for [`colors`], after the tokens resolve.
pub fn patch_look(surface: &str, bar: &str, raw: &Map<String, Value>, look: &mut Look, appearance: &mut Appearance) -> Vec<(String, TokenValue)> {
    let mut rest = Vec::new();
    let mut all = tokens("glass");
    for (name, value) in tokens(surface) {
        all.retain(|(n, _)| *n != name);
        all.push((name, value));
    }
    let free = |path: &[&str]| !user_sets(raw, bar, path);
    for (name, value) in all {
        match (name.as_str(), &value) {
            ("tint", TokenValue::Color(c)) if free(&["appearance", "tint"]) => {
                if let Some((bytes, _)) = rgb(c) {
                    appearance.tint = Some(glaze_theme::Color(bytes));
                    // A tint needs a body to tint: a transparent design gets its glass back,
                    // unless the user chose transparent themselves.
                    if free(&["transparent"]) && free(&["background"]) {
                        look.transparent = Some(false);
                    }
                }
            }
            ("tint.opacity" | "glass.tint", TokenValue::Number(n)) if free(&["glass", "tint"]) && free(&["tint"]) => {
                look.glass.tint = Some(n.clamp(0.0, 1.0));
            }
            ("glass.clarity", TokenValue::Number(n)) if free(&["glass", "clarity"]) => look.glass.clarity = Some(n.clamp(0.0, 1.0)),
            ("glass.blur", TokenValue::Number(n)) if free(&["glass", "blur"]) => look.glass.blur = Some(n.max(0.0)),
            ("glass.refraction", TokenValue::Number(n)) if free(&["glass", "refraction"]) => look.glass.refraction = Some(*n),
            ("glass.highlight", TokenValue::Number(n)) if free(&["glass", "highlight"]) => look.glass.highlight = Some(*n),
            ("radius", TokenValue::Number(n)) if free(&["radius"]) && free(&["appearance", "radius"]) => look.radius = Some(n.clamp(0.0, 64.0)),
            ("transparent", TokenValue::Flag(on)) if free(&["transparent"]) => look.transparent = Some(*on),
            ("background", TokenValue::Color(c)) if free(&["background"]) && hex(c).is_some() => {
                if let Ok(fill) = serde_json::from_value(serde_json::json!({ "type": "solid", "color": c })) {
                    look.background = Some(fill);
                }
            }
            ("font.size", TokenValue::Number(n)) if free(&["font", "size"]) => look.font.size = Some(n.clamp(8.0, 32.0)),
            ("font.weight", TokenValue::Number(n)) if free(&["font", "weight"]) => look.font.weight = Some(n.clamp(100.0, 900.0) as u16),
            _ => rest.push((name, value)),
        }
    }
    rest
}

/// Theme colour roles (`accent`, `label`, `secondary`, `tertiary`, `separator`, `surface`, …) set
/// to a hex colour or another role. `accent` stays the user's when `bars.json` names one.
pub fn colors(tokens: &[(String, TokenValue)], raw: &Map<String, Value>, palette: &mut glaze_ui::theme::Palette) {
    let user_accent = raw.get("appearance").and_then(|a| a.get("accent")).is_some();
    for (name, value) in tokens {
        let role = |p: &glaze_ui::theme::Palette, n: &str| -> Option<Rgba> {
            Some(match n {
                "label" => p.label,
                "secondary" => p.secondary,
                "tertiary" => p.tertiary,
                "accent" => p.accent,
                "warning" => p.warning,
                "destructive" => p.destructive,
                "positive" => p.success,
                _ => return None,
            })
        };
        let color = match value {
            TokenValue::Color(c) => hex(c),
            TokenValue::Tone(t) => role(palette, t),
            _ => None,
        };
        let Some(color) = color else { continue };
        let slot = match name.as_str() {
            "accent" if !user_accent => &mut palette.accent,
            "label" => &mut palette.label,
            "secondary" => &mut palette.secondary,
            "tertiary" => &mut palette.tertiary,
            "separator" => &mut palette.separator,
            "surface" => &mut palette.surface,
            "elevated" => &mut palette.elevated,
            "hover" => &mut palette.hover,
            "pressed" => &mut palette.pressed,
            "warning" => &mut palette.warning,
            "destructive" => &mut palette.destructive,
            "positive" => &mut palette.success,
            _ => continue,
        };
        *slot = color;
    }
}

/// `height` and `icon.size` for bar `bar`: geometry, so the bar rebuilds when they change.
pub fn number(surface: &str, bar: &str, raw: &Map<String, Value>, name: &str) -> Option<f32> {
    if raw.get(bar).and_then(|b| b.get(name)).is_some() {
        return None;
    }
    tokens(surface).into_iter().rev().find_map(|(n, v)| match v {
        TokenValue::Number(x) if n == name => Some(x),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The patches are process-wide; tests that publish take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn raw(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn the_users_fields_win_and_the_plugin_fills_the_rest() {
        let _turn = TURN.lock().unwrap();
        publish([json!({ "surface": "top-bar", "tokens": [["tint", { "Color": "#3a6df0" }], ["tint.opacity", { "Number": 0.18 }], ["radius", { "Number": 4.0 }]] })]);
        let mut look = Look { transparent: Some(true), ..Look::default() };
        let mut appearance = Appearance::default();
        // The user set the radius; the tint and its strength are free.
        let user = raw(json!({ "top": { "look": { "radius": 9 } } }));
        patch_look("top-bar", "top", &user, &mut look, &mut appearance);
        assert_eq!(appearance.tint, Some(glaze_theme::Color([0x3a, 0x6d, 0xf0])));
        assert_eq!(look.glass.tint, Some(0.18));
        assert_eq!(look.transparent, Some(false), "a tint brings the design's glass back");
        assert_eq!(look.radius, None, "the user's radius stays theirs");
        // A user who chose transparent keeps it.
        let mut look = Look { transparent: Some(true), ..Look::default() };
        patch_look("top-bar", "top", &raw(json!({ "look": { "transparent": true } })), &mut look, &mut Appearance::default());
        assert_eq!(look.transparent, Some(true));
        publish(Vec::<Value>::new());
    }

    #[test]
    fn later_patches_stack_over_earlier_ones_and_publishing_the_same_is_free() {
        let _turn = TURN.lock().unwrap();
        let a = json!({ "surface": "dock", "tokens": [["label", { "Color": "#ff0000" }]] });
        let b = json!({ "surface": "dock", "tokens": [["label", { "Color": "#00ff00" }]] });
        publish([a.clone(), b.clone()]);
        let g = generation();
        publish([a, b]);
        assert_eq!(generation(), g);
        assert_eq!(tokens("dock"), vec![("label".into(), TokenValue::Color("#00ff00".into()))]);
        publish(Vec::<Value>::new());
    }
}
