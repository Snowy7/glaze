//! Panel row anatomy on the existing kit's drawing, focus, accessibility and hit machinery.

use crate::panel::tokens::radius;
use glaze_ui::controls::{Align, Behavior, Font, Glyph, key};
use glaze_ui::{Point, Rect, Rgba, Role, TextStyle};

use crate::actions::{Action, Edit, FieldId};
use crate::app::{App, field_key};
use crate::settings::{Kind, Setting};
use crate::theme::glyph;
use crate::ui::{B, Col};

pub fn font(b: &B, size: f32, weight: u16) -> TextStyle {
    let mut style = b.font(Font::Body);
    style.size *= size / 14.0;
    style.weight = weight;
    style.optical = style.size / b.s * 0.75;
    style
}

pub fn text(b: &mut B, r: Rect, label: &str, size: f32, weight: u16, ink: Rgba, align: Align) {
    let style = font(b, size, weight);
    b.text_in(r, label, &style, ink, align);
}

pub fn height(b: &B, base: f32) -> f32 {
    b.px(base * b.t.text_scale)
}

pub fn row_height(b: &B, caption: &str) -> f32 {
    height(
        b,
        b.kit.metrics.row + if caption.is_empty() { 0.0 } else { 12.0 },
    )
}

pub use glaze_ui::controls::format_value as unit;

/// Where a picture card's label starts: flush with the picture, just below it.
pub fn card_label_top(b: &B, picture: Rect) -> f32 {
    picture.bottom + b.px(8.0)
}

/// A 2 DIP ring around a card's picture, with a 2 DIP clear gap and a concentric centerline.
pub fn card_selection(b: &mut B, picture: Rect) {
    b.stroke(
        picture.inflate(b.px(3.0)),
        b.px(radius::MEDIUM + 3.0),
        b.px(2.0),
        b.p.ink1,
    );
}

/// A card is its picture, rounded on its own, with the label under it: no plate behind either.
/// Hovering washes the picture; selection is [`card_selection`] around it.
pub fn card_picture(b: &mut B, picture: Rect, hot: bool, draw: impl FnOnce(&mut B, Rect)) {
    let start = b.list.ops.len();
    b.fill(picture, 0.0, b.p.well);
    draw(b, picture);
    if hot {
        let wash = if b.p.dark { Rgba::new(1.0, 1.0, 1.0, 0.10) } else { Rgba::new(0.0, 0.0, 0.0, 0.08) };
        b.fill(picture, 0.0, wash);
    }
    let mut ops = b.list.ops.split_off(start);
    fn square_canvas(ops: &mut [glaze_ui::Op], picture: Rect) {
        for op in ops {
            match op {
                glaze_ui::Op::RoundedImage { rect, radius, .. }
                | glaze_ui::Op::Fill { rect, radius, .. }
                    if (rect.left - picture.left).abs() < 1.0
                        && (rect.top - picture.top).abs() < 1.0
                        && (rect.right - picture.right).abs() < 1.0
                        && (rect.bottom - picture.bottom).abs() < 1.0 =>
                {
                    *radius = 0.0
                }
                glaze_ui::Op::Clipped { ops, .. } => {
                    square_canvas(std::sync::Arc::make_mut(ops), picture)
                }
                _ => {}
            }
        }
    }
    square_canvas(&mut ops, picture);
    b.list.ops.push(glaze_ui::Op::RoundedClipped {
        rect: picture,
        radius: b.px(radius::MEDIUM),
        ops: vec![glaze_ui::Op::Clipped {
            rect: picture,
            ops: ops.into(),
        }]
        .into(),
    });
}

/// Distance between two boundaries, used for a flexible gap rather than a separator.
pub fn spacing_glyph(b: &mut B, rect: Rect) {
    let x = rect.left + (rect.width() - b.px(16.0)) / 2.0;
    let y = rect.top + rect.height() / 2.0;
    let scale = b.s;
    let point = |dx, dy| Point::new(x + dx * scale, y + dy * scale);
    let ink = b.p.ink2;
    let lines = [
        ((1.0, -5.0), (1.0, 5.0)),
        ((15.0, -5.0), (15.0, 5.0)),
        ((4.0, 0.0), (12.0, 0.0)),
        ((4.0, 0.0), (6.0, -2.0)),
        ((4.0, 0.0), (6.0, 2.0)),
        ((12.0, 0.0), (10.0, -2.0)),
        ((12.0, 0.0), (10.0, 2.0)),
    ];
    for (from, to) in lines {
        b.line(point(from.0, from.1), point(to.0, to.1), b.px(1.0), ink);
    }
}

