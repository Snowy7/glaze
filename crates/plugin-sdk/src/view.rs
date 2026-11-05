//! Declarative views: a tree of kit nodes the host draws natively (docs/plugins.md, "Views").
//! Plugins never draw pixels; they describe what to show and the host draws it with the kit.

use serde::{Deserialize, Serialize};

/// A face's size class, from the instance's tile size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SizeClass {
    Small,
    #[default]
    Medium,
    Large,
}

/// A theme color role, or `#rrggbb` for brand fills. Roles follow the theme; hex doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Label,
    Secondary,
    Tertiary,
    Accent,
    Warning,
    Destructive,
    Positive,
    #[serde(untagged)]
    Hex(String),
}

/// The kit's text roles (`mypaper_ui::theme::TextRole`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    LargeTitle,
    Title1,
    Title2,
    Title3,
    Headline,
    #[default]
    Body,
    Callout,
    Subheadline,
    Footnote,
    Caption,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub fraction: f32,
    pub color: Tone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub label: String,
    pub values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Tone>,
}

/// One node. Unknown kinds (from a newer SDK minor) deserialize as [`View::Unknown`] and draw as
/// nothing, so hosts stay compatible with newer plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    Stack {
        children: Vec<View>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<f32>,
    },
    Row {
        children: Vec<View>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<f32>,
    },
    Grid {
        children: Vec<View>,
        columns: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<f32>,
    },
    Text {
        text: String,
        #[serde(default)]
        role: Role,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Tone>,
        #[serde(default)]
        align: Align,
    },
    Icon {
        icon: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Tone>,
    },
    Button {
        label: String,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        #[serde(default)]
        primary: bool,
    },
    Field {
        placeholder: String,
        action: String,
    },
    ListRow {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trailing: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },
    Stat {
        label: String,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Ring {
        fraction: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Tone>,
    },
    /// The kit's quota bar: `left` fills from the start, the used rest is hatched, `pace` marks
    /// where an even burn would be, `badge` is a monogram, `reset` a countdown chip at the end.
    LimitBar {
        left: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pace: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Tone>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        badge: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset: Option<String>,
    },
    SegmentedBar {
        segments: Vec<Segment>,
    },
    Sparkline {
        values: Vec<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Tone>,
    },
    /// The kit's area chart: series overlaid in order (largest first), first/middle/last x
    /// labels, ticks printed with `unit` appended. Takes the free height.
    AreaChart {
        series: Vec<Series>,
        #[serde(default)]
        x_labels: [String; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    /// A card: the kit's grouped surface around `child`, with an optional section title.
    Card {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        child: Box<View>,
    },
    Empty {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Spacer,
    #[serde(other)]
    Unknown,
}

/// What a face shows: the body tree plus the chrome's footer caption.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub body: View,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    /// A refresh button in the header; clicking it sends the action `"refresh"`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refresh: bool,
}

impl View {
    pub fn text(text: impl Into<String>, role: Role) -> Self {
        Self::Text {
            text: text.into(),
            role,
            color: None,
            align: Align::Start,
        }
    }

    pub fn tinted(text: impl Into<String>, role: Role, color: Tone) -> Self {
        Self::Text {
            text: text.into(),
            role,
            color: Some(color),
            align: Align::Start,
        }
    }

    pub fn stack(children: impl IntoIterator<Item = View>) -> Self {
        Self::Stack {
            children: children.into_iter().collect(),
            gap: None,
        }
    }

    pub fn row(children: impl IntoIterator<Item = View>) -> Self {
        Self::Row {
            children: children.into_iter().collect(),
            gap: None,
        }
    }

    pub fn empty(icon: &str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Empty {
            icon: Some(icon.into()),
            title: title.into(),
            detail: Some(detail.into()),
        }
    }

    pub fn button(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self::Button {
            label: label.into(),
            action: action.into(),
            icon: None,
            primary: false,
        }
    }

    /// The face around this body, without a footer.
    pub fn face(self) -> Face {
        Face {
            body: self,
            footer: None,
            refresh: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn views_round_trip_with_kind_tags() {
        let view = View::stack([
            View::tinted("3 running", Role::Headline, Tone::Accent),
            View::LimitBar {
                left: 0.6,
                pace: Some(0.5),
                color: Some(Tone::Hex("#542e25".into())),
                badge: Some("IS".into()),
                label: Some("60%".into()),
                reset: Some("3h 2m".into()),
            },
        ]);
        let wire = serde_json::to_value(&view).unwrap();
        assert_eq!(wire["kind"], "stack");
        assert_eq!(wire["children"][0]["color"], "accent");
        assert_eq!(wire["children"][1]["color"], "#542e25");
        assert_eq!(serde_json::from_value::<View>(wire).unwrap(), view);
    }

    #[test]
    fn unknown_kinds_from_newer_plugins_draw_as_nothing() {
        let tree = json!({ "kind": "row", "children": [{ "kind": "hologram", "x": 1 }, { "kind": "spacer" }] });
        let View::Row { children, .. } = serde_json::from_value(tree).unwrap() else {
            panic!("row");
        };
        assert_eq!(children, [View::Unknown, View::Spacer]);
    }
}
