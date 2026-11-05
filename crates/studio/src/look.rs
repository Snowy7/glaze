//! The user's look (`config.json` › `look`, D2) is what Look edits. Windows styling is read from
//! it directly (core layers `look.windows` over the wallpaper's). The shell still reads its look
//! from `bars.json` (`look`, `appearance`) and `widgets.json` (`appearance`), so each look edit
//! also writes the keys those files use, in the same undo step: a style switch, a material drag
//! or a colour shows on the desktop at once. Only the keys behind the look paths an edit touched
//! are written, so anything else in those files is left alone.

use mypaper_config::Look;
use mypaper_config::look::{FamilyLook, Icons, Material, Scheme, Style, Tone};
use mypaper_config::Paint;
use serde_json::{Value, json};

use crate::docs::{DocId, Docs, Op, apply_op};

/// One key of a shell file and the look path it comes from.
struct Key {
    doc: DocId,
    path: &'static str,
    from: &'static str,
    value: Option<Value>,
}

/// The shell-file ops that follow `ops` on the user's look.
pub fn projection_after(docs: &Docs, ops: &[Op]) -> Vec<Op> {
    let mut config = docs.doc(DocId::Config).raw.clone();
    let touched: Vec<&str> = ops
        .iter()
        .filter(|o| o.doc == DocId::Config && o.path.starts_with("/look"))
        .map(|o| o.path.as_str())
        .collect();
    if touched.is_empty() {
        return Vec::new();
    }
    for op in ops.iter().filter(|o| o.doc == DocId::Config) {
        apply_op(&mut config, op);
    }
    let look: Look = config
        .get("look")
        .and_then(|l| serde_json::from_value(l.clone()).ok())
        .unwrap_or_default();
    keys(&look)
        .into_iter()
        .filter(|k| {
            touched
                .iter()
                .any(|t| t.starts_with(k.from) || k.from.starts_with(*t))
        })
        .filter(|k| docs.error(k.doc).is_none())
        .map(|k| Op {
            doc: k.doc,
            path: k.path.into(),
            value: k.value,
        })
        .collect()
}

fn paint(p: Paint) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// Every shell key the look drives, with its value (`None`: back to the wallpaper's).
fn keys(look: &Look) -> Vec<Key> {
    let bars = family(look, look.bars.as_ref());
    let widgets = family(look, look.widgets.as_ref());
    let clear = |m: &Material| m.clarity.map(|c| json!(round(1.0 - c)));
    let mut keys = vec![
        // The bars' style and material.
        Key {
            doc: DocId::Bars,
            path: "/look/style",
            from: "/look/style",
            value: bars.0.map(|s| json!(bar_style(s))),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/family",
            from: "/look/style",
            value: (bars.0 == Some(Style::Fluent)).then(|| json!("fluent")),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/glass/tint",
            from: "/look/material/clarity",
            value: clear(&bars.1),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/tint",
            from: "/look/material/clarity",
            value: bars
                .1
                .clarity
                .filter(|_| matches!(bars.0, Some(Style::Mono | Style::Oneui | Style::Fluent)))
                .map(|c| json!(round((1.0 - c).max(0.05)))),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/glass/blur",
            from: "/look/material/blur",
            value: bars.1.blur.map(|v| json!(v)),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/glass/refraction",
            from: "/look/material/refraction",
            value: bars.1.refraction.map(|v| json!(v)),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/glass/highlight",
            from: "/look/material/highlight",
            value: bars.1.highlight.map(|v| json!(v)),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/scheme",
            from: "/look/appearance",
            value: look.appearance.map(|s| {
                json!(match s {
                    Scheme::Light => "light",
                    Scheme::Dark => "dark",
                    Scheme::Auto => "auto",
                })
            }),
        },
        Key {
            doc: DocId::Bars,
            path: "/look/glyphs",
            from: "/look/text/icons",
            value: look.text.icons.map(|i| {
                json!(match i {
                    Icons::Fluent => "fluent",
                    Icons::Lucide => "default",
                })
            }),
        },
        Key {
            doc: DocId::Bars,
            path: "/icons",
            from: "/look/text/icon_pack",
            value: look.text.icon_pack.clone().map(Value::String),
        },
        // Widgets: the surface follows the style.
        Key {
            doc: DocId::Widgets,
            path: "/appearance/surface",
            from: "/look/style",
            value: widgets.0.map(|s| {
                // The style draws the body (Mono and Solid without glass); "tint" is not a
                // widgets.json surface and made the whole file fail to load.
                json!(match s {
                    Style::Solid => "solid",
                    _ => "glass",
                })
            }),
        },
        Key {
            doc: DocId::Widgets,
            path: "/appearance/style",
            from: "/look/style",
            value: widgets
                .0
                .map(|s| json!(if s == Style::Liquid { "liquid" } else { "minimal" })),
        },
        Key {
            doc: DocId::Widgets,
            path: "/appearance/opacity",
            from: "/look/material/clarity",
            value: clear(&widgets.1),
        },
        Key {
            doc: DocId::Widgets,
            path: "/appearance/blur",
            from: "/look/material/blur",
            value: widgets.1.blur.map(|v| json!(v)),
        },
    ];
    // What both families share: colour, type and shape.
    for doc in [DocId::Bars, DocId::Widgets] {
        let tint = if doc == DocId::Bars { bars.1.tint } else { widgets.1.tint };
        keys.extend([
            Key {
                doc,
                path: "/appearance/tint",
                from: "/look/material/tint",
                // `auto` has no fixed colour for the shell's tint: the wallpaper's look decides.
                value: tint.and_then(|t| match t {
                    Paint::Color(c) => Some(json!(c.hex())),
                    Paint::Auto => None,
                }),
            },
            Key {
                doc,
                path: "/appearance/accent",
                from: "/look/accent",
                value: look.accent.map(paint),
            },
            Key {
                doc,
                path: "/appearance/font",
                from: "/look/text/font",
                value: look.text.font.clone().map(Value::String),
            },
            Key {
                doc,
                path: "/appearance/scale",
                from: "/look/text/size",
                value: look.text.size.map(|v| json!(v)),
            },
            Key {
                doc,
                path: "/appearance/shadow",
                from: "/look/text/shadow",
                value: look.text.shadow.map(|v| json!(v)),
            },
            Key {
                doc,
                path: "/appearance/text",
                from: "/look/text/tone",
                value: look.text.tone.and_then(|t| match t {
                    Tone::Light => Some(json!("light")),
                    Tone::Dark => Some(json!("dark")),
                    Tone::Auto => None,
                }),
            },
            Key {
                doc,
                path: "/appearance/radius",
                from: "/look/shape/corners",
                value: look.shape.corners.map(|v| json!(v)),
            },
            Key {
                doc,
                path: "/appearance/border",
                from: "/look/shape/edge",
                value: look.shape.edge.map(|v| json!(v)),
            },
        ]);
    }
    keys
}