/// All inline sliders reserve the kit's common label column.
pub fn label_width(b: &B, c: &Col) -> f32 {
    b.label_width(c.w)
}

pub fn displays(app: &App, b: &mut B, c: &mut Col) {
    // In the standalone frame the display picker belongs to its stage.
    if app.panel.as_ref().is_some_and(|state| state.windowed)
        && crate::view::has_stage(super::current_page(app))
    {
        return;
    }
    if app.monitors.len() < 2 {
        return;
    }
    let order = display_order(app);
    let labels: Vec<_> = order.iter().map(|&i| display_name(app, i)).collect();
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let style = b.font(Font::Caption);
    let needed: f32 = labels
        .iter()
        .map(|label| b.measure(label, &style).0 + b.px(20.0))
        .sum();
    if labels.len() > 4 || needed > c.w - b.px(4.0) {
        group(b, c, "", |b, c| {
            super::editors::select(
                b,
                c,
                "Display",
                serde_json::json!(app.nav.display),
                Edit::new(crate::docs::DocId::Studio, "/panel/display", "Display"),
                labels
                    .iter()
                    .zip(&order)
                    .map(|(label, index)| (serde_json::json!(index), (*label).to_owned()))
                    .collect(),
            )
        });
        return;
    }
    // "Display" says what Left and Right mean; the switch is as wide as its labels.
    let caption = font(b, 13.0, 500);
    let label_w = b.measure("Display", &caption).0 + b.px(12.0);
    let row = Rect::new(c.x, c.y + b.px(6.0), c.w, height(b, 30.0));
    text(b, Rect::new(row.left, row.top, label_w, row.height()), "Display", 13.0, 500, b.p.ink2, Align::Start);
    let w = b.segmented_width(&labels).max(b.px(200.0)).min(c.w - label_w);
    let r = Rect::new(row.left + label_w, row.top, w, row.height());
    b.segmented(
        key(("panel-displays", c.view)),
        r,
        &labels,
        order.iter().position(|&i| i == app.nav.display),
        "Display",
        true,
        |i| Action::Panel(super::Command::Display(order[i])),
    );
    c.y = r.bottom + b.px(4.0);
}

pub fn display_order(app: &App) -> Vec<usize> {
    let mut order: Vec<_> = (0..app.monitors.len()).collect();
    order.sort_by_key(|&i| (app.monitors[i].rect.x, app.monitors[i].rect.y));
    order
}

/// Place names remain stable when Windows enumerates the monitors in another order.
pub fn display_name(app: &App, index: usize) -> String {
    let order = display_order(app);
    let rank = order.iter().position(|&i| i == index).unwrap_or(0);
    if order.len() < 2 {
        return "Main".into();
    }
    if order.len() == 2 {
        return if rank == 0 { "Left" } else { "Right" }.into();
    }
    if app.monitors.get(index).is_some_and(|m| m.primary) {
        return "Main".into();
    }
    let main = order
        .iter()
        .position(|&i| app.monitors[i].primary)
        .unwrap_or(0);
    if rank < main {
        format!("Left {}", main - rank)
    } else {
        format!("Right {}", rank - main)
    }
}

pub fn flash_hovered_display(app: &App, b: &B) {
    let hovered = app.input.pointer.and_then(|point| {
        b.targets.iter().rev().find_map(|target| {
            if !target.visible.is_some_and(|rect| rect.contains(point)) {
                return None;
            }
            match &target.action {
                Action::Panel(super::Command::Display(index)) => Some(*index),
                _ => None,
            }
        })
    });
    super::pick_client::flash_display(app, hovered);
}

/// Hints stay until dismissed; only that explicit action writes Studio's own settings.
pub fn first_use_hint(app: &App, b: &mut B, c: &mut Col, page: super::Page) {
    if app.panel.as_ref().is_some_and(|state| state.windowed) {
        return;
    }
    use super::Page;
    let (id, message) = match page {
        Page::Root => ("home", "Click anything on your desktop to edit it"),
        Page::Widget => (
            "widget",
            "Drag to move \u{b7} Drag the corner brackets to resize",
        ),
        Page::Top | Page::Dock | Page::Module => {
            ("bar", "Drag modules by their names to move them")
        }
        _ => return,
    };
    let path = format!("/panel_hints/{id}");
    if app
        .get(crate::docs::DocId::Studio, &path)
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return;
    }
    let top = c.y + b.px(4.0);
    let w = c.w - b.px(32.0);
    let style = font(b, 12.0, 400);
    let h = b
        .paragraph(c.x, top, w, message, &style, height(b, 18.0), b.p.ink2)
        .max(b.px(24.0));
    icon_button(
        b,
        Rect::new(c.right() - b.px(24.0), top, b.px(24.0), b.px(24.0)),
        glaze_ui::icons::Icon::Close.into(),
        &format!("Dismiss {id} hint"),
        true,
        Action::Set(
            Edit::new(crate::docs::DocId::Studio, path, "Dismiss hint"),
            serde_json::json!(true),
        ),
    );
    c.y = top + h + b.px(8.0);
}

