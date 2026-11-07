//! Flat item and surface states, resolved once per look. Explicit surface hairlines may draw
//! over glass; the engine owns its specular rim and shadow.
use mypaper_bars::style::{BorderStyle, ColorRef, ItemStyle, Look, Paint, ShadowStyle};
use mypaper_ui::{Rgba, Theme};

#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub width: f32,
    pub color: Rgba,
}
#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub spread: f32,
    pub blur: f32,
    pub color: Rgba,
}
#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub fill: Rgba,
    pub color: Rgba,
    pub edge: Edge,
    pub scale: f32,
}

#[derive(Clone)]
pub struct Styles {
    pub items: [Item; 3],
    pub item_radius: Option<f32>,
    pub item_color: bool,
    pub configured_items: bool,
    pub press_sink: bool,
    pub edges: [Edge; 3],
    pub shadows: [Option<Shadow>; 3],
    pub outset: f32,
}

impl Styles {
    pub fn resolve(look: &Look, theme: &Theme, color: impl Fn(&ColorRef) -> Rgba) -> Self {
        let edge = |base: Edge, style: Option<&BorderStyle>| {
            style.map_or(base, |s| Edge {
                width: s.width.unwrap_or(base.width),
                color: match &s.color {
                    Some(Paint::Color(c)) => color(c),
                    _ => base.color,
                },
            })
        };
        let item = |base: Item, style: Option<&ItemStyle>| {
            style.map_or(base, |s| Item {
                fill: match &s.background {
                    Some(Paint::Color(c)) => color(c),
                    _ => base.fill,
                },
                color: s.color.as_ref().map_or(base.color, &color),
                edge: edge(base.edge, s.border.as_ref()),
                scale: s.scale.unwrap_or(base.scale),
            })
        };
        let empty = Edge {
            width: 0.0,
            color: theme.color.separator,
        };
        let rest = item(
            Item {
                fill: Rgba::TRANSPARENT,
                color: theme.color.label,
                edge: empty,
                scale: 1.0,
            },
            look.items.rest.as_ref(),
        );
        let hover = item(
            item(
                Item {
                    fill: theme.color.hover,
                    ..rest
                },
                look.items.rest.as_ref(),
            ),
            look.items.hover.as_ref(),
        );
        let active = item(
            item(
                Item {
                    fill: theme.color.pressed,
                    scale: 1.0,
                    ..rest
                },
                look.items.rest.as_ref(),
            ),
            look.items.active.as_ref(),
        );
        let body = edge(
            Edge {
                width: if theme.border.is_some() { 1.0 } else { 0.0 },
                color: theme.color.separator,
            },
            look.border.rest.as_ref(),
        );
        let shadow = |base: Option<Shadow>, style: Option<&ShadowStyle>| {
            style
                .map(|s| {
                    let base = base.unwrap_or(Shadow {
                        x: 0.0,
                        y: 2.0,
                        spread: 0.0,
                        blur: 8.0,
                        color: theme.color.shadow,
                    });
                    Shadow {
                        x: s.x.unwrap_or(base.x),
                        y: s.y.unwrap_or(base.y),
                        spread: s.spread.unwrap_or(base.spread),
                        blur: s.blur.unwrap_or(base.blur),
                        color: s.color.as_ref().map_or(base.color, &color),
                    }
                })
                .or(base)
        };
        let rest_shadow = shadow(None, look.shadow.rest.as_ref());
        let shadows = [
            rest_shadow,
            shadow(rest_shadow, look.shadow.hover.as_ref()),
            shadow(rest_shadow, look.shadow.active.as_ref()),
        ];
        let outset = shadows
            .iter()
            .flatten()
            .map(|s| s.blur + s.spread.max(0.0) + s.x.abs().max(s.y.abs()) + 1.0)
            .fold(0.0_f32, f32::max);
        Self {
            items: [rest, hover, active],
            item_radius: look.items.radius,
            configured_items: look.items.rest.is_some()
                || look.items.hover.is_some()
                || look.items.active.is_some(),
            press_sink: look.items.rest.as_ref().is_none_or(|s| s.scale.is_none())
                && look.items.active.as_ref().is_none_or(|s| s.scale.is_none()),
            item_color: [&look.items.rest, &look.items.hover, &look.items.active]
                .into_iter()
                .flatten()
                .any(|s| s.color.is_some()),
            edges: [
                body,
                edge(body, look.border.hover.as_ref()),
                edge(body, look.border.active.as_ref()),
            ],
            shadows,
            outset,
        }
    }
}

impl Item {
    pub fn mix(self, to: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let rgba = |a: Rgba, b: Rgba| {
            Rgba::new(
                lerp(a.r, b.r),
                lerp(a.g, b.g),
                lerp(a.b, b.b),
                lerp(a.a, b.a),
            )
        };
        Self {
            fill: rgba(self.fill, to.fill),
            color: rgba(self.color, to.color),
            edge: Edge {
                width: lerp(self.edge.width, to.edge.width),
                color: rgba(self.edge.color, to.edge.color),
            },
            scale: lerp(self.scale, to.scale),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn states_inherit_fields_and_width_only_edges_keep_their_color() {
        let look: Look = serde_json::from_value(serde_json::json!({"items":{"rest":{"scale":1.2,"border":{"width":2,"color":"#123456"}},"hover":{"color":"#fedcba"},"active":{"scale":0.8}}})).unwrap();
        let theme = Theme::new(
            mypaper_ui::Style::Solid,
            mypaper_ui::Scheme::Dark,
            mypaper_ui::theme::SYSTEM_BLUE,
        );
        let styles = Styles::resolve(&look, &theme, |c| {
            super::super::look::hex(c.as_str()).unwrap()
        });
        assert_eq!(styles.items[1].scale, 1.2);
        assert_eq!(styles.items[1].edge.width, 2.0);
        assert_eq!(
            styles.items[1].edge.color,
            Rgba::rgb8([0x12, 0x34, 0x56], 1.0)
        );
        assert_eq!(styles.items[2].scale, 0.8);
        assert_eq!(styles.items[0].mix(styles.items[2], 1.0).scale, 0.8);
    }
}
