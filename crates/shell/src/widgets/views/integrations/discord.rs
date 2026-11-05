use super::*;
use crate::integrations::widgets::{ITEM, PRIMARY};
use mypaper_integrations::Data;
use mypaper_ui::kit::{Accessory, Lead, Row};

pub(crate) fn view(f: &mut Frame<'_>, m: &Model, rect: Rect, _now_ms: i64) {
    let l = layout(f, rect, "Discord", Icon::Headphones, true);
    let Some(Data::Notifications(state)) = &m.integration.snapshot.data else {
        setup(
            f,
            m,
            l,
            "Connect Discord",
            "Allow Windows notifications in Settings. Voice controls need an approved Discord RPC application.",
            Icon::Bell,
        );
        return;
    };
    let id = m.str("appId", "com.squirrel.Discord.Discord");
    let cards: Vec<_> = state
        .cards
        .iter()
        .filter(|card| card.app_id == id)
        .collect();
    let (open, body) = l.body.take_bottom(f.px(30.0), f.px(7.0));
    f.button(
        PRIMARY,
        open,
        "Open Discord",
        Some(Icon::External),
        ButtonKind::Tinted,
        true,
    );
    if l.compact {
        metric(
            f,
            body,
            &cards.len().to_string(),
            "saved notifications",
            f.theme.color.label,
        );
    } else {
        let (summary, rows) = if l.large {
            body.take_top(f.px(55.0), f.px(7.0))
        } else {
            body.take_left(f.px(100.0), f.px(8.0))
        };
        metric(
            f,
            summary,
            &cards.len().to_string(),
            "saved notifications",
            f.theme.color.label,
        );
        let row_height = f.px(47.0);
        for (index, card) in cards
            .iter()
            .enumerate()
            .take((rows.height() / row_height).floor().max(0.0) as usize)
        {
            f.row(
                &Row {
                    id: ITEM + index as u32,
                    lead: Lead::Icon(Icon::Message),
                    title: if card.title.is_empty() {
                        "Discord notification"
                    } else {
                        &card.title
                    },
                    subtitle: Some(if card.body.is_empty() {
                        "Open Discord to read it"
                    } else {
                        &card.body
                    }),
                    accessory: Accessory::Chevron,
                    selected: false,
                    enabled: true,
                },
                Rect::new(
                    rows.left,
                    rows.top + index as f32 * row_height,
                    rows.width(),
                    row_height,
                ),
            );
        }
        if cards.is_empty() {
            f.text_in(
                rows,
                "All quiet",
                TextRole::Callout,
                f.theme.color.secondary,
                Align::Center,
            );
        }
    }
    footer(f, m, l.footer, "Windows notifications · not unread count");
}