#[cfg(test)]
mod display_hint_tests {
    use super::*;

    #[test]
    fn place_names_and_display_actions_follow_physical_order() {
        let mut ui = glaze_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.monitors[0].rect.x = 1920;
        app.monitors[1].rect.x = 0;
        assert_eq!(display_order(&app), [1, 0]);
        assert_eq!(display_name(&app, 1), "Left");
        assert_eq!(display_name(&app, 0), "Right");
        let built = super::super::build(&app, &mut ui.text);
        let actions: Vec<_> = built
            .targets
            .iter()
            .filter_map(|target| match target.action {
                Action::Panel(super::super::Command::Display(index)) => Some(index),
                _ => None,
            })
            .collect();
        assert_eq!(actions, [1, 0]);
    }

    #[test]
    fn dismissing_one_hint_changes_only_studio_settings_and_survives_a_rebuild() {
        let mut ui = glaze_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        let before = [
            crate::docs::DocId::Config,
            crate::docs::DocId::Bars,
            crate::docs::DocId::Widgets,
        ]
        .map(|id| app.docs.doc(id).raw.clone());
        let built = super::super::build(&app, &mut ui.text);
        let dismiss = built
            .targets
            .iter()
            .find(|target| {
                matches!(&target.action,
            Action::Set(edit,_) if edit.path.as_ref() == "/panel_hints/home")
            })
            .unwrap()
            .action
            .clone();
        app.run(dismiss);
        let built = super::super::build(&app, &mut ui.text);
        assert!(
            !built
                .list
                .hits
                .iter()
                .any(|hit| hit.label.as_ref() == "Dismiss home hint")
        );
        assert_eq!(
            app.get(crate::docs::DocId::Studio, "/panel_hints/home"),
            Some(&serde_json::json!(true))
        );
        for (id, before) in [
            crate::docs::DocId::Config,
            crate::docs::DocId::Bars,
            crate::docs::DocId::Widgets,
        ]
        .into_iter()
        .zip(before)
        {
            assert_eq!(app.docs.doc(id).raw, before);
        }
        assert_eq!(app.docs.undo_count(), 0);
    }
}

/// Read failures stay beside their editors; write failures remain visible on every page.
pub fn relevant_docs(page: super::Page) -> &'static [crate::docs::DocId] {
    use super::Page;
    use crate::docs::DocId;
    match page {
        Page::Widgets | Page::Widget | Page::AddWidget => &[DocId::Widgets],
        Page::Bars
        | Page::BarDesign
        | Page::Top
        | Page::Dock
        | Page::Module
        | Page::AddModule
        | Page::Panels
        | Page::BarPanel => &[DocId::Bars],
        Page::Connections | Page::Connection => &[
            DocId::Integrations,
            DocId::Plugins,
            DocId::Bars,
            DocId::Widgets,
        ],
        Page::History => &[DocId::Config, DocId::Bars, DocId::Widgets],
        Page::FirstRun => &[DocId::Config, DocId::Bars, DocId::Widgets, DocId::Studio],
        Page::General => &[DocId::Studio, DocId::Config],
        Page::Root
        | Page::Look
        | Page::Windows
        | Page::Performance
        | Page::DesktopMenu
        | Page::Wallpapers
        | Page::Wallpaper
        | Page::AddWallpaper => &[DocId::Config],
        _ => &[],
    }
}

pub fn save_banner(app: &App, b: &mut B, c: &mut Col, page: super::Page) {
    super::saving::banner(app, b, c, page);
    use crate::docs::ReadIssue;
    for &doc in relevant_docs(page) {
        let issue = app.docs.doc(doc).issue;
        if issue.is_none() {
            continue;
        }
        let message = app.docs.error(doc).unwrap_or("Couldn't save");
        let mut actions = vec![];
        match issue {
            Some(ReadIssue::Newer) => actions.push(("Show file", Action::ShowFile(doc))),
            Some(ReadIssue::Malformed { .. }) => {
                actions.push(("Show file", Action::ShowFile(doc)));
                actions.push(("Start over", Action::StartOver(doc)));
            }
            Some(ReadIssue::Unavailable) | None => {
                actions.push(("Retry", Action::Panel(super::Command::RetryDoc(doc))));
                actions.push((
                    "Show folder",
                    Action::Panel(super::Command::ShowFolder(doc)),
                ));
            }
        }
        feedback(b, c, message, actions);
    }
}

