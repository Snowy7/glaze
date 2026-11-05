//! Built-in integration widgets that became component plugin faces (docs/plugins.md, "Built-ins
//! that moved to plugins"). The shell applies [`migrate`] after every layout load: an instance
//! of a moved built-in becomes the face `<plugin>--<face>`, keeping its instance id (and with it
//! its storage), cell, size, look and settings. It is the same layout version, so an older
//! mypaper still reads the file; one without the plugin shows the face as missing.
use serde_json::{Map, Value};

use crate::layout::Layout;

/// One moved built-in variant.
pub struct FaceMove {
    /// The built-in widget id.
    pub widget: &'static str,
    /// Its `variant` setting, or `None` for any variant (list those rows after the exact ones).
    pub variant: Option<&'static str>,
    /// The component face, `<plugin>--<face>` (both `[a-z0-9-]`, see `manifest::valid_id`).
    pub face: &'static str,
}

/// Each port adds its own rows; the first matching row wins.
pub const MOVED: &[FaceMove] = &[
    FaceMove { widget: "discord", variant: None, face: "discord--main" },
    FaceMove { widget: "grok", variant: Some("prompt"), face: "grok--prompt" },
    FaceMove { widget: "grok", variant: Some("recent"), face: "grok--recent" },
    FaceMove { widget: "grok", variant: None, face: "grok--routines" },
    FaceMove { widget: "docker", variant: None, face: "docker--containers" },
    FaceMove { widget: "tailscale", variant: None, face: "tailscale--connection" },];

/// The face an instance of `widget` with `settings` moves to, if a row matches.
pub fn face_for<'a>(table: &'a [FaceMove], widget: &str, settings: &Map<String, Value>) -> Option<&'a str> {
    let variant = settings.get("variant").and_then(Value::as_str);
    table
        .iter()
        .find(|row| row.widget == widget && row.variant.is_none_or(|v| Some(v) == variant))
        .map(|row| row.face)
}

/// Moves every instance a row of `table` matches onto its face, when `installed(face)` says the
/// plugin providing it is installed (so a missing plugin never strands the user's widget).
/// Returns whether anything moved; the caller saves the layout then.
pub fn migrate(layout: &mut Layout, table: &[FaceMove], installed: impl Fn(&str) -> bool) -> bool {
    let mut moved = false;
    for instance in &mut layout.instances {
        let Some(face) = face_for(table, &instance.widget, &instance.settings) else { continue };
        if !crate::manifest::valid_id(face) || !installed(face) {
            continue;
        }
        log::info!("{}: the built-in {} moved to the plugin face {face}", instance.instance, instance.widget);
        instance.widget = face.to_owned();
        moved = true;
    }
    moved
}

/// Whether the component plugin named by `face` (`<plugin>--<face>`) is installed with that face.
pub fn face_installed(face: &str) -> bool {
    let Some((plugin, named)) = face.split_once("--") else { return false };
    crate::registry::find_plugin(plugin).is_some_and(|(manifest, _)| manifest.faces.iter().any(|f| f.id == named))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &[FaceMove] = &[
        FaceMove { widget: "t3", variant: Some("limits"), face: "t3--limits" },
        FaceMove { widget: "t3", variant: None, face: "t3--running" },
        FaceMove { widget: "obs", variant: None, face: "obs--controls" },
    ];

    fn layout(json: &str) -> Layout {
        Layout::parse(json).unwrap()
    }

    #[test]
    fn moves_variants_to_faces_and_keeps_the_instance() {
        let mut layout = layout(
            r#"{"version":2,"instances":[
                {"instance":"t3-1","widget":"t3","monitor":"M","cell":[1,2],"size":[8,9],"settings":{"variant":"limits"}},
                {"instance":"t3-2","widget":"t3","monitor":"M","cell":[0,0],"size":[8,5],"settings":{"variant":"running"}},
                {"instance":"clock-1","widget":"clock","monitor":"M","cell":[0,0],"size":[4,4]}]}"#,
        );
        assert!(migrate(&mut layout, TABLE, |_| true));
        let [limits, running, clock] = &layout.instances[..] else { panic!() };
        assert_eq!((limits.instance.as_str(), limits.widget.as_str(), limits.cell, limits.size), ("t3-1", "t3--limits", [1, 2], [8, 9]));
        assert_eq!(limits.settings.get("variant").and_then(Value::as_str), Some("limits"), "settings are kept");
        assert_eq!(running.widget, "t3--running", "the variant-less row is the fallback");
        assert_eq!(clock.widget, "clock");
        let saved = serde_json::to_string(&layout).unwrap();
        assert_eq!(Layout::parse(&saved).unwrap(), layout, "the moved layout still parses (valid ids)");
        assert!(!migrate(&mut layout, TABLE, |_| true), "nothing left to move");
    }

    #[test]
    fn a_missing_plugin_leaves_the_built_in() {
        let mut layout = layout(r#"{"version":2,"instances":[{"instance":"obs-1","widget":"obs","monitor":"M","cell":[0,0],"size":[4,4]}]}"#);
        assert!(!migrate(&mut layout, TABLE, |face| face != "obs--controls"));
        assert_eq!(layout.instances[0].widget, "obs");
    }

    #[test]
    fn every_row_names_a_valid_face() {
        for row in MOVED {
            assert!(crate::manifest::valid_id(row.face) && row.face.split_once("--").is_some(), "{}", row.face);
        }
    }
}