/// A family's style and material: its own over the look's.
fn family(look: &Look, own: Option<&FamilyLook>) -> (Option<Style>, Material) {
    match own {
        Some(f) => (f.style.or(look.style), f.material.over(&look.material)),
        None => (look.style, look.material),
    }
}

/// The bars' style for a look style: the same five.
fn bar_style(s: Style) -> &'static str {
    match s {
        Style::Liquid => "liquid",
        Style::Oneui => "oneui",
        Style::Fluent => "fluent",
        Style::Solid => "solid",
        Style::Mono => "mono",
    }
}

fn round(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

/// Which material controls a style uses (spec 4.3): clarity, blur, refraction, highlight.
pub fn uses(style: Style) -> [bool; 4] {
    match style {
        Style::Liquid => [true, true, true, true],
        Style::Oneui | Style::Fluent => [true, true, false, false],
        Style::Mono => [true, false, false, false],
        Style::Solid => [false, false, false, false],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(path: &str, value: Value) -> Op {
        Op {
            doc: DocId::Config,
            path: path.into(),
            value: Some(value),
        }
    }

    #[test]
    fn a_look_edit_writes_only_the_shell_keys_behind_it() {
        let docs = Docs::fixture(["{}", r#"{ "version": 1, "appearance": { "font": "Inter" } }"#, "{}", "{}"]);
        let ops = projection_after(&docs, &[op("/look/material/clarity", json!(0.7))]);
        let paths: Vec<_> = ops.iter().map(|o| (o.doc, o.path.as_str())).collect();
        assert!(paths.contains(&(DocId::Bars, "/look/glass/tint")));
        assert!(paths.contains(&(DocId::Widgets, "/appearance/opacity")));
        assert!(!paths.iter().any(|(_, p)| *p == "/appearance/font"), "the font is untouched");
        let tint = ops.iter().find(|o| o.path == "/look/glass/tint").unwrap();
        assert_eq!(tint.value, Some(json!(0.3)));
    }

    #[test]
    fn styles_map_onto_what_the_shell_draws_and_families_override() {
        let docs = Docs::fixture(["{}", r#"{ "version": 1 }"#, "{}", "{}"]);
        let ops = projection_after(
            &docs,
            &[
                op("/look/style", json!("fluent")),
                op("/look/widgets", json!({ "style": "solid" })),
            ],
        );
        let get = |doc, path: &str| ops.iter().find(|o| o.doc == doc && o.path == path).and_then(|o| o.value.clone());
        assert_eq!(get(DocId::Bars, "/look/style"), Some(json!("fluent")));
        assert_eq!(get(DocId::Bars, "/look/family"), Some(json!("fluent")));
        assert_eq!(get(DocId::Widgets, "/appearance/surface"), Some(json!("solid")));
        // Back to the wallpaper's: the keys are removed.
        let reset = projection_after(&docs, &[Op { doc: DocId::Config, path: "/look/style".into(), value: None }]);
        assert!(reset.iter().filter(|o| o.path == "/look/style").all(|o| o.value.is_none()));
    }
}