/// A compact alert sentence with correctly sized recovery actions.
pub fn feedback(b: &mut B, c: &mut Col, message: &str, actions: Vec<(&str, Action)>) {
    let first_op = b.list.ops.len();
    let mut right = c.right();
    for (label, action) in actions.into_iter().rev() {
        let width = b.measure(label, &font(b, 13.0, 500)).0 + b.px(16.0);
        right -= width;
        button(
            b,
            Rect::new(right, c.y + b.px(4.0), width, b.px(28.0)),
            label,
            None,
            false,
            true,
            action,
        );
        right -= b.px(4.0);
    }
    b.icon(
        Rect::new(c.x, c.y + b.px(9.0), b.px(16.0), b.px(16.0)),
        glaze_ui::icons::Icon::Warning,
        14.0,
        b.p.ink2,
    );
    let style = font(b, 11.0, 400);
    let style = glaze_ui::TextStyle {
        size: style.size * 12.0 / 11.0,
        ..style
    };
    let lines = b
        .text
        .lines(message, &style, (right - c.x - b.px(21.0)).max(1.0), 2);
    let line = b.px(15.0) * b.t.text_scale;
    let h = (lines.len() as f32 * line + b.px(8.0)).max(b.px(36.0));
    for (i, label) in lines.iter().enumerate() {
        b.text_at(
            c.x + b.px(21.0),
            c.y + (h - lines.len() as f32 * line) / 2.0 + i as f32 * line,
            label,
            &style,
            b.p.ink2,
        );
    }
    c.y += h;
    super::saving::emphasize_feedback(b, first_op);
}

