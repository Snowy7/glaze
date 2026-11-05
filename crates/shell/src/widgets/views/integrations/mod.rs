//! Integration faces use the public kit directly. No provider reads or side effects in a view.
pub(crate) mod discord;
pub(crate) mod calendar;
pub(crate) mod custom;
pub(crate) mod grok;
pub(crate) mod obs;
pub(crate) mod t3;
mod t3_limits;
mod t3_usage;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod custom_tests;

use crate::integrations::widgets::{CONNECT, REFRESH};
use crate::widgets::model::Model;
use mypaper_integrations::Availability;
use mypaper_ui::{
    HitId, Rect, Rgba, Role,
    icons::Icon,
    kit::{ButtonKind, Frame},
    layout::{Align, Insets},
    theme::TextRole,
};

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub body: Rect,
    pub footer: Rect,
    pub compact: bool,
    pub large: bool,
}

pub(super) fn layout(
    f: &mut Frame<'_>,
    rect: Rect,
    title: &str,
    icon: Icon,
    refresh: bool,
) -> Layout {
    let compact = rect.width() / f.s < 240.0;
    let inner = rect.inset(Insets::all(f.px(if compact { 14.0 } else { 16.0 })));
    let (header, content) = inner.take_top(f.px(24.0), f.px(10.0));
    let (symbol, label) = header.take_left(f.px(20.0), f.px(7.0));
    f.icon(symbol, icon, 17.0, f.theme.color.accent);
    let (control, label) = label.take_right(f.px(24.0), f.px(2.0));
    f.text_in(
        label,
        title,
        TextRole::Subheadline,
        f.theme.color.secondary,
        Align::Start,
    );
    if refresh {
        f.icon_button(
            REFRESH,
            control,
            Icon::Refresh,
            24.0,
            "Refresh integration",
            false,
        );
    }
    let (footer, body) = content.take_bottom(f.px(19.0), f.px(5.0));
    Layout {
        body,
        footer,
        compact,
        large: rect.height() / f.s >= 280.0,
    }
}

pub(super) fn footer(f: &mut Frame<'_>, m: &Model, area: Rect, default: &str) {
    let state = &m.integration;
    let (label, color) = if let Some(error) = state
        .notice
        .as_ref()
        .or(state.snapshot.action_error.as_ref())
    {
        (error.as_str(), f.theme.color.destructive)
    } else {
        match &state.snapshot.availability {
            Availability::Cached => ("Snapshot · refresh to update", f.theme.color.tertiary),
            Availability::Stale(_) => ("Disconnected · showing last state", f.theme.color.warning),
            Availability::Unavailable(error) => (error.as_str(), f.theme.color.tertiary),
            Availability::Connecting => ("Connecting…", f.theme.color.tertiary),
            Availability::Live => (default, f.theme.color.tertiary),
        }
    };
    f.text_in(area, label, TextRole::Caption, color, Align::Start);
}

pub(super) fn setup(
    f: &mut Frame<'_>,
    m: &Model,
    layout: Layout,
    title: &str,
    detail: &str,
    icon: Icon,
) {
    let (button, content) = layout.body.take_bottom(f.px(30.0), f.px(8.0));
    let title_height = f.line(TextRole::Headline);
    let (heading, content) = content.take_top(title_height, f.px(4.0));
    f.text_in(
        heading,
        title,
        TextRole::Headline,
        f.theme.color.label,
        Align::Start,
    );
    if !layout.compact {
        f.paragraph(
            content.left,
            content.top,
            content.width(),
            detail,
            TextRole::Callout,
            f.theme.color.secondary,
            if layout.large { 4 } else { 2 },
        );
    }
    let connecting =
        m.integration.busy() || m.integration.snapshot.availability == Availability::Connecting;
    f.button(
        CONNECT,
        button,
        if connecting { "Connecting…" } else { title },
        Some(icon),
        ButtonKind::Tinted,
        !connecting,
    );
    footer(f, m, layout.footer, "Ready when you are");
}

pub(super) fn metric(f: &mut Frame<'_>, rect: Rect, value: &str, caption: &str, color: Rgba) {
    let (number, label) = rect.take_top(f.px(35.0), f.px(1.0));
    f.text_in(number, value, TextRole::LargeTitle, color, Align::Start);
    f.text_in(
        label,
        caption,
        TextRole::Caption,
        f.theme.color.secondary,
        Align::Start,
    );
}

pub(super) fn pill(f: &mut Frame<'_>, rect: Rect, text: &str, color: Rgba) {
    f.badge(rect.left, rect.center().y, text, Some(color));
}

/// Compact actions need a disabled state as well as an accessible label.
pub(super) fn icon_action(
    f: &mut Frame<'_>,
    id: HitId,
    rect: Rect,
    icon: Icon,
    label: &str,
    enabled: bool,
) {
    let rect = rect.place(f.px(28.0), f.px(28.0), Align::Center, Align::Center);
    let radius = rect.width() * 0.5;
    f.wash(id, rect, radius, enabled);
    f.icon(
        rect,
        icon,
        16.0,
        if enabled {
            f.theme.color.label
        } else {
            f.theme.color.quaternary
        },
    );
    f.focus_ring(id, rect, radius);
    f.hit(id, rect, Role::Button, label, enabled, None);
}
