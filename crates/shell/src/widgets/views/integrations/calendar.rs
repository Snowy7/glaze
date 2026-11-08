//! Account controls wrap the calendar's existing month/agenda renderer.
use crate::integrations::calendar::*;
use crate::widgets::views::Ctx;
use glaze_integrations::Data;
use glaze_ui::{Align, ButtonKind, Rect, TextRole, icons::Icon, layout::SizeClass};

pub(crate) fn controls(c: &mut Ctx, r: Rect) -> Rect {
    if c.m.event_draft.is_some() {
        return r;
    }
    let class = SizeClass::of(c.rect.width() / c.s(), c.rect.height() / c.s());
    let expanded = c.m.integration.calendar.show_sources;
    let (footer, body) = r.take_bottom(
        c.px(if class == SizeClass::Compact {
            20.0
        } else {
            24.0
        }),
        c.px(5.0),
    );
    let (refresh, label) = footer.take_right(c.px(24.0), c.px(3.0));
    c.f.button(
        SOURCES,
        label,
        if expanded {
            "Back to calendar"
        } else {
            "Calendars"
        },
        Some(if expanded {
            Icon::ChevronLeft
        } else {
            Icon::Calendar
        }),
        ButtonKind::Plain,
        true,
    );
    c.f.icon_button(
        REFRESH,
        refresh,
        Icon::Refresh,
        22.0,
        "Refresh calendars",
        false,
    );
    if !expanded {
        return body;
    }
    let busy = c.m.integration.calendar.busy();
    let notice = c.m.integration.calendar.notice.clone();
    let sources = match &c.m.integration.snapshot.data {
        Some(Data::Calendar(s)) => s.sources.clone(),
        _ => Default::default(),
    };
    let color = c.color();
    let row = c.px(48.0);
    let height = (row + c.px(5.0)) * 3.0 + c.px(if notice.is_some() { 62.0 } else { 44.0 });
    c.scroll(body, height, |c, origin| {
        let mut y = origin;
        for (name, key, id, disconnect) in [
            ("On this PC", "Windows", WINDOWS, None),
            ("Google Calendar", "Google", GOOGLE, Some(DISCONNECT_GOOGLE)),
            (
                "Microsoft / Outlook",
                "Microsoft",
                MICROSOFT,
                Some(DISCONNECT_MICROSOFT),
            ),
        ] {
            let at = Rect::new(body.left, y, body.width(), row);
            let (action, title) = at.take_right(
                c.px(if class == SizeClass::Large {
                    100.0
                } else {
                    78.0
                }),
                c.px(4.0),
            );
            let (title, detail) = title.take_top(c.px(20.0), 0.0);
            c.f.text_in(title, name, TextRole::Callout, color.label, Align::Start);
            let status = sources
                .get(key)
                .map(String::as_str)
                .unwrap_or("Not connected");
            c.f.text_in(
                detail,
                status,
                TextRole::Caption,
                color.tertiary,
                Align::Start,
            );
            let button = action.place(action.width(), c.px(28.0), Align::Center, Align::Center);
            let button = if status != "Not connected"
                && let Some(disconnect) = disconnect
            {
                let (remove, button) = button.take_right(c.px(22.0), c.px(3.0));
                c.f.icon_button(
                    disconnect,
                    remove,
                    Icon::Close,
                    18.0,
                    &format!("Disconnect {key}"),
                    false,
                );
                button
            } else {
                button
            };
            c.f.button(
                id,
                button,
                if id == WINDOWS { "Read" } else { "Connect" },
                None,
                ButtonKind::Tinted,
                !busy,
            );
            y += row + c.px(5.0);
        }
        c.f.paragraph(
            body.left,
            y,
            body.width(),
            notice.as_deref().unwrap_or(
                "Read-only · merged with your local events. Cloud events update when you refresh.",
            ),
            TextRole::Caption,
            if notice.is_some() {
                color.warning
            } else {
                color.tertiary
            },
            if notice.is_some() { 3 } else { 2 },
        );
    });
    Rect::new(0.0, 0.0, 0.0, 0.0)
}