pub fn title(b: &mut B, c: &mut Col, title: &str, subtitle: &str) {
    let h = height(b, 40.0);
    text(
        b,
        Rect::new(c.x, c.y, c.w, h),
        title,
        30.0,
        700,
        b.p.ink1,
        Align::Start,
    );
    c.y += h;
    if !subtitle.is_empty() {
        let h = height(b, 20.0);
        text(
            b,
            Rect::new(c.x, c.y, c.w, h),
            subtitle,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
        c.y += h;
    }
    c.y += b.px(16.0);
}

/// Keep a sub-page's return action beside its actual heading in the editor column.
pub fn page_title(app: &App, b: &mut B, c: &mut Col, heading: &str, subtitle: &str) {
    let windowed = app.panel.as_ref().is_some_and(|state| state.windowed);
    // A tab already names the page; the page starts with its content.
    if windowed && crate::view::tabbed(crate::panel::current_page(app)) {
        return;
    }
    // The window's back button sits before its breadcrumbs; the docked panel has its header's.
    title(b, c, heading, subtitle);
}

pub fn group(b: &mut B, c: &mut Col, heading: &str, content: impl FnOnce(&mut B, &mut Col)) {
    group_heading(b, c, heading, None, content);
}

pub fn icon_group(b: &mut B, c: &mut Col, heading: &str, icon: Glyph, content: impl FnOnce(&mut B, &mut Col)) {
    group_heading(b, c, heading, Some(icon), content);
}

/// A section's heading with no plate under it, for picture cards that sit on the page.
pub fn section(b: &mut B, c: &mut Col, heading: &str) {
    c.y += b.px(20.0);
    let h = height(b, 20.0);
    text(b, Rect::new(c.x, c.y, c.w, h), heading, 16.0, 600, b.p.ink1, Align::Start);
    c.y += h + b.px(12.0);
}

/// Picture cards under a heading, straight on the page: cards bring no plate of their own.
pub fn cards_section(b: &mut B, c: &mut Col, heading: &str, content: impl FnOnce(&mut B, &mut Col)) {
    section(b, c, heading);
    content(b, c);
}

fn group_heading(b: &mut B, c: &mut Col, heading: &str, icon: Option<Glyph>, content: impl FnOnce(&mut B, &mut Col)) {
    if !heading.is_empty() {
        c.y += b.px(20.0);
        let h = height(b, 20.0);
        let inset = if let Some(icon) = icon {
            b.icon(Rect::new(c.x, c.y, b.px(18.0), h), icon, 15.0, b.p.ink3);
            b.px(26.0)
        } else { 0.0 };
        text(
            b,
            Rect::new(c.x + inset, c.y, c.w - inset, h),
            heading,
            16.0,
            600,
            b.p.ink1,
            Align::Start,
        );
        c.y += h;
    }
    c.y += b.px(8.0);
    let start = c.y;
    let at = b.list.ops.len();
    let mut inner = Col {
        x: c.x + b.px(b.kit.metrics.pad),
        y: start + b.px(10.0),
        w: c.w - b.px(b.kit.metrics.pad * 2.0),
        view: c.view,
    };
    b.group(key(("panel-group", c.view, heading, start as i32)), |b| {
        content(b, &mut inner)
    });
    // The plate is inserted under its contents after their height is known.
    b.list.ops.insert(
        at,
        glaze_ui::Op::Fill {
            rect: Rect::new(c.x, start, c.w, inner.y - start + b.px(10.0)),
            radius: b.px(b.kit.radius.card),
            color: b.p.panel,
        },
    );
    b.fill(
        Rect::new(inner.x, inner.y - 1.0, inner.w, 1.0),
        0.0,
        b.p.panel,
    );
    c.y = inner.y + b.px(10.0);
}

pub fn button(
    b: &mut B,
    r: Rect,
    label: &str,
    icon: Option<Glyph>,
    primary: bool,
    enabled: bool,
    action: Action,
) {
    let primary = primary && enabled;
    let k = key(("panel-button", label));
    let hot = b.hovered(k) || b.pressed(k);
    b.fill(
        r,
        b.control_radius(r),
        if primary {
            b.p.solid
        } else if hot && enabled {
            b.p.hover
        } else {
            b.p.fill
        },
    );
    if !primary {
        b.stroke(r, b.control_radius(r), b.px(1.0), b.p.line);
    }
    let ink = if !enabled {
        b.p.ink3
    } else if primary {
        b.p.on_solid
    } else {
        b.p.ink1
    };
    let left = if let Some(g) = icon {
        b.icon(
            Rect::new(r.left + b.px(9.0), r.top, b.px(14.0), r.height()),
            g,
            13.0,
            ink,
        );
        r.left + b.px(27.0)
    } else {
        r.left
    };
    text(
        b,
        Rect::new(
            left,
            r.top,
            r.right - left - if icon.is_some() { b.px(8.0) } else { 0.0 },
            r.height(),
        ),
        label,
        13.0,
        500,
        ink,
        Align::Center,
    );
    if enabled {
        b.hit(k, r, Role::Button, label, action, Behavior::Click);
        b.focus_ring(k, r, b.control_radius(r));
    } else {
        // Keep deferred actions discoverable to screen readers without making them focus stops.
        b.list.hits.push(glaze_ui::Hit {
            id: u32::MAX - b.list.hits.len() as u32,
            rect: r,
            role: Role::Button,
            label: label.into(),
            enabled: false,
            actionable: false,
            checked: None,
        });
    }
}

pub fn icon_button(b: &mut B, r: Rect, icon: Glyph, label: &str, enabled: bool, action: Action) {
    let k = key(("panel-icon", label));
    if enabled {
        b.wash(k, r, b.control_radius(r));
    }
    b.icon(
        r,
        icon,
        16.0,
        if enabled {
            if b.hovered(k) { b.p.ink1 } else { b.p.ink3 }
        } else {
            b.p.ink3.with_alpha(0.6)
        },
    );
    if enabled {
        b.hit(k, r, Role::Button, label, action, Behavior::Click);
        b.focus_ring(k, r, b.control_radius(r));
    } else {
        b.list.hits.push(glaze_ui::Hit {
            id: u32::MAX - b.list.hits.len() as u32,
            rect: r,
            role: Role::Button,
            label: label.into(),
            enabled: false,
            actionable: false,
            checked: None,
        });
    }
}

/// A compact dropdown for toolbars: the current value and a chevron in a filled box. `action`
/// opens its menu (`Action::Menu` or `Action::Choose`). Returns the width it took.
pub fn dropdown(b: &mut B, k: glaze_ui::controls::Key, x: f32, y: f32, label: &str, value: &str, action: Action) -> f32 {
    let style = font(b, 13.0, 500);
    let w = b.measure(value, &style).0 + b.px(42.0);
    let r = Rect::new(x, y, w, height(b, 32.0));
    b.fill(r, b.px(radius::CONTROL), if b.hovered(k) { b.p.hover } else { b.p.fill });
    text(b, Rect::new(r.left + b.px(12.0), r.top, w - b.px(36.0), r.height()), value, 13.0, 500, b.p.ink1, Align::Start);
    b.icon(Rect::new(r.right - b.px(26.0), r.top, b.px(16.0), r.height()), glyph::CHEVRON_DOWN, 14.0, b.p.ink3);
    b.hit(k, r, Role::ComboBox, &format!("{label} {value}"), action, Behavior::Click);
    b.focus_ring(k, r, b.px(radius::CONTROL));
    w
}

/// A 52-DIP row, or 64 with a caption. Values yield space to the label and ellipsize.
pub fn row(
    b: &mut B,
    c: &mut Col,
    icon: Option<Glyph>,
    label: &str,
    value: &str,
    caption: &str,
    checked: Option<bool>,
    action: Action,
) {
    row_content(b, c, icon, None, false, label, value, caption, checked, action);
}

/// App identities reserve a centered 32 DIP box before either line of text.
pub fn artwork_row(
    b: &mut B, c: &mut Col, icon: Glyph, image: Option<&std::sync::Arc<glaze_ui::Image>>,
    label: &str, value: &str, caption: &str, action: Action,
) {
    row_content(b, c, Some(icon), image, true, label, value, caption, None, action);
}

fn row_content(
    b: &mut B, c: &mut Col, icon: Option<Glyph>, image: Option<&std::sync::Arc<glaze_ui::Image>>,
    boxed: bool, label: &str, value: &str, caption: &str, checked: Option<bool>, action: Action,
) {
    let h = row_height(b, caption);
    let r = Rect::new(c.x, c.y, c.w, h);
    let k = key(("panel-row", c.view, label, caption));
    let choice = matches!(&action, Action::Choose(_) | Action::Menu(_));
    let on = checked == Some(true);
    let area = b.list_row_area(r);
    if on {
        b.fill(area, b.px(radius::CONTROL), b.p.selected);
    } else {
        b.wash(k, area, b.px(radius::CONTROL));
    }
    let mut x = r.left;
    if let Some(g) = icon {
        let gw = b.px(if boxed { 32.0 } else { 18.0 });
        let tile = Rect::new(x, c.y + (h - gw) / 2.0, gw, gw);
        if let Some(image) = image {
            crate::stage::image_round(b, tile.inflate(-b.px(2.0)), image, 1.0, b.px(7.0));
        } else {
            if boxed { b.fill(tile, b.px(8.0), b.p.well); }
            b.icon(
                tile,
                g,
                16.0,
                if on || b.hovered(k) { b.p.ink1 } else { b.p.ink3 },
            );
        }
        x += gw + b.px(if boxed { 12.0 } else { 10.0 });
    }
    let tail_w = height(b, 16.0);
    let mut tail = Rect::new(r.right - tail_w, r.top, tail_w, h);
    // A select's value and chevron sit in a dropdown box at the row's end.
    let dropdown = choice && checked.is_none() && !value.is_empty();
    if dropdown {
        tail = Rect::new(r.right - tail_w - b.px(10.0), r.top, tail_w, h);
    }
    let end = tail.left - b.px(8.0);
    let value_w = if value.is_empty() {
        0.0
    } else {
        let style = font(b, 13.0, 400);
        b.measure(value, &style)
            .0
            .min((end - x - b.px(140.0)).max(0.0))
    };
    if dropdown {
        let left = end - value_w - b.px(12.0);
        let boxed = Rect::new(left, r.top + (h - height(b, 32.0)) / 2.0, r.right - left, height(b, 32.0));
        b.fill(boxed, b.px(radius::CONTROL), b.p.fill);
    }
    if let Some(checked) = checked {
        if checked {
            b.icon(tail, glyph::CHECK, 16.0, b.p.ink1);
        }
    } else {
        b.icon(
            tail,
            if choice {
                glyph::CHEVRON_DOWN
            } else {
                glyph::CHEVRON_RIGHT
            },
            15.0,
            b.p.ink3,
        );
    }
    let label_end = end - value_w - if value_w > 0.0 { b.px(12.0) } else { 0.0 };
    let label_h = if caption.is_empty() {
        h
    } else {
        height(b, 22.0)
    };
    let y = if caption.is_empty() {
        c.y
    } else {
        c.y + (h - height(b, 40.0)) / 2.0
    };
    text(
        b,
        Rect::new(x, y, (label_end - x).max(0.0), label_h),
        label,
        13.0,
        if on { 600 } else { 500 },
        b.p.ink1,
        Align::Start,
    );
    if !caption.is_empty() {
        text(
            b,
            Rect::new(x, y + label_h, end - x, height(b, 18.0)),
            caption,
            12.0,
            400,
            if on { b.p.ink2 } else { b.p.ink3 },
            Align::Start,
        );
    }
    if value_w > 0.0 {
        text(
            b,
            Rect::new(end - value_w, c.y, value_w, h),
            value,
            13.0,
            400,
            b.p.ink2,
            Align::End,
        );
    }
    let label = format!("{label} {value} {caption}");
    b.hit_checked(
        k,
        area,
        if checked.is_some() {
            Role::RadioButton
        } else if choice {
            Role::ComboBox
        } else {
            Role::Button
        },
        &label,
        checked,
        action,
        Behavior::Click,
    );
    b.focus_ring(k, area, b.px(radius::CONTROL));
    b.hline(c.x, c.right(), r.bottom - 1.0, b.p.line.with_alpha(0.45));
    c.y += h;
}

pub fn setting(app: &App, b: &mut B, c: &mut Col, id: &'static str, label: &str, caption: &str) {
    let s = Setting::get(id).expect("panel uses known settings");
    crate::ui::anchor(app, id, c.view, c.y);
    let value = app.value(s);
    if id == "font" {
        crate::panel::pickers::font(
            app,
            b,
            c,
            Edit::new(s.doc, s.path, s.label),
            value.as_str().unwrap_or(""),
        );
        return;
    }

    let edit = if let Some(field) = s.path.strip_prefix("/look/windows/") {
        Edit::new(
            crate::docs::DocId::Studio,
            format!("/panel/windows/{field}"),
            s.label,
        )
    } else {
        Edit::new(s.doc, s.path, s.label)
    };
    let h = row_height(b, caption);
    let r = Rect::new(c.x, c.y, c.w, h);
    let text_h = if caption.is_empty() {
        h
    } else {
        height(b, 24.0)
    };
    let y = if caption.is_empty() {
        c.y
    } else {
        c.y + b.px(6.0)
    };
    let label_w = match s.kind {
        Kind::Switch { .. } => c.w - b.px(48.0),
        Kind::Text { .. } => c.w,
        _ if c.w < b.px(360.0) || b.t.text_scale > 1.0 => c.w,
        _ => label_width(b, c),
    };
    let label_op = b.list.ops.len();
    text(
        b,
        Rect::new(c.x, y, label_w, text_h),
        label,
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    if !caption.is_empty() {
        text(
            b,
            Rect::new(c.x, y + text_h, c.w - b.px(48.0), height(b, 18.0)),
            caption,
            12.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
    match s.kind {
        Kind::Switch { .. } => {
            let on = value.as_bool().unwrap_or(false);
            b.switch(
                key(("panel-setting", id)),
                Point::new(r.right - b.px(36.0), c.y + (h - b.px(20.0)) / 2.0),
                on,
                label,
                Action::Toggle(edit, on),
            );
            c.y += h;
        }
        Kind::Select { options, .. } => {
            b.list.ops.truncate(label_op);
            super::editors::select(
                b,
                c,
                label,
                value,
                edit,
                options
                    .iter()
                    .map(|(value, label)| {
                        (
                            serde_json::from_str(value).expect("setting value"),
                            (*label).to_owned(),
                        )
                    })
                    .collect(),
            );
        }
        Kind::Choice { options, .. } => {
            let mut options: Vec<_> = options.iter().collect();
            options.sort_by_key(|(_, label)| *label != "As is");
            b.list.ops.truncate(label_op);
            super::editors::segments(
                b,
                c,
                label,
                value,
                edit,
                options
                    .into_iter()
                    .map(|(value, label)| {
                        (
                            serde_json::from_str(value).expect("setting value"),
                            (*label).to_owned(),
                        )
                    })
                    .collect(),
            );
        }
        Kind::Slider {
            min,
            max,
            step,
            unit,
            factor,
            default,
        } => {
            let value = value.as_f64().unwrap_or(default);
            let stacked = c.w < b.px(360.0) || b.t.text_scale > 1.0;
            let control = if stacked {
                Rect::new(c.x, c.y + height(b, 28.0), c.w, height(b, 36.0))
            } else {
                Rect::new(c.x + label_w + b.px(8.0), c.y, c.w - label_w - b.px(8.0), h)
            };
            b.slider(
                key(("panel-setting", id)),
                control,
                value,
                (min, max, step),
                &self::unit(value * factor, unit),
                b.px(48.0),
                label,
                Action::Slider(edit),
            );
            c.y = control.bottom + if stacked { b.px(4.0) } else { 0.0 };
        }
        Kind::Text { placeholder } => {
            let control = Rect::new(c.x, c.y + height(b, 28.0), c.w, height(b, 32.0));
            crate::ui::text_field(
                app,
                b,
                control,
                FieldId::Text(s.doc, s.path.into()),
                None,
                placeholder,
                value.as_str().unwrap_or(""),
                label,
            );
            b.stroke(control, b.control_radius(control), b.px(1.0), b.p.edge);
            c.y = control.bottom + b.px(8.0);
        }
        Kind::Custom => c.y += h,
    }
    b.hline(c.x, c.right(), c.y - 1.0, b.p.line.with_alpha(0.45));
}

pub fn search(app: &App, b: &mut B, c: &mut Col) {
    let id = FieldId::Palette;
    let k = field_key(&id);
    let r = Rect::new(c.x, c.y, c.w, height(b, 32.0));
    b.fill(r, b.control_radius(r), b.p.fill);
    b.stroke(r, b.control_radius(r), b.px(1.0), b.p.line);
    b.icon(
        Rect::new(r.left + b.px(10.0), r.top, b.px(16.0), r.height()),
        glyph::SEARCH,
        15.0,
        b.p.ink3,
    );
    let inner = Rect::new(
        r.left + b.px(34.0),
        r.top + b.px(4.0),
        r.width() - b.px(96.0),
        r.height() - b.px(8.0),
    );
    if let Some(f) = app.field.as_ref().filter(|f| f.id == id) {
        f.field.set_rect(inner);
        b.list.ops.push(glaze_ui::Op::TextField {
            rect: inner,
            field: f.field.clone(),
            revision: f.field.revision(),
        });
    } else {
        let value = app.text(&id);
        text(
            b,
            inner,
            if value.is_empty() {
                "Search everything"
            } else {
                value
            },
            13.0,
            400,
            b.p.ink3,
            Align::Start,
        );
    }
    text(
        b,
        Rect::new(r.right - b.px(58.0), r.top, b.px(48.0), r.height()),
        "Ctrl K",
        11.0,
        500,
        b.p.ink3,
        Align::End,
    );
    b.hit(
        k,
        r,
        Role::TextInput,
        "Search everything",
        Action::Field(id.clone()),
        Behavior::Field,
    );
    let value = app
        .field
        .as_ref()
        .filter(|field| field.id == id)
        .and_then(|field| field.field.text().ok())
        .unwrap_or_else(|| app.text(&id).to_owned());
    b.semantics(k, glaze_ui::Semantics::Text(value.into()));
    b.focus_ring(k, r, b.px(radius::CONTROL));
    c.y += r.height();
}

pub fn more(app: &App, b: &mut B, c: &mut Col, hint: &str) -> bool {
    c.y += b.px(8.0);
    let r = Rect::new(c.x, c.y, c.w, height(b, 36.0));
    let k = key(("panel-more", c.view));
    let open = app.is_open(k);
    let label_width = b.measure("More", &font(b, 13.0, 500)).0 + b.px(12.0);
    text(b, r, "More", 13.0, 500, b.p.ink2, Align::Start);
    text(
        b,
        Rect::new(
            r.left + label_width,
            r.top,
            r.width() - label_width - b.px(28.0),
            r.height(),
        ),
        hint,
        12.0,
        400,
        b.p.ink3,
        Align::Start,
    );
    b.icon(
        Rect::new(r.right - b.px(16.0), r.top, b.px(16.0), r.height()),
        if open {
            glaze_ui::icons::Icon::ChevronUp.into()
        } else {
            glyph::CHEVRON_DOWN
        },
        15.0,
        b.p.ink3,
    );
    b.hit_checked(
        k,
        r,
        Role::Button,
        "More",
        Some(open),
        Action::More(k),
        Behavior::Click,
    );
    b.focus_ring(k, r, b.px(radius::CONTROL));
    c.y += r.height();
    open
}

pub fn notice(b: &mut B, c: &mut Col, message: &str) {
    let style = font(b, 12.0, 400);
    c.y += b.paragraph(c.x, c.y, c.w, message, &style, height(b, 18.0), b.p.ink3) + b.px(12.0);
}

/// Give fields and toggles the panel's visible boundary; sliders and segments own their fills.
/// Run inside the body's scroll clip, after its controls have registered their geometry.
pub fn boundaries(b: &mut B) {
    for target in b.targets.clone() {
        match target.behavior {
            Behavior::Toggle => b.stroke(
                target.rect.inflate(-b.px(4.0)),
                b.px(radius::MEDIUM),
                b.px(1.0),
                b.p.edge,
            ),
            Behavior::Field => b.stroke(
                target.rect,
                b.control_radius(target.rect),
                b.px(1.0),
                b.p.edge,
            ),
            _ => match target.action {
                Action::Choose(_) if target.rect.height() < b.px(36.0) => b.stroke(
                    target.rect,
                    b.control_radius(target.rect),
                    b.px(1.0),
                    b.p.edge,
                ),
                _ => {}
            },
        }
    }
}
